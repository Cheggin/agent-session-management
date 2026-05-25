use std::{
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{Agent, Session, resume::dispatch_resume};

static SESSION_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"("sessionId"\s*:\s*)"[^"]*""#).expect("session id regex compiles")
});

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
    let forked = stream_fork(&source.agent, source_path, cut_index, current_cwd, roots)?;

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
    let (forked, line_count) =
        stream_fork_with_count(&source.agent, source_path, cut_index, current_cwd, roots)?;
    Ok((
        forked,
        ForkPhaseTimings {
            total_us: total.elapsed().as_micros(),
            line_count,
        },
    ))
}

fn stream_fork(
    agent: &Agent,
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    roots: &ForkRoots,
) -> Result<ForkResult> {
    stream_fork_with_count(agent, source_path, cut_index, current_cwd, roots).map(|(r, _)| r)
}

fn stream_fork_with_count(
    agent: &Agent,
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    roots: &ForkRoots,
) -> Result<(ForkResult, usize)> {
    match agent {
        Agent::Claude => write_claude_fork_streaming(source_path, cut_index, current_cwd, &roots.claude),
        Agent::Codex => write_codex_fork_streaming(source_path, cut_index, current_cwd, &roots.codex),
    }
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
    write_claude_fork_streaming(source_path, cut_index, current_cwd, claude_root).map(|(r, _)| r)
}

pub fn write_codex_fork(
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    codex_root: &Path,
) -> Result<ForkResult> {
    write_codex_fork_streaming(source_path, cut_index, current_cwd, codex_root).map(|(r, _)| r)
}

fn write_claude_fork_streaming(
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    claude_root: &Path,
) -> Result<(ForkResult, usize)> {
    let new_id = Uuid::new_v4().to_string();
    let dest_dir = claude_root
        .join("projects")
        .join(encode_claude_cwd(current_cwd));
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create Claude project dir {}", dest_dir.display()))?;
    let final_path = dest_dir.join(format!("{new_id}.jsonl"));
    let tmp_path = dest_dir.join(format!("{new_id}.jsonl.tmp"));

    let source = File::open(source_path)
        .with_context(|| format!("failed to open source session {}", source_path.display()))?;
    let mut reader = BufReader::with_capacity(256 * 1024, source);
    let dest = File::create(&tmp_path)
        .with_context(|| format!("failed to create Claude fork {}", tmp_path.display()))?;
    let mut writer = BufWriter::with_capacity(256 * 1024, dest);

    let replacement = format!("$1\"{new_id}\"");
    let mut first_user_turn: Option<usize> = None;
    let mut lines_written = 0usize;
    let mut line_buf = String::new();

    for line_index in 0..=cut_index {
        line_buf.clear();
        let n = reader
            .read_line(&mut line_buf)
            .with_context(|| format!("failed to read source session {}", source_path.display()))?;
        if n == 0 {
            break;
        }
        let trimmed = line_buf.trim_end_matches(['\n', '\r']);

        if first_user_turn.is_none()
            && let Ok(event) = serde_json::from_str::<Value>(trimmed)
            && claude_turn(line_index, &event).is_some()
        {
            first_user_turn = Some(line_index);
        }

        let rewritten = SESSION_ID_REGEX.replace_all(trimmed, replacement.as_str());
        writeln!(writer, "{rewritten}")
            .with_context(|| format!("failed to write Claude fork {}", tmp_path.display()))?;
        lines_written += 1;
    }

    finalize_fork(
        &Agent::Claude,
        lines_written,
        cut_index,
        first_user_turn,
        source_path,
        &tmp_path,
        &final_path,
        &mut writer,
    )?;
    Ok((
        ForkResult {
            path: final_path,
            id: new_id,
        },
        lines_written,
    ))
}

