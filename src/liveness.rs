use std::{
    collections::HashSet,
    ffi::OsStr,
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

    None
}

fn is_agent_name(value: &OsStr) -> bool {
    matches!(value.to_str(), Some("claude" | "codex"))
}

fn is_uuid_shape(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}
