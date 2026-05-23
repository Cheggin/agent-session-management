use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{Agent, Entrypoint, Parser, Session};

pub struct CodexParser;

impl CodexParser {
    pub fn extract_message_bodies(path: &Path) -> Result<Vec<String>> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Codex session {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut bodies = Vec::new();

        for line in reader.lines() {
            let line = line.with_context(|| {
                format!("failed to read Codex session line from {}", path.display())
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };

            if event.get("type").and_then(Value::as_str) != Some("event_msg") {
                continue;
            }

            let Some(payload) = event.get("payload") else {
                continue;
            };
            match payload.get("type").and_then(Value::as_str) {
                Some("user_message") | Some("agent_message") => {
                    if let Some(message) = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .and_then(nonempty_trimmed)
                    {
                        bodies.push(message);
                    }
                }
                Some("task_complete") => {
                    if let Some(message) = payload
                        .get("last_agent_message")
                        .and_then(Value::as_str)
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
}

impl Parser for CodexParser {
    fn parse(path: &Path) -> Result<Session> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Codex session {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let session_meta = read_session_meta(&mut lines, path)?;
        let payload = session_meta
            .get("payload")
            .context("Codex session_meta event has no payload")?;

        let id = payload
            .get("id")
            .and_then(Value::as_str)
            .context("Codex session_meta payload has no id")?
            .to_owned();
        let started_at = parse_timestamp(payload.get("timestamp")).with_context(|| {
            format!(
                "Codex session {} has no parseable payload timestamp",
                path.display()
            )
        })?;
        let cwd = payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let git_branch = payload
            .get("git")
            .and_then(|git| git.get("branch"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let entrypoint = payload
            .get("originator")
            .and_then(Value::as_str)
            .map(parse_entrypoint);
        let is_sidechain = payload
            .get("thread_source")
            .and_then(Value::as_str)
            .is_some_and(|thread_source| thread_source == "subagent");

        let mut first_user_prompt: Option<String> = None;
        let mut recent_user_prompts = Vec::new();
        let mut last_agent_message: Option<String> = None;
        let mut last_task_complete_message: Option<String> = None;
        let mut last_user_msg_at: Option<DateTime<Utc>> = None;
        let mut last_assistant_msg_at: Option<DateTime<Utc>> = None;
        let mut user_msg_count = 0_u32;

        for line in lines {
            let line = line.with_context(|| {
                format!("failed to read Codex session line from {}", path.display())
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };

            if event.get("type").and_then(Value::as_str) != Some("event_msg") {
                continue;
            }

            let timestamp = parse_timestamp(event.get("timestamp"));
            let Some(payload) = event.get("payload") else {
                continue;
            };

            match payload.get("type").and_then(Value::as_str) {
                Some("user_message") => {
                    user_msg_count += 1;
                    if first_user_prompt.is_none()
                        && let Some(message) = payload.get("message").and_then(Value::as_str)
                    {
                        first_user_prompt = Some(message.trim().to_owned());
                    }
                    if let Some(message) = payload
                        .get("message")
                        .and_then(Value::as_str)
                        .and_then(nonempty_trimmed)
                    {
                        remember_recent_user_prompt(&mut recent_user_prompts, message);
                    }
                    if let Some(timestamp) = timestamp {
                        last_user_msg_at = Some(timestamp);
                    }
                }
                Some("agent_message") => {
                    if let Some(message) = payload.get("message").and_then(Value::as_str) {
                        last_agent_message = Some(message.to_owned());
                    }
                    if let Some(timestamp) = timestamp {
                        update_latest(&mut last_assistant_msg_at, timestamp);
                    }
                }
                Some("task_complete") => {
                    if let Some(message) = payload.get("last_agent_message").and_then(Value::as_str)
                    {
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
            is_sidechain,
        })
    }
}

fn read_session_meta(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    path: &Path,
) -> Result<Value> {
    for line in lines {
        let line = line.with_context(|| {
            format!("failed to read Codex session line from {}", path.display())
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        if event.get("type").and_then(Value::as_str) == Some("session_meta") {
            return Ok(event);
        }

        bail!(
            "Codex session {} first parseable event is not session_meta",
            path.display()
        );
    }

    bail!("Codex session {} has no session_meta event", path.display());
}

fn parse_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(Value::as_str)
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

fn remember_recent_user_prompt(recent_user_prompts: &mut Vec<String>, prompt: String) {
    recent_user_prompts.push(prompt);
    if recent_user_prompts.len() > 3 {
        recent_user_prompts.remove(0);
    }
}
