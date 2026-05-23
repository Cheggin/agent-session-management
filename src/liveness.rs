use std::{collections::HashSet, ffi::OsStr, path::Path};

use sysinfo::{Process, ProcessRefreshKind, RefreshKind, System, UpdateKind};
use tracing::warn;
use uuid::Uuid;

use crate::Session;

pub fn live_session_ids() -> HashSet<String> {
    if !sysinfo::IS_SUPPORTED_SYSTEM {
        warn!("sysinfo process scanning is not supported on this platform");
        return HashSet::new();
    }

    match std::panic::catch_unwind(live_session_ids_inner) {
        Ok(ids) => ids,
        Err(_) => {
            warn!("sysinfo process scan failed while detecting live sessions");
            HashSet::new()
        }
    }
}

pub fn mark_live_sessions(sessions: &mut [Session]) {
    if sessions.is_empty() {
        return;
    }

    let live_ids = live_session_ids();
    for session in sessions {
        session.is_live = live_ids.contains(&session.id);
    }
}

fn live_session_ids_inner() -> HashSet<String> {
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
        if matches!(window[0].as_ref(), "--resume" | "resume") && is_uuid_shape(&window[1]) {
            return Some(window[1].to_string());
        }
    }

    // TODO(step 9.5): use process.cwd() plus recent indexed sessions to surface
    // maybe-live matches separately from these confirmed resume-argv matches.
    None
}

fn is_agent_name(value: &OsStr) -> bool {
    matches!(value.to_str(), Some("claude" | "codex"))
}

fn is_uuid_shape(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}
