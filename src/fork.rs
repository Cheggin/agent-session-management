use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{Agent, Session, resume::dispatch_resume};

#[derive(Debug, Clone)]
pub struct ForkRoots {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCut {
    pub cut_index: usize,
    pub timestamp: Option<DateTime<Utc>>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkResult {
    pub path: PathBuf,
    pub id: String,
}

#[derive(Debug, Clone, Default)]
pub struct ForkPhaseTimings {
    pub read_jsonl_us: u128,
    pub validate_us: u128,
    pub transform_write_us: u128,
    pub total_us: u128,
    pub line_count: usize,
}

impl ForkRoots {
    pub fn from_home(home: impl AsRef<Path>) -> Self {
        let home = home.as_ref();
        Self {
            claude: home.join(".claude"),
            codex: home.join(".codex"),
        }
    }
}

pub fn fork_session(
    source: &Session,
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    no_resume: bool,
) -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    fork_session_with_roots(
        source,
        source_path,
        cut_index,
        current_cwd,
        &ForkRoots::from_home(home),
        no_resume,
    )
}

pub fn fork_session_with_roots(
    source: &Session,
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    roots: &ForkRoots,
    no_resume: bool,
) -> Result<PathBuf> {
    let lines = read_jsonl_lines(source_path)?;
    validate_cut_index(&source.agent, &lines, cut_index, source_path)?;

    let forked = match source.agent {
        Agent::Claude => {
            write_claude_fork_from_lines(&lines, cut_index, current_cwd, &roots.claude)?
        }
        Agent::Codex => write_codex_fork_from_lines(&lines, cut_index, current_cwd, &roots.codex)?,
    };

    if !no_resume {
        let mut forked_session = source.clone();
        forked_session.id = forked.id.clone();
        forked_session.path = forked.path.clone();
        forked_session.cwd = Some(current_cwd.to_path_buf());
        dispatch_resume(&forked_session)?;
    }

    Ok(forked.path)
}

pub fn fork_session_timed(
    source: &Session,
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    roots: &ForkRoots,
) -> Result<(ForkResult, ForkPhaseTimings)> {
    let total = Instant::now();
    let mut timings = ForkPhaseTimings::default();

    let t = Instant::now();
    let lines = read_jsonl_lines(source_path)?;
    timings.read_jsonl_us = t.elapsed().as_micros();
    timings.line_count = lines.len();

    let t = Instant::now();
    validate_cut_index(&source.agent, &lines, cut_index, source_path)?;
    timings.validate_us = t.elapsed().as_micros();

    let t = Instant::now();
    let forked = match source.agent {
        Agent::Claude => {
            write_claude_fork_from_lines(&lines, cut_index, current_cwd, &roots.claude)?
        }
        Agent::Codex => write_codex_fork_from_lines(&lines, cut_index, current_cwd, &roots.codex)?,
    };
    timings.transform_write_us = t.elapsed().as_micros();

    timings.total_us = total.elapsed().as_micros();
    Ok((forked, timings))
}

pub fn end_cut_index(source_path: &Path) -> Result<usize> {
    let lines = read_jsonl_lines(source_path)?;
    lines
        .len()
        .checked_sub(1)
        .with_context(|| format!("source session {} is empty", source_path.display()))
}

pub fn read_turns(agent: &Agent, source_path: &Path) -> Result<Vec<TurnCut>> {
    let lines = read_jsonl_lines(source_path)?;
    Ok(turns_from_lines(agent, &lines))
}

pub fn encode_claude_cwd(current_cwd: &Path) -> String {
    current_cwd
        .to_string_lossy()
        .replace('/', "-")
        .replace("-.", "--")
}

pub fn codex_dest_path(codex_root: &Path, now: DateTime<Local>, new_id: Uuid) -> PathBuf {
    codex_root
        .join("sessions")
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%d").to_string())
        .join(format!(
            "rollout-{}-{new_id}.jsonl",
            now.format("%Y-%m-%dT%H-%M-%S")
        ))
}

pub fn write_claude_fork(
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    claude_root: &Path,
) -> Result<ForkResult> {
    let lines = read_jsonl_lines(source_path)?;
    validate_cut_index(&Agent::Claude, &lines, cut_index, source_path)?;
    write_claude_fork_from_lines(&lines, cut_index, current_cwd, claude_root)
}

pub fn write_codex_fork(
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    codex_root: &Path,
) -> Result<ForkResult> {
    let lines = read_jsonl_lines(source_path)?;
    validate_cut_index(&Agent::Codex, &lines, cut_index, source_path)?;
    write_codex_fork_from_lines(&lines, cut_index, current_cwd, codex_root)
}

fn write_claude_fork_from_lines(
    lines: &[String],
    cut_index: usize,
    current_cwd: &Path,
    claude_root: &Path,
) -> Result<ForkResult> {
    let new_id = Uuid::new_v4().to_string();
    let dest_dir = claude_root
        .join("projects")
        .join(encode_claude_cwd(current_cwd));
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create Claude project dir {}", dest_dir.display()))?;
    let path = dest_dir.join(format!("{new_id}.jsonl"));
    let mut file = File::create(&path)
        .with_context(|| format!("failed to create Claude fork {}", path.display()))?;
    let session_id_regex = Regex::new(r#"("sessionId"\s*:\s*)"[^"]*""#)?;

    for line in &lines[..=cut_index] {
        let rewritten = session_id_regex.replace_all(line, format!("$1\"{new_id}\""));
        writeln!(file, "{rewritten}")
            .with_context(|| format!("failed to write Claude fork {}", path.display()))?;
    }

    Ok(ForkResult { path, id: new_id })
}

fn write_codex_fork_from_lines(
    lines: &[String],
    cut_index: usize,
    current_cwd: &Path,
    codex_root: &Path,
) -> Result<ForkResult> {
    let new_id = Uuid::now_v7();
    let now_utc = Utc::now();
    let now_local = now_utc.with_timezone(&Local);
    let timestamp = now_utc.to_rfc3339_opts(SecondsFormat::Nanos, true);
    let path = codex_dest_path(codex_root, now_local, new_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create Codex sessions dir {}", parent.display()))?;
    }

    let mut meta = parse_json_line(
        lines
            .first()
            .context("Codex source session has no session_meta line")?,
    )?;
    if meta.get("type").and_then(Value::as_str) != Some("session_meta") {
        bail!("Codex source first line is not a session_meta event");
    }
    meta["timestamp"] = Value::String(timestamp.clone());
    let payload = meta
        .get_mut("payload")
        .and_then(Value::as_object_mut)
        .context("Codex session_meta has no object payload")?;
    payload.insert("id".to_owned(), Value::String(new_id.to_string()));
    payload.insert("timestamp".to_owned(), Value::String(timestamp));
    payload.insert(
        "cwd".to_owned(),
        Value::String(current_cwd.to_string_lossy().into_owned()),
    );
    match current_git(current_cwd) {
        Some(git) => {
            payload.insert("git".to_owned(), git);
        }
        None => {
            payload.remove("git");
        }
    }

    let mut file = File::create(&path)
        .with_context(|| format!("failed to create Codex fork {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&meta)?)
        .with_context(|| format!("failed to write Codex session_meta {}", path.display()))?;
    for line in &lines[1..=cut_index] {
        writeln!(file, "{line}")
            .with_context(|| format!("failed to write Codex fork {}", path.display()))?;
    }

    Ok(ForkResult {
        path,
        id: new_id.to_string(),
    })
}

fn validate_cut_index(
    agent: &Agent,
    lines: &[String],
    cut_index: usize,
    source_path: &Path,
) -> Result<()> {
    if lines.is_empty() {
        bail!("source session {} is empty", source_path.display());
    }
    if cut_index >= lines.len() {
        bail!(
            "cut index {cut_index} is out of range for {} lines in {}",
            lines.len(),
            source_path.display()
        );
    }
    let turns = turns_from_lines(agent, lines);
    if turns.is_empty() {
        bail!("source session {} has 0 user turns", source_path.display());
    }
    if cut_index < turns[0].cut_index {
        bail!(
            "cut index {cut_index} is before the first user turn in {}",
            source_path.display()
        );
    }
    Ok(())
}

fn turns_from_lines(agent: &Agent, lines: &[String]) -> Vec<TurnCut> {
    let mut turns = Vec::new();
    for (cut_index, line) in lines.iter().enumerate() {
        let Ok(event) = parse_json_line(line) else {
            continue;
        };
        let turn = match agent {
            Agent::Claude => claude_turn(cut_index, &event),
            Agent::Codex => codex_turn(cut_index, &event),
        };
        if let Some(turn) = turn {
            turns.push(turn);
        }
    }
    turns
}

fn claude_turn(cut_index: usize, event: &Value) -> Option<TurnCut> {
    if event.get("type").and_then(Value::as_str) != Some("user")
        || event
            .get("isMeta")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return None;
    }
    Some(TurnCut {
        cut_index,
        timestamp: parse_timestamp(event.get("timestamp")),
        message: claude_user_text(event)?,
    })
}

fn codex_turn(cut_index: usize, event: &Value) -> Option<TurnCut> {
    if event.get("type").and_then(Value::as_str) != Some("event_msg") {
        return None;
    }
    let payload = event.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("user_message") {
        return None;
    }
    Some(TurnCut {
        cut_index,
        timestamp: parse_timestamp(event.get("timestamp")),
        message: payload.get("message")?.as_str()?.trim().to_owned(),
    })
}

fn claude_user_text(event: &Value) -> Option<String> {
    let content = event.get("message")?.get("content")?;
    match content {
        Value::String(text) => Some(text.trim().to_owned()),
        Value::Array(parts) => parts
            .iter()
            .find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|part| part.get("text").and_then(Value::as_str))
            .map(|text| text.trim().to_owned()),
        _ => None,
    }
}

fn current_git(current_cwd: &Path) -> Option<Value> {
    let commit = git_output(current_cwd, &["rev-parse", "HEAD"])?;
    let branch = git_output(current_cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let mut git = Map::new();
    git.insert("commit_hash".to_owned(), Value::String(commit));
    if branch != "HEAD" {
        git.insert("branch".to_owned(), Value::String(branch));
    }
    Some(Value::Object(git))
}

fn git_output(current_cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(current_cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn read_jsonl_lines(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path)
        .with_context(|| format!("failed to open source session {}", path.display()))?;
    BufReader::new(file)
        .lines()
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read source session {}", path.display()))
}

fn parse_json_line(line: &str) -> Result<Value> {
    serde_json::from_str(line).context("failed to parse JSONL line")
}

fn parse_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
}
