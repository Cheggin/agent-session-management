use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

mod claude;
pub mod cli;
mod codex;
pub mod db;
pub mod discover;
pub mod export;
pub mod fork;
pub mod liveness;
pub mod reindex;
pub mod resume;
pub mod shell;
pub mod ui;

pub use claude::ClaudeParser;
pub use codex::CodexParser;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agent {
    Claude,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entrypoint {
    Cli,
    Tui,
    Sdk,
    Vscode,
    Exec,
    Desktop,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    // identity
    pub id: String,
    pub agent: Agent,
    pub path: PathBuf,

    // environment
    pub cwd: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub entrypoint: Option<Entrypoint>,

    // display
    pub title: Option<String>,
    pub first_user_prompt: Option<String>,
    pub last_assistant_text: Option<String>,

    // timing
    pub started_at: DateTime<Utc>,
    pub last_user_msg_at: Option<DateTime<Utc>>,
    pub last_assistant_msg_at: Option<DateTime<Utc>>,

    // noise filter
    pub user_msg_count: u32,

    // status
    pub is_live: bool,
    pub is_sidechain: bool,
}

pub trait Parser {
    fn parse(path: &Path) -> Result<Session>;
}
