use std::{
    borrow::Cow,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::{Agent, Entrypoint, Parser, Session, TranscriptRole, TranscriptTurn};

const READ_BUF_CAP: usize = 128 * 1024;

pub struct CodexParser;

/// Top-level Codex JSONL event. `payload` is captured as RawValue so it's
/// only deserialized for the event types we actually use.
#[derive(Deserialize, Default)]
struct CodexEvent<'a> {
    #[serde(default, borrow, rename = "type")]
    event_type: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize, Default)]
struct CodexEventPayload<'a> {
    #[serde(default, borrow, rename = "type")]
    payload_type: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    message: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    last_agent_message: Option<Cow<'a, str>>,
}

#[derive(Deserialize, Default)]
struct CodexSessionMetaPayload<'a> {
    #[serde(default, borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    git: Option<CodexGit<'a>>,
    #[serde(default, borrow)]
    originator: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    thread_source: Option<Cow<'a, str>>,
}

#[derive(Deserialize, Default)]
struct CodexGit<'a> {
    #[serde(default, borrow)]
    branch: Option<Cow<'a, str>>,
}

impl CodexParser {
    pub fn extract_message_bodies(path: &Path) -> Result<Vec<String>> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Codex session {}", path.display()))?;
        let mut reader = BufReader::with_capacity(READ_BUF_CAP, file);
        let mut bodies = Vec::new();
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            let n = reader.read_line(&mut line_buf).with_context(|| {
                format!("failed to read Codex session line from {}", path.display())
            })?;
            if n == 0 {
                break;
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<CodexEvent<'_>>(trimmed) else {
                continue;
            };
            if event.event_type.as_deref() != Some("event_msg") {
                continue;
            }
            let Some(payload_raw) = event.payload else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<CodexEventPayload<'_>>(payload_raw.get())
            else {
                continue;
            };

