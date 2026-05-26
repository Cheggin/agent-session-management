use std::{
    borrow::Cow,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;

use crate::{Agent, Entrypoint, Parser, Session, TranscriptRole, TranscriptTurn};

const READ_BUF_CAP: usize = 128 * 1024;

pub struct ClaudeParser;

/// Top-level Claude JSONL event. `message` is captured as RawValue so the
/// (potentially huge) `message.content` tree is never deserialized during the
/// main walk — it's only decoded for the lines whose text we actually need
/// (user turns + the final assistant turn).
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ClaudeEvent<'a> {
    #[serde(default, borrow, rename = "type")]
    event_type: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    git_branch: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    entrypoint: Option<Cow<'a, str>>,
    #[serde(default)]
    is_sidechain: bool,
    #[serde(default)]
    is_meta: bool,
    #[serde(default, borrow)]
    ai_title: Option<Cow<'a, str>>,
    #[serde(default, borrow)]
    message: Option<&'a RawValue>,
}

#[derive(Deserialize, Default)]
struct ClaudeMessage<'a> {
    #[serde(default, borrow)]
    content: Option<&'a RawValue>,
}

impl ClaudeParser {
    pub fn extract_message_bodies(path: &Path) -> Result<Vec<String>> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Claude session {}", path.display()))?;
        let mut reader = BufReader::with_capacity(READ_BUF_CAP, file);
        let mut bodies = Vec::new();
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            let n = reader.read_line(&mut line_buf).with_context(|| {
                format!("failed to read Claude session line from {}", path.display())
            })?;
            if n == 0 {
                break;
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<ClaudeEvent<'_>>(trimmed) else {
                continue;
            };

