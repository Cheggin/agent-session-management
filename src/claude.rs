use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{Agent, Entrypoint, Parser, Session};

pub struct ClaudeParser;

impl ClaudeParser {
    pub fn extract_message_bodies(path: &Path) -> Result<Vec<String>> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Claude session {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut bodies = Vec::new();

        for line in reader.lines() {
            let line = line.with_context(|| {
                format!("failed to read Claude session line from {}", path.display())
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };

            match event.get("type").and_then(Value::as_str) {
                Some("user") if !is_meta(&event) => bodies.extend(
                    user_text_bodies(&event)
                        .into_iter()
                        .filter(|t| !is_system_noise(t)),
                ),
                Some("assistant") => bodies.extend(assistant_text_bodies(&event)),
                _ => {}
            }
        }

        Ok(bodies)
    }
}

impl Parser for ClaudeParser {
    fn parse(path: &Path) -> Result<Session> {
        let file = File::open(path)
            .with_context(|| format!("failed to open Claude session {}", path.display()))?;
        let reader = BufReader::new(file);

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
        let mut last_assistant_text: Option<String> = None;
        let mut started_at: Option<DateTime<Utc>> = None;
        let mut last_user_msg_at: Option<DateTime<Utc>> = None;
        let mut last_assistant_msg_at: Option<DateTime<Utc>> = None;
        let mut user_msg_count = 0_u32;
        let mut is_sidechain = false;

        for line in reader.lines() {
            let line = line.with_context(|| {
                format!("failed to read Claude session line from {}", path.display())
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };

            let timestamp = parse_timestamp(event.get("timestamp"));
            if started_at.is_none() {
                started_at = timestamp;
            }

            if cwd.is_none() {
                cwd = event.get("cwd").and_then(Value::as_str).map(PathBuf::from);
            }

            if !saw_git_branch && let Some(branch) = event.get("gitBranch").and_then(Value::as_str)
            {
                saw_git_branch = true;
                if branch != "HEAD" {
                    git_branch = Some(branch.to_owned());
                }
            }

            if entrypoint.is_none() {
                entrypoint = event
                    .get("entrypoint")
                    .and_then(Value::as_str)
                    .map(parse_entrypoint);
            }

            if event
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                is_sidechain = true;
            }

            match event.get("type").and_then(Value::as_str) {
                Some("ai-title") => {
                    if let Some(ai_title) = event.get("aiTitle").and_then(Value::as_str) {
                        title = Some(ai_title.to_owned());
                    }
                }
                Some("user") => {
                    if is_meta(&event) {
                        continue;
                    }

                    let Some(prompt) = user_prompt_text(&event) else {
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
                    if let Some(text) = assistant_text(&event) {
                        last_assistant_text = Some(text);
                    }
                }
                _ => {}
            }
        }

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

fn parse_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(Value::as_str)
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

fn is_meta(event: &Value) -> bool {
    event
        .get("isMeta")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn user_prompt_text(event: &Value) -> Option<String> {
    user_text_bodies(event).into_iter().next()
}

fn user_text_bodies(event: &Value) -> Vec<String> {
    let Some(content) = event
        .get("message")
        .and_then(|message| message.get("content"))
    else {
        return Vec::new();
    };

    match content {
        Value::String(text) => nonempty_trimmed(text).into_iter().collect(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter_map(nonempty_trimmed)
            .collect(),
        _ => Vec::new(),
    }
}

fn assistant_text(event: &Value) -> Option<String> {
    assistant_text_bodies(event).into_iter().last()
}

fn assistant_text_bodies(event: &Value) -> Vec<String> {
    let Some(parts) = event
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    parts
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .filter_map(nonempty_trimmed)
        .collect()
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