fn write_codex_fork_streaming(
    source_path: &Path,
    cut_index: usize,
    current_cwd: &Path,
    codex_root: &Path,
) -> Result<(ForkResult, usize)> {
    let new_id = Uuid::now_v7();
    let now_utc = Utc::now();
    let now_local = now_utc.with_timezone(&Local);
    let timestamp = now_utc.to_rfc3339_opts(SecondsFormat::Nanos, true);
    let final_path = codex_dest_path(codex_root, now_local, new_id);
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create Codex sessions dir {}", parent.display()))?;
    }
    let tmp_path = with_tmp_suffix(&final_path);

    // Resolve git eagerly: cheap when .git is direct (single fs read),
    // falls back to `git` subprocesses only when the on-disk layout is unusual.
    let git = current_git(current_cwd);

    let source = File::open(source_path)
        .with_context(|| format!("failed to open source session {}", source_path.display()))?;
    let mut reader = BufReader::with_capacity(256 * 1024, source);
    let dest = File::create(&tmp_path)
        .with_context(|| format!("failed to create Codex fork {}", tmp_path.display()))?;
    let mut writer = BufWriter::with_capacity(256 * 1024, dest);

    // Line 0: parse + mutate session_meta.
    let mut line_buf = String::new();
    let n = reader
        .read_line(&mut line_buf)
        .with_context(|| format!("failed to read source session {}", source_path.display()))?;
    if n == 0 {
        let _ = fs::remove_file(&tmp_path);
        bail!("source session {} is empty", source_path.display());
    }
    let mut meta = parse_json_line(line_buf.trim_end_matches(['\n', '\r']))?;
    if meta.get("type").and_then(Value::as_str) != Some("session_meta") {
        let _ = fs::remove_file(&tmp_path);
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
    match git {
        Some(git) => {
            payload.insert("git".to_owned(), git);
        }
        None => {
            payload.remove("git");
        }
    }
    writeln!(writer, "{}", serde_json::to_string(&meta)?)
        .with_context(|| format!("failed to write Codex session_meta {}", tmp_path.display()))?;

    let mut lines_written = 1usize;
    let mut first_user_turn: Option<usize> = None;

    for line_index in 1..=cut_index {
        line_buf.clear();
        let n = reader
            .read_line(&mut line_buf)
            .with_context(|| format!("failed to read source session {}", source_path.display()))?;
        if n == 0 {
            break;
        }

        if first_user_turn.is_none() {
            let trimmed = line_buf.trim_end_matches(['\n', '\r']);
            if let Ok(event) = serde_json::from_str::<Value>(trimmed)
                && codex_turn(line_index, &event).is_some()
            {
                first_user_turn = Some(line_index);
            }
        }

        // Codex copies lines 1..=cut_index byte-for-byte (no transform).
        // read_line preserves the trailing newline; pass it through unchanged.
        let bytes = line_buf.as_bytes();
        writer
            .write_all(bytes)
            .with_context(|| format!("failed to write Codex fork {}", tmp_path.display()))?;
        if !bytes.ends_with(b"\n") {
            writer
                .write_all(b"\n")
                .with_context(|| format!("failed to write Codex fork {}", tmp_path.display()))?;
        }
        lines_written += 1;
    }

    finalize_fork(
        &Agent::Codex,
        lines_written,
        cut_index,
        first_user_turn,
        source_path,
        &tmp_path,
        &final_path,
        &mut writer,
    )?;
    Ok((
        ForkResult {
            path: final_path,
            id: new_id.to_string(),
        },
        lines_written,
    ))
}

fn finalize_fork(
    _agent: &Agent,
    lines_written: usize,
    cut_index: usize,
    first_user_turn: Option<usize>,
    source_path: &Path,
    tmp_path: &Path,
    final_path: &Path,
    writer: &mut BufWriter<File>,
) -> Result<()> {
    let cleanup = |label: &str| -> anyhow::Error {
        let _ = fs::remove_file(tmp_path);
        anyhow::anyhow!("{label}")
    };
    if lines_written == 0 {
        return Err(cleanup(&format!(
            "source session {} is empty",
            source_path.display()
        )));
    }
    if lines_written <= cut_index {
        return Err(cleanup(&format!(
            "cut index {cut_index} is out of range for {} lines in {}",
            lines_written,
            source_path.display()
        )));
    }
    let first_turn = match first_user_turn {
        Some(t) => t,
        None => {
            return Err(cleanup(&format!(
                "source session {} has 0 user turns",
                source_path.display()
            )));
        }
    };
    if cut_index < first_turn {
        return Err(cleanup(&format!(
            "cut index {cut_index} is before the first user turn in {}",
            source_path.display()
        )));
    }
    writer
        .flush()
        .with_context(|| format!("failed to flush fork {}", tmp_path.display()))?;
    fs::rename(tmp_path, final_path)
        .with_context(|| format!("failed to finalize fork {}", final_path.display()))?;
    Ok(())
}

fn with_tmp_suffix(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
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
    read_git_head_direct(current_cwd).or_else(|| current_git_via_subprocess(current_cwd))
}

fn read_git_head_direct(cwd: &Path) -> Option<Value> {
    let git_dir = find_git_dir(cwd)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();

    let (commit, branch) = if let Some(ref_path) = head.strip_prefix("ref:") {
        let ref_path = ref_path.trim();
        let branch = Path::new(ref_path).file_name()?.to_str()?.to_owned();
        let commit = match fs::read_to_string(git_dir.join(ref_path)) {
            Ok(s) => s.trim().to_owned(),
            Err(_) => find_packed_ref(&git_dir, ref_path)?,
        };
        (commit, Some(branch))
    } else {
        (head.to_owned(), None)
    };

    if commit.is_empty() {
        return None;
    }
    let mut git = Map::new();
    git.insert("commit_hash".to_owned(), Value::String(commit));
    if let Some(branch) = branch
        && branch != "HEAD"
    {
        git.insert("branch".to_owned(), Value::String(branch));
    }
    Some(Value::Object(git))
}

fn find_git_dir(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        let candidate = current.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            let contents = fs::read_to_string(&candidate).ok()?;
            for line in contents.lines() {
                if let Some(rest) = line.strip_prefix("gitdir:") {
                    let rest = rest.trim();
                    let p = Path::new(rest);
                    return Some(if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        current.join(p)
                    });
                }
            }
            return None;
        }
        current = current.parent()?;
    }
}

fn find_packed_ref(git_dir: &Path, ref_path: &str) -> Option<String> {
    let packed = fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    for line in packed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let (hash, name) = line.split_once(' ')?;
        if name == ref_path {
            return Some(hash.to_owned());
        }
    }
    None
}

fn current_git_via_subprocess(cwd: &Path) -> Option<Value> {
    let commit = git_output(cwd, &["rev-parse", "HEAD"])?;
    let branch = git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
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
