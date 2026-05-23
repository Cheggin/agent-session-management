use std::{fs, path::PathBuf};

use asm::{
    ClaudeParser, CodexParser, Parser,
    fork::{ForkRoots, encode_claude_cwd, end_cut_index, fork_session_with_roots},
};
use chrono::{DateTime, Utc};
use serde_json::Value;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn claude_fork_at_end_writes_to_current_cwd_project_dir() {
    let temp = tempfile::tempdir().unwrap();
    let roots = temp_roots(temp.path());
    let current_cwd = temp.path().join(".hidden/repo");
    fs::create_dir_all(&current_cwd).unwrap();
    let source_path = fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let source = ClaudeParser::parse(&source_path).unwrap();
    let cut_index = end_cut_index(&source_path).unwrap();

    let fork_path =
        fork_session_with_roots(&source, &source_path, cut_index, &current_cwd, &roots, true)
            .unwrap();

    assert!(fork_path.exists());
    assert_eq!(
        fork_path.parent().unwrap(),
        roots
            .claude
            .join("projects")
            .join(encode_claude_cwd(&current_cwd))
    );
    let source_lines = read_lines(&source_path);
    let fork_lines = read_lines(&fork_path);
    assert_eq!(fork_lines.len(), source_lines.len());
    assert!(!fork_lines.is_empty());

    let new_id = fork_path.file_stem().unwrap().to_string_lossy();
    for line in fork_lines {
        let value: Value = serde_json::from_str(&line).unwrap();
        if let Some(session_id) = value.get("sessionId").and_then(Value::as_str) {
            assert_eq!(session_id, new_id);
            assert_ne!(session_id, source.id);
        }
    }
}

#[test]
fn codex_fork_at_end_rewrites_session_meta() {
    let temp = tempfile::tempdir().unwrap();
    let roots = temp_roots(temp.path());
    let current_cwd = temp.path().join("repo");
    fs::create_dir_all(&current_cwd).unwrap();
    let source_path = fixture(
        "tests/fixtures/codex/rollout-2026-04-22T14-54-46-019db730-4121-7f91-9ff5-c6cf321b7a0c.jsonl",
    );
    let source = CodexParser::parse(&source_path).unwrap();
    let cut_index = end_cut_index(&source_path).unwrap();
    let before = Utc::now();

    let fork_path =
        fork_session_with_roots(&source, &source_path, cut_index, &current_cwd, &roots, true)
            .unwrap();

    let first_line = read_lines(&fork_path).remove(0);
    let meta: Value = serde_json::from_str(&first_line).unwrap();
    let payload = meta.get("payload").unwrap();
    let new_id = payload.get("id").and_then(Value::as_str).unwrap();
    let timestamp = parse_utc(payload.get("timestamp").and_then(Value::as_str).unwrap());

    assert_ne!(new_id, source.id);
    assert_eq!(
        payload.get("cwd").and_then(Value::as_str),
        Some(current_cwd.to_string_lossy().as_ref())
    );
    assert_eq!(
        payload.get("originator").and_then(Value::as_str),
        Some("codex_exec")
    );
    assert!(timestamp >= before);
    assert!(Utc::now().signed_duration_since(timestamp).num_seconds() < 60);
    assert_eq!(
        meta.get("timestamp").and_then(Value::as_str),
        payload.get("timestamp").and_then(Value::as_str)
    );
}

fn temp_roots(path: &std::path::Path) -> ForkRoots {
    ForkRoots {
        claude: path.join(".claude"),
        codex: path.join(".codex"),
    }
}

fn read_lines(path: &std::path::Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn parse_utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}
