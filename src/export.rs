use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};

use crate::{Agent, Session};

const EXPORT_NOTE: &str =
    "Note: this export shows the bookends only. Full transcript dump is planned for a later step.";

pub fn export_session(session: &Session, dest_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dest_dir)
        .with_context(|| format!("failed to create export directory {}", dest_dir.display()))?;
    let path = disambiguated_export_path(session, dest_dir);
    write_markdown(session, &path)?;
    Ok(path)
}

pub fn export_session_to_path(session: &Session, path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create export directory {}", parent.display()))?;
    }
    let path = disambiguate_path(path);
    write_markdown(session, &path)?;
    Ok(path)
}

fn write_markdown(session: &Session, path: &Path) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create export {}", path.display()))?;
    file.write_all(markdown(session).as_bytes())
        .with_context(|| format!("failed to write export {}", path.display()))?;
    Ok(())
}

fn markdown(session: &Session) -> String {
    format!(
        "# {title}\n\n\
- **Agent:** {agent}\n\
- **Session id:** {id}\n\
- **Cwd:** {cwd}\n\
- **Branch:** {branch}\n\
- **Started:** {started}\n\
- **Last activity:** {last_activity}\n\
- **Source file:** {source_file}\n\n\
---\n\n\
## Conversation\n\n\
### You\n\
{first_user_prompt}\n\n\
### Assistant (most recent)\n\
{last_assistant_text}\n\n\
{EXPORT_NOTE}\n",
        title = session_title(session),
        agent = agent_name(&session.agent),
        id = session.id,
        cwd = session
            .cwd
            .as_deref()
            .map(display_path)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "none".to_owned()),
        branch = session.git_branch.as_deref().unwrap_or("none"),
        started = format_ts(session.started_at),
        last_activity = format_ts(last_activity(session)),
        source_file = display_path(&session.path).to_string_lossy(),
        first_user_prompt = session.first_user_prompt.as_deref().unwrap_or(""),
        last_assistant_text = session.last_assistant_text.as_deref().unwrap_or(""),
    )
}

fn disambiguated_export_path(session: &Session, dest_dir: &Path) -> PathBuf {
    let base = format!(
        "asm-export-{}-{}",
        agent_name(&session.agent),
        id_short(&session.id)
    );
    disambiguate_path(&dest_dir.join(format!("{base}.md")))
}

fn disambiguate_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("asm-export");
    let extension = path.extension().and_then(|extension| extension.to_str());

    for suffix in 1_u32.. {
        let file_name = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem}-{suffix}.{extension}"),
            _ => format!("{stem}-{suffix}"),
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded suffix search should return before u32 overflow")
}

fn session_title(session: &Session) -> String {
    session
        .title
        .as_deref()
        .or(session.first_user_prompt.as_deref())
        .unwrap_or(&session.id)
        .to_owned()
}

fn last_activity(session: &Session) -> DateTime<Utc> {
    match (session.last_user_msg_at, session.last_assistant_msg_at) {
        (Some(user), Some(assistant)) => user.max(assistant),
        (Some(user), None) => user,
        (None, Some(assistant)) => assistant,
        (None, None) => session.started_at,
    }
}

fn display_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn format_ts(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn id_short(id: &str) -> String {
    let short: String = id.chars().take(8).collect();
    if short.is_empty() {
        "unknown".to_owned()
    } else {
        short
    }
}

fn agent_name(agent: &Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}
