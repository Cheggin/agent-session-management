use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use chrono::{Duration as ChronoDuration, Utc};
use sysinfo::{Process, ProcessRefreshKind, RefreshKind, System, UpdateKind};
use tracing::warn;
use uuid::Uuid;

use crate::Session;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LivenessSnapshot {
    pub confirmed: HashSet<String>,
    pub maybe: HashSet<String>,
}

pub fn live_session_ids(sessions: &[Session]) -> LivenessSnapshot {
    if !sysinfo::IS_SUPPORTED_SYSTEM {
        warn!("sysinfo process scanning is not supported on this platform");
        return LivenessSnapshot::default();
    }

    match std::panic::catch_unwind(|| live_session_ids_inner(sessions)) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            warn!("sysinfo process scan failed while detecting live sessions");
            LivenessSnapshot::default()
        }
    }
}

pub fn mark_live_sessions(sessions: &mut [Session]) {
    if sessions.is_empty() {
        return;
    }

    let snapshot = live_session_ids(sessions);
    for session in sessions {
        let confirmed = snapshot.confirmed.contains(&session.id);
        let maybe = snapshot.maybe.contains(&session.id);
        session.is_live = confirmed || maybe;
        session.maybe_live = maybe && !confirmed;
    }
}

fn live_session_ids_inner(sessions: &[Session]) -> LivenessSnapshot {
    let refresh_kind = ProcessRefreshKind::new()
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::OnlyIfNotSet);
    let system = System::new_with_specifics(RefreshKind::new().with_processes(refresh_kind));

    let mut confirmed = HashSet::new();
    let mut maybe_cwds = Vec::new();

    for process in system
        .processes()
        .values()
        .filter(|process| is_agent_process(process))
    {
        if let Some(session_id) = resume_session_id(process) {
            confirmed.insert(session_id);
        } else if let Some(cwd) = process.cwd() {
            maybe_cwds.push(cwd.to_path_buf());
        }
    }

    let maybe = maybe_live_session_ids(sessions, &maybe_cwds, &confirmed);
    LivenessSnapshot { confirmed, maybe }
}

fn maybe_live_session_ids(
    sessions: &[Session],
    process_cwds: &[PathBuf],
    confirmed: &HashSet<String>,
) -> HashSet<String> {
    if process_cwds.is_empty() {
        return HashSet::new();
    }

    let now = Utc::now();
    sessions
        .iter()
        .filter(|session| !confirmed.contains(&session.id))
        .filter(|session| has_recent_user_message(session, now))
        .filter(|session| {
            session.cwd.as_deref().is_some_and(|session_cwd| {
                process_cwds
                    .iter()
                    .any(|process_cwd| session_cwd.starts_with(process_cwd))
            })
        })
        .map(|session| session.id.clone())
        .collect()
}

fn has_recent_user_message(session: &Session, now: chrono::DateTime<Utc>) -> bool {
    session.last_user_msg_at.is_some_and(|last_user_msg_at| {
        last_user_msg_at <= now
            && now.signed_duration_since(last_user_msg_at) <= ChronoDuration::minutes(5)
    })
}

fn is_agent_process(process: &Process) -> bool {
    let process_name = process.name().to_string_lossy();
    let argv = process_argv(process);
    is_agent_invocation(&process_name, &argv)
}

fn resume_session_id(process: &Process) -> Option<String> {
    let argv = process_argv(process);
    resume_session_id_from_argv(&argv)
}

fn process_argv(process: &Process) -> Vec<String> {
    process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[doc(hidden)]
pub fn is_agent_invocation<S>(process_name: &str, argv: &[S]) -> bool
where
    S: AsRef<str>,
{
    is_agent_name(process_name) || argv.iter().any(|arg| is_agent_arg(arg.as_ref()))
}

#[doc(hidden)]
pub fn resume_session_id_from_argv<S>(argv: &[S]) -> Option<String>
where
    S: AsRef<str>,
{
    for window in argv.windows(2) {
        let flag = window[0].as_ref();
        let value = window[1].as_ref();
        if matches!(flag, "--resume" | "resume") && is_uuid_shape(value) {
            return Some(value.to_string());
        }
    }

    None
}

fn is_agent_arg(value: &str) -> bool {
    AGENT_PATH_MARKERS
        .iter()
        .any(|marker| value.contains(marker))
        || Path::new(value)
            .file_name()
            .and_then(|name| name.to_str())
            .map(strip_node_script_suffix)
            .is_some_and(is_agent_name)
}

fn strip_node_script_suffix(value: &str) -> &str {
    for suffix in [".js", ".mjs", ".cjs"] {
        if let Some(stripped) = value.strip_suffix(suffix) {
            return stripped;
        }
    }

    value
}

fn is_agent_name(value: &str) -> bool {
    matches!(value, "claude" | "codex")
}

const AGENT_PATH_MARKERS: &[&str] = &[
    "@openai/codex/bin/",
    "@anthropic-ai/claude-code/",
    "/claude-code/bin/",
    "/codex/bin/",
];

fn is_uuid_shape(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}