            match payload.payload_type.as_deref() {
                Some("user_message") | Some("agent_message") => {
                    if let Some(message) = payload.message.as_deref().and_then(nonempty_trimmed) {
                        bodies.push(message);
                    }
                }
                Some("task_complete") => {
                    if let Some(message) = payload
                        .last_agent_message
                        .as_deref()
                        .and_then(nonempty_trimmed)
                    {
                        bodies.push(message);
                    }
                }
                _ => {}
            }
        }

        Ok(bodies)
    }

    pub fn extract_transcript(path: &Path) -> Result<Vec<TranscriptTurn>> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Codex session {}", path.display()))?;
        let mut reader = BufReader::with_capacity(READ_BUF_CAP, file);
        let mut turns = Vec::new();
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            let n = reader.read_line(&mut line_buf).with_context(|| {
                format!("failed to read Codex session line from {}", path.display())
            })?;
            if n == 0 {
                break;
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<CodexEvent<'_>>(trimmed) else {
                continue;
            };
            if event.event_type.as_deref() != Some("event_msg") {
                continue;
            }

            let timestamp = parse_timestamp_str(event.timestamp.as_deref());
            let Some(payload_raw) = event.payload else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<CodexEventPayload<'_>>(payload_raw.get())
            else {
                continue;
            };

            match payload.payload_type.as_deref() {
                Some("user_message") => {
                    if let Some(message) = payload.message.as_deref().and_then(nonempty_trimmed)
                        && !is_system_noise(&message)
                    {
                        turns.push(TranscriptTurn {
                            role: TranscriptRole::User,
                            timestamp,
                            text: message,
                        });
                    }
                }
                Some("agent_message") => {
                    if let Some(message) = payload.message.as_deref().and_then(nonempty_trimmed) {
                        turns.push(TranscriptTurn {
                            role: TranscriptRole::Assistant,
                            timestamp,
                            text: message,
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(turns)
    }
}

impl Parser for CodexParser {
    fn parse(path: &Path) -> Result<Session> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Codex session {}", path.display()))?;
        let mut reader = BufReader::with_capacity(READ_BUF_CAP, file);
        let mut line_buf = String::new();

        // Locate the first parseable event; it must be session_meta.
        let meta_text = read_session_meta_line(&mut reader, &mut line_buf, path)?;
        let meta_event: CodexEvent<'_> = serde_json::from_str(&meta_text).with_context(|| {
            format!("Codex session {} session_meta is malformed", path.display())
        })?;
        if meta_event.event_type.as_deref() != Some("session_meta") {
            bail!(
                "Codex session {} first parseable event is not session_meta",
                path.display()
            );
        }
        let payload_raw = meta_event.payload.with_context(|| {
            format!(
                "Codex session {} session_meta has no payload",
                path.display()
            )
        })?;
        let meta_payload: CodexSessionMetaPayload<'_> = serde_json::from_str(payload_raw.get())
            .with_context(|| {
                format!(
                    "Codex session {} session_meta payload is malformed",
                    path.display()
                )
            })?;

        let id = meta_payload
            .id
            .as_deref()
            .with_context(|| {
                format!(
                    "Codex session {} session_meta payload has no id",
                    path.display()
                )
            })?
            .to_owned();
        let started_at =
            parse_timestamp_str(meta_payload.timestamp.as_deref()).with_context(|| {
                format!(
                    "Codex session {} has no parseable payload timestamp",
                    path.display()
                )
            })?;
        let cwd = meta_payload.cwd.as_deref().map(PathBuf::from);
        let git_branch = meta_payload
            .git
            .as_ref()
            .and_then(|git| git.branch.as_deref())
            .map(str::to_owned);
        let entrypoint = meta_payload.originator.as_deref().map(parse_entrypoint);
        let is_sidechain = meta_payload
            .thread_source
            .as_deref()
            .is_some_and(|thread_source| thread_source == "subagent");

        let mut first_user_prompt: Option<String> = None;
        let mut recent_user_prompts = Vec::new();
        let mut last_agent_message: Option<String> = None;
        let mut last_task_complete_message: Option<String> = None;
        let mut last_user_msg_at: Option<DateTime<Utc>> = None;
        let mut last_assistant_msg_at: Option<DateTime<Utc>> = None;
        let mut user_msg_count = 0_u32;

        loop {
            line_buf.clear();
            let n = reader.read_line(&mut line_buf).with_context(|| {
                format!("failed to read Codex session line from {}", path.display())
            })?;
            if n == 0 {
                break;
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<CodexEvent<'_>>(trimmed) else {
                continue;
            };
            if event.event_type.as_deref() != Some("event_msg") {
                continue;
            }

            let timestamp = parse_timestamp_str(event.timestamp.as_deref());
            let Some(payload_raw) = event.payload else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<CodexEventPayload<'_>>(payload_raw.get())
            else {
                continue;
            };

            match payload.payload_type.as_deref() {
                Some("user_message") => {
                    user_msg_count += 1;
                    if let Some(message) = payload.message.as_deref() {
                        let trimmed_msg = message.trim();
                        if !trimmed_msg.is_empty() {
                            if first_user_prompt.is_none() {
                                first_user_prompt = Some(trimmed_msg.to_owned());
                            }
                            remember_recent_user_prompt(
                                &mut recent_user_prompts,
                                trimmed_msg.to_owned(),
                            );
                        }
                    }
                    if let Some(timestamp) = timestamp {
                        last_user_msg_at = Some(timestamp);
                    }
                }
                Some("agent_message") => {
                    if let Some(message) = payload.message.as_deref() {
                        last_agent_message = Some(message.to_owned());
                    }
                    if let Some(timestamp) = timestamp {
                        update_latest(&mut last_assistant_msg_at, timestamp);
                    }
                }
                Some("task_complete") => {
                    if let Some(message) = payload.last_agent_message.as_deref() {
                        last_task_complete_message = Some(message.to_owned());
                    }
                    if let Some(timestamp) = timestamp {
                        update_latest(&mut last_assistant_msg_at, timestamp);
                    }
                }
                _ => {}
            }
        }

        Ok(Session {
            id,
            agent: Agent::Codex,
            path: path.to_path_buf(),
            cwd,
            git_branch,
            entrypoint,
            title: first_user_prompt.clone(),
            first_user_prompt,
            recent_user_prompts,
            last_assistant_text: last_task_complete_message.or(last_agent_message),
            started_at,
            last_user_msg_at,
            last_assistant_msg_at,
            user_msg_count,
            is_live: false,
            maybe_live: false,
            is_sidechain,
        })
    }
}

fn read_session_meta_line(
    reader: &mut BufReader<File>,
    line_buf: &mut String,
    path: &Path,
) -> Result<String> {
    loop {
        line_buf.clear();
        let n = reader.read_line(line_buf).with_context(|| {
            format!("failed to read Codex session line from {}", path.display())
        })?;
        if n == 0 {
            bail!("Codex session {} has no session_meta event", path.display());
        }
        let trimmed = line_buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        return Ok(trimmed.to_owned());
    }
}

fn parse_timestamp_str(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn parse_entrypoint(value: &str) -> Entrypoint {
    match value {
        "codex-tui" => Entrypoint::Tui,
        "codex_exec" => Entrypoint::Exec,
        "codex_cli_rs" => Entrypoint::Cli,
        "vscode" => Entrypoint::Vscode,
        other => Entrypoint::Other(other.to_owned()),
    }
}

fn update_latest(latest: &mut Option<DateTime<Utc>>, timestamp: DateTime<Utc>) {
    if latest.is_none_or(|current| timestamp > current) {
        *latest = Some(timestamp);
    }
}

fn nonempty_trimmed(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn is_system_noise(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<task-notification>")
        || t.starts_with("<system-reminder>")
        || t.starts_with("<command-name>")
        || t.starts_with("<command-message>")
        || t.starts_with("<command-args>")
        || t.starts_with("<local-command-stdout>")
        || t.starts_with("<local-command-stderr>")
        || t.starts_with("<bash-input>")
        || t.starts_with("<bash-stdout>")
        || t.starts_with("<bash-stderr>")
}

fn remember_recent_user_prompt(recent_user_prompts: &mut Vec<String>, prompt: String) {
    recent_user_prompts.push(prompt);
    if recent_user_prompts.len() > 3 {
        recent_user_prompts.remove(0);
    }
}