            match event.event_type.as_deref() {
                Some("user") if !event.is_meta => {
                    if let Some(message) = event.message {
                        bodies.extend(
                            user_text_bodies(message)
                                .into_iter()
                                .filter(|t| !is_system_noise(t)),
                        );
                    }
                }
                Some("assistant") => {
                    if let Some(message) = event.message {
                        bodies.extend(assistant_text_bodies(message));
                    }
                }
                _ => {}
            }
        }

        Ok(bodies)
    }

    pub fn extract_transcript(path: &Path) -> Result<Vec<TranscriptTurn>> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Claude session {}", path.display()))?;
        let mut reader = BufReader::with_capacity(READ_BUF_CAP, file);
        let mut turns = Vec::new();
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            let n = reader.read_line(&mut line_buf).with_context(|| {
                format!("failed to read Claude session line from {}", path.display())
            })?;
            if n == 0 {
                break;
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<ClaudeEvent<'_>>(trimmed) else {
                continue;
            };
            let timestamp = parse_timestamp_str(event.timestamp.as_deref());

            match event.event_type.as_deref() {
                Some("user") if !event.is_meta => {
                    if let Some(message) = event.message {
                        turns.extend(
                            user_text_bodies(message)
                                .into_iter()
                                .filter(|text| !is_system_noise(text))
                                .map(|text| TranscriptTurn {
                                    role: TranscriptRole::User,
                                    timestamp,
                                    text,
                                }),
                        );
                    }
                }
                Some("assistant") => {
                    if let Some(message) = event.message.and_then(last_assistant_text) {
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

impl Parser for ClaudeParser {
    fn parse(path: &Path) -> Result<Session> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Claude session {}", path.display()))?;
        let mut reader = BufReader::with_capacity(READ_BUF_CAP, file);

        let id = path
            .file_stem()
            .context("Claude session path has no filename stem")?
            .to_string_lossy()
            .into_owned();

        let mut cwd: Option<PathBuf> = None;
        let mut git_branch: Option<String> = None;
        let mut saw_git_branch = false;
        let mut entrypoint: Option<Entrypoint> = None;
        let mut title: Option<String> = None;
        let mut first_user_prompt: Option<String> = None;
        let mut recent_user_prompts = Vec::new();
        // Raw text of the last assistant line. Parsed once at end to extract
        // the assistant text — avoids per-line content walking.
        let mut last_assistant_raw: Option<String> = None;
        let mut started_at: Option<DateTime<Utc>> = None;
        let mut last_user_msg_at: Option<DateTime<Utc>> = None;
        let mut last_assistant_msg_at: Option<DateTime<Utc>> = None;
        let mut user_msg_count = 0_u32;
        let mut is_sidechain = false;
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            let n = reader.read_line(&mut line_buf).with_context(|| {
                format!("failed to read Claude session line from {}", path.display())
            })?;
            if n == 0 {
                break;
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<ClaudeEvent<'_>>(trimmed) else {
                continue;
            };

            let timestamp = parse_timestamp_str(event.timestamp.as_deref());
            if started_at.is_none() {
                started_at = timestamp;
            }

            if cwd.is_none()
                && let Some(c) = event.cwd.as_deref()
            {
                cwd = Some(PathBuf::from(c));
            }

            if !saw_git_branch && let Some(branch) = event.git_branch.as_deref() {
                saw_git_branch = true;
                if branch != "HEAD" {
                    git_branch = Some(branch.to_owned());
                }
            }

            if entrypoint.is_none()
                && let Some(ep) = event.entrypoint.as_deref()
            {
                entrypoint = Some(parse_entrypoint(ep));
            }

            if event.is_sidechain {
                is_sidechain = true;
            }

            match event.event_type.as_deref() {
                Some("ai-title") => {
                    if let Some(ai_title) = event.ai_title.as_deref() {
                        title = Some(ai_title.to_owned());
                    }
                }
                Some("user") => {
                    if event.is_meta {
                        continue;
                    }
                    let Some(message_raw) = event.message else {
                        continue;
                    };
                    let Some(prompt) = first_user_text(message_raw) else {
                        continue;
                    };
                    if is_system_noise(&prompt) {
                        continue;
                    }

                    user_msg_count += 1;
                    if first_user_prompt.is_none() {
                        first_user_prompt = Some(prompt.clone());
                    }
                    remember_recent_user_prompt(&mut recent_user_prompts, prompt);
                    if let Some(timestamp) = timestamp {
                        last_user_msg_at = Some(timestamp);
                    }
                }
                Some("assistant") => {
                    if let Some(timestamp) = timestamp {
                        last_assistant_msg_at = Some(timestamp);
                    }
                    // Defer text extraction to end of file — only the last
                    // assistant message needs decoding.
                    last_assistant_raw = Some(trimmed.to_owned());
                }
                _ => {}
            }
        }

        let last_assistant_text = last_assistant_raw
            .as_deref()
            .and_then(|raw| serde_json::from_str::<ClaudeEvent<'_>>(raw).ok())
            .and_then(|event| event.message.and_then(last_assistant_text));

        let started_at = started_at.with_context(|| {
            format!(
                "Claude session {} has no parseable timestamped event",
                path.display()
            )
        })?;

        Ok(Session {
            id,
            agent: Agent::Claude,
            path: path.to_path_buf(),
            cwd,
            git_branch,
            entrypoint,
            title: title.or_else(|| first_user_prompt.clone()),
            first_user_prompt,
            recent_user_prompts,
            last_assistant_text,
            started_at,
            last_user_msg_at,
            last_assistant_msg_at,
            user_msg_count,
            is_live: false,
            is_sidechain,
        })
    }
}

fn parse_timestamp_str(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn parse_entrypoint(value: &str) -> Entrypoint {
    match value {
        "cli" => Entrypoint::Cli,
        "sdk-cli" => Entrypoint::Sdk,
        "claude-desktop" => Entrypoint::Desktop,
        other => Entrypoint::Other(other.to_owned()),
    }
}

fn first_user_text(message: &RawValue) -> Option<String> {
    user_text_bodies(message).into_iter().next()
}

fn user_text_bodies(message: &RawValue) -> Vec<String> {
    let Ok(msg) = serde_json::from_str::<ClaudeMessage<'_>>(message.get()) else {
        return Vec::new();
    };
    let Some(content) = msg.content else {
        return Vec::new();
    };
    extract_text_bodies(content)
}

fn last_assistant_text(message: &RawValue) -> Option<String> {
    assistant_text_bodies(message).into_iter().last()
}

fn assistant_text_bodies(message: &RawValue) -> Vec<String> {
    let Ok(msg) = serde_json::from_str::<ClaudeMessage<'_>>(message.get()) else {
        return Vec::new();
    };
    let Some(content) = msg.content else {
        return Vec::new();
    };
    extract_text_bodies(content)
}

/// Parse a `content` field (either a string or an array of {type,text} parts).
fn extract_text_bodies(content: &RawValue) -> Vec<String> {
    let raw = content.get();
    let value: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    match value {
        Value::String(text) => nonempty_trimmed(&text).into_iter().collect(),
        Value::Array(parts) => parts
            .into_iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .and_then(nonempty_trimmed)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn nonempty_trimmed(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Harness-injected messages get recorded with `type: "user"` but aren't real
/// prompts (task-notifications, system-reminders, command stubs, etc.). Filter
/// them out when extracting prompts for display.
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
