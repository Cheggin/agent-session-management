use std::{collections::HashSet, ffi::OsStr, path::Path};

use asm::liveness::live_session_ids;
use sysinfo::{Process, ProcessRefreshKind, RefreshKind, System, UpdateKind};
use uuid::Uuid;

#[test]
fn live_session_ids_returns_confirmed_resume_ids_without_panicking() {
    let snapshot = live_session_ids(&[]);
    let possible_ids = currently_running_agent_resume_ids();

    assert!(
        snapshot
            .confirmed
            .iter()
            .all(|id| Uuid::parse_str(id).is_ok())
    );
    assert!(
        snapshot.confirmed.is_subset(&possible_ids),
        "live ids {:?} should be a subset of running claude/codex resume ids {possible_ids:?}",
        snapshot.confirmed
    );
    assert!(snapshot.maybe.is_empty());
    assert!(snapshot.confirmed.is_disjoint(&snapshot.maybe));
}

fn currently_running_agent_resume_ids() -> HashSet<String> {
    if !sysinfo::IS_SUPPORTED_SYSTEM {
        return HashSet::new();
    }

    let refresh_kind = ProcessRefreshKind::new().with_cmd(UpdateKind::OnlyIfNotSet);
    let system = System::new_with_specifics(RefreshKind::new().with_processes(refresh_kind));

    system
        .processes()
        .values()
        .filter(|process| is_agent_process(process))
        .filter_map(resume_session_id)
        .collect()
}

fn is_agent_process(process: &Process) -> bool {
    is_agent_name(process.name())
        || process
            .cmd()
            .iter()
            .take(3)
            .filter_map(|arg| Path::new(arg).file_name())
            .any(is_agent_name)
}

fn resume_session_id(process: &Process) -> Option<String> {
    let argv: Vec<_> = process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect();

    for window in argv.windows(2) {
        if matches!(window[0].as_ref(), "--resume" | "resume")
            && Uuid::parse_str(&window[1]).is_ok()
        {
            return Some(window[1].to_string());
        }
    }

    None
}

fn is_agent_name(value: &OsStr) -> bool {
    matches!(value.to_str(), Some("claude" | "codex"))
}
