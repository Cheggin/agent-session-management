use std::collections::HashSet;

use asm::liveness::{is_agent_invocation, live_session_ids, resume_session_id_from_argv};
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

#[test]
fn detects_node_wrapper_agent_invocations_and_resume_ids() {
    const SESSION_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    for argv in [
        vec!["/opt/homebrew/bin/codex", "resume", SESSION_ID],
        vec![
            "/usr/local/bin/node",
            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            "resume",
            SESSION_ID,
        ],
        vec![
            "node",
            "/Users/reagan/.npm/_npx/123/node_modules/@anthropic-ai/claude-code/cli.js",
            "--resume",
            SESSION_ID,
        ],
        vec![
            "node",
            "--experimental-loader",
            "tsx",
            "wrapper",
            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            "resume",
            SESSION_ID,
        ],
    ] {
        assert!(
            is_agent_invocation("node", &argv),
            "argv should be agent-flavored: {argv:?}"
        );
        assert_eq!(
            resume_session_id_from_argv(&argv).as_deref(),
            Some(SESSION_ID)
        );
    }
}

#[test]
fn rejects_non_agent_invocations_and_allows_maybe_live_agents() {
    let rg = vec!["rg", "foo"];
    assert!(!is_agent_invocation("rg", &rg));
    assert_eq!(resume_session_id_from_argv(&rg), None);

    let claude = vec!["claude"];
    assert!(is_agent_invocation("claude", &claude));
    assert_eq!(resume_session_id_from_argv(&claude), None);
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
        .filter_map(|process| {
            let argv = process_argv(process);
            let process_name = process.name().to_string_lossy();
            is_agent_invocation(&process_name, &argv)
                .then(|| resume_session_id_from_argv(&argv))
                .flatten()
        })
        .collect()
}

fn process_argv(process: &Process) -> Vec<String> {
    process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}
