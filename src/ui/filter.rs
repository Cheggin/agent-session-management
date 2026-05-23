use std::{
    cmp::Ordering,
    collections::HashMap,
    path::{Path, PathBuf},
};

use chrono::{Duration, Utc};
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::{Agent, Entrypoint, Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chip {
    Agent(Agent),
    Running,
    HereCwd(PathBuf),
    Recency(Duration),
    Branch(String),
    PathSubstring(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFilter {
    pub chips: Vec<Chip>,
    pub search: String,
}

pub fn parse_filter_text(input: &str, current_cwd: &Path) -> ParsedFilter {
    let mut chips = Vec::new();
    let mut search_terms = Vec::new();
    let current_cwd = canonical_path(current_cwd);

    for token in input.split_whitespace() {
        match parse_chip(token, &current_cwd) {
            Some(chip) => chips.push(chip),
            None => search_terms.push(token.to_owned()),
        }
    }

    ParsedFilter {
        chips,
        search: search_terms.join(" "),
    }
}

pub fn apply_filters(sessions: &[Session], chips: &[Chip], search: &str) -> Vec<usize> {
    let haystacks = build_session_haystacks(sessions, &HashMap::new());
    apply_filters_with_haystacks(sessions, chips, search, &haystacks)
}

pub fn apply_filters_with_haystacks(
    sessions: &[Session],
    chips: &[Chip],
    search: &str,
    session_haystacks: &[String],
) -> Vec<usize> {
    let pattern = (!search.trim().is_empty())
        .then(|| Pattern::parse(search.trim(), CaseMatching::Ignore, Normalization::Smart));
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut utf32_buf = Vec::new();

    let mut indices = Vec::new();
    for (idx, session) in sessions.iter().enumerate() {
        // Drop non-interactive automation sessions from the default session list.
        if session.is_sidechain
            || session.user_msg_count == 0
            || matches!(
                session.entrypoint.as_ref(),
                Some(Entrypoint::Sdk | Entrypoint::Exec)
            )
        {
            continue;
        }
        if !chips.iter().all(|chip| matches_chip(session, chip)) {
            continue;
        }
        if let Some(pattern) = &pattern {
            let fallback_haystack;
            let haystack = if let Some(haystack) = session_haystacks.get(idx) {
                haystack.as_str()
            } else {
                fallback_haystack = session_haystack(session, None);
                fallback_haystack.as_str()
            };
            if pattern
                .score(Utf32Str::new(haystack, &mut utf32_buf), &mut matcher)
                .is_none()
            {
                continue;
            }
        }
        indices.push(idx);
    }

    indices.sort_by(|left, right| compare_sessions(&sessions[*left], &sessions[*right]));
    indices
}

pub fn build_session_haystacks(
    sessions: &[Session],
    message_bodies: &HashMap<String, String>,
) -> Vec<String> {
    build_session_haystacks_optional(sessions, Some(message_bodies))
}

pub(crate) fn build_session_haystacks_optional(
    sessions: &[Session],
    message_bodies: Option<&HashMap<String, String>>,
) -> Vec<String> {
    sessions
        .iter()
        .map(|session| {
            session_haystack(
                session,
                message_bodies
                    .and_then(|bodies| bodies.get(&session.id))
                    .map(String::as_str),
            )
        })
        .collect()
}

pub fn chip_label(chip: &Chip) -> String {
    match chip {
        Chip::Agent(Agent::Claude) => "@claude".to_owned(),
        Chip::Agent(Agent::Codex) => "@codex".to_owned(),
        Chip::Running => "@running".to_owned(),
        Chip::HereCwd(_) => "@here".to_owned(),
        Chip::Recency(duration) if *duration == Duration::days(1) => "@today".to_owned(),
        Chip::Recency(duration) if *duration == Duration::days(7) => "@week".to_owned(),
        Chip::Recency(duration) => format!("@{}d", duration.num_days()),
        Chip::Branch(branch) => format!("@branch:{branch}"),
        Chip::PathSubstring(path) => format!("@path:{path}"),
    }
}

pub(crate) fn is_sql_pushdown_chip(chip: &Chip) -> bool {
    matches!(
        chip,
        Chip::HereCwd(_) | Chip::Agent(_) | Chip::Branch(_) | Chip::PathSubstring(_)
    )
}

pub(crate) fn parse_chip(token: &str, current_cwd: &Path) -> Option<Chip> {
    match token {
        "@claude" => Some(Chip::Agent(Agent::Claude)),
        "@codex" => Some(Chip::Agent(Agent::Codex)),
        "@running" => Some(Chip::Running),
        "@here" => Some(Chip::HereCwd(current_cwd.to_path_buf())),
        "@today" => Some(Chip::Recency(Duration::days(1))),
        "@week" => Some(Chip::Recency(Duration::days(7))),
        _ => token
            .strip_prefix("@branch:")
            .filter(|value| !value.is_empty())
            .map(|value| Chip::Branch(value.to_owned()))
            .or_else(|| {
                token
                    .strip_prefix("@path:")
                    .filter(|value| !value.is_empty())
                    .map(|value| Chip::PathSubstring(value.to_owned()))
            }),
    }
}

fn matches_chip(session: &Session, chip: &Chip) -> bool {
    match chip {
        Chip::Agent(agent) => session.agent == *agent,
        Chip::Running => session.is_live,
        Chip::HereCwd(current_cwd) => session
            .cwd
            .as_deref()
            .map(canonical_path)
            .is_some_and(|cwd| cwd.starts_with(current_cwd)),
        Chip::Recency(duration) => {
            let activity = session.last_user_msg_at.unwrap_or(session.started_at);
            Utc::now().signed_duration_since(activity) <= *duration
        }
        Chip::Branch(expected) => session
            .git_branch
            .as_deref()
            .is_some_and(|branch| branch.eq_ignore_ascii_case(expected)),
        Chip::PathSubstring(needle) => session.cwd.as_deref().is_some_and(|cwd| {
            cwd.to_string_lossy()
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase())
        }),
    }
}

fn session_haystack(session: &Session, message_body: Option<&str>) -> String {
    format!(
        "{} {} {} {} {}",
        session.title.as_deref().unwrap_or_default(),
        session.first_user_prompt.as_deref().unwrap_or_default(),
        session
            .cwd
            .as_deref()
            .map(|cwd| cwd.to_string_lossy())
            .unwrap_or_default(),
        session.git_branch.as_deref().unwrap_or_default(),
        message_body.unwrap_or_default(),
    )
}

fn compare_sessions(left: &Session, right: &Session) -> Ordering {
    right
        .is_live
        .cmp(&left.is_live)
        .then_with(|| compare_last_user_desc(left, right))
        .then_with(|| right.started_at.cmp(&left.started_at))
        .then_with(|| left.id.cmp(&right.id))
}

fn compare_last_user_desc(left: &Session, right: &Session) -> Ordering {
    match (
        left.last_user_msg_at.as_ref(),
        right.last_user_msg_at.as_ref(),
    ) {
        (Some(left), Some(right)) => right.cmp(left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn canonical_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
