use std::path::PathBuf;

use asm::{Agent, ClaudeParser, CodexParser, Entrypoint, Parser, Session};
use chrono::{DateTime, Utc};

struct ExpectedSession {
    id: &'static str,
    agent: Agent,
    path: &'static str,
    cwd: Option<&'static str>,
    git_branch: Option<&'static str>,
    entrypoint: Option<Entrypoint>,
    title: Option<&'static str>,
    first_user_prompt: Option<&'static str>,
    last_assistant_text: Option<&'static str>,
    started_at: &'static str,
    last_user_msg_at: Option<&'static str>,
    last_assistant_msg_at: Option<&'static str>,
    user_msg_count: u32,
    is_live: bool,
    is_sidechain: bool,
}

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn parse_utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn assert_session(session: Session, expected: ExpectedSession) {
    assert_eq!(session.id, expected.id);
    assert_eq!(session.agent, expected.agent);
    assert_eq!(session.path, fixture(expected.path));
    assert_eq!(session.cwd, expected.cwd.map(PathBuf::from));
    assert_eq!(session.git_branch, expected.git_branch.map(String::from));
    assert_eq!(session.entrypoint, expected.entrypoint);
    assert_eq!(session.title, expected.title.map(String::from));
    assert_eq!(
        session.first_user_prompt,
        expected.first_user_prompt.map(String::from)
    );
    assert_eq!(
        session.last_assistant_text,
        expected.last_assistant_text.map(String::from)
    );
    assert_eq!(session.started_at, parse_utc(expected.started_at));
    assert_eq!(
        session.last_user_msg_at,
        expected.last_user_msg_at.map(parse_utc)
    );
    assert_eq!(
        session.last_assistant_msg_at,
        expected.last_assistant_msg_at.map(parse_utc)
    );
    assert_eq!(session.user_msg_count, expected.user_msg_count);
    assert_eq!(session.is_live, expected.is_live);
    assert_eq!(session.is_sidechain, expected.is_sidechain);
}

#[test]
fn parses_claude_ai_title_session() {
    let path = fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let session = ClaudeParser::parse(&path).unwrap();

    assert_session(
        session,
        ExpectedSession {
            id: "b0fd6a29-a517-4325-a68a-fd7c1fbba04b",
            agent: Agent::Claude,
            path: "tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl",
            cwd: Some("/Users/reagan/.superset/worktrees/desktop-app/parallel-clef"),
            git_branch: Some("codex/session-schema-identity-guard"),
            entrypoint: Some(Entrypoint::Cli),
            title: Some("Check GitHub storage usage"),
            first_user_prompt: Some(
                "how much of the github storage are we using right now for this repo",
            ),
            last_assistant_text: Some(
                "**browser-use/desktop** is using **16,432 KB (~16 MB / ~0.016 GB)** of GitHub storage.\n\nFor reference, GitHub's soft limit is 1 GB per repo — you're at ~1.6% of that.",
            ),
            started_at: "2026-05-13T17:59:15.349Z",
            last_user_msg_at: Some("2026-05-13T17:59:23.740Z"),
            last_assistant_msg_at: Some("2026-05-13T17:59:31.594Z"),
            user_msg_count: 1,
            is_live: false,
            is_sidechain: false,
        },
    );
}

#[test]
fn parses_claude_first_prompt_title_fallback_session() {
    let path = fixture("tests/fixtures/claude/47370d97-696e-4a07-a96b-1fdf288749d2.jsonl");
    let session = ClaudeParser::parse(&path).unwrap();

    assert_session(
        session,
        ExpectedSession {
            id: "47370d97-696e-4a07-a96b-1fdf288749d2",
            agent: Agent::Claude,
            path: "tests/fixtures/claude/47370d97-696e-4a07-a96b-1fdf288749d2.jsonl",
            cwd: Some("/Users/reagan/.superset/worktrees/desktop-app/responsible-court"),
            git_branch: Some("feature/chat-view"),
            entrypoint: Some(Entrypoint::Sdk),
            title: Some("Reply with exactly the word OK and nothing else."),
            first_user_prompt: Some("Reply with exactly the word OK and nothing else."),
            last_assistant_text: Some("OK"),
            started_at: "2026-05-12T01:43:20.487Z",
            last_user_msg_at: Some("2026-05-12T01:43:20.493Z"),
            last_assistant_msg_at: Some("2026-05-12T01:43:25.037Z"),
            user_msg_count: 1,
            is_live: false,
            is_sidechain: false,
        },
    );
}

#[test]
fn parses_codex_tui_session() {
    let path = fixture(
        "tests/fixtures/codex/rollout-2026-04-14T18-08-53-019d8eaf-1998-76f2-a95e-8b65fadc3a1c.jsonl",
    );
    let session = CodexParser::parse(&path).unwrap();

    assert_session(
        session,
        ExpectedSession {
            id: "019d8eaf-1998-76f2-a95e-8b65fadc3a1c",
            agent: Agent::Codex,
            path: "tests/fixtures/codex/rollout-2026-04-14T18-08-53-019d8eaf-1998-76f2-a95e-8b65fadc3a1c.jsonl",
            cwd: Some("/Users/reagan/Documents/GitHub/request-for-startups"),
            git_branch: Some("main"),
            entrypoint: Some(Entrypoint::Tui),
            title: Some("gh issue view 52 then fix it"),
            first_user_prompt: Some("gh issue view 52 then fix it"),
            last_assistant_text: Some(
                "I’m pulling the issue details first, then I’ll inspect the codebase around the affected area and implement the fix directly in this workspace.",
            ),
            started_at: "2026-04-15T01:08:53.279Z",
            last_user_msg_at: Some("2026-04-15T01:09:15.080Z"),
            last_assistant_msg_at: Some("2026-04-15T01:09:30.495Z"),
            user_msg_count: 1,
            is_live: false,
            is_sidechain: false,
        },
    );
}

#[test]
fn parses_codex_exec_session() {
    let path = fixture(
        "tests/fixtures/codex/rollout-2026-04-22T14-54-46-019db730-4121-7f91-9ff5-c6cf321b7a0c.jsonl",
    );
    let session = CodexParser::parse(&path).unwrap();

    assert_session(
        session,
        ExpectedSession {
            id: "019db730-4121-7f91-9ff5-c6cf321b7a0c",
            agent: Agent::Codex,
            path: "tests/fixtures/codex/rollout-2026-04-22T14-54-46-019db730-4121-7f91-9ff5-c6cf321b7a0c.jsonl",
            cwd: Some("/Users/reagan/Documents/GitHub/reagan"),
            git_branch: Some("main"),
            entrypoint: Some(Entrypoint::Exec),
            title: Some("say hello in 5 words"),
            first_user_prompt: Some("say hello in 5 words"),
            last_assistant_text: Some("Hello there, wishing you well."),
            started_at: "2026-04-22T21:54:46.184Z",
            last_user_msg_at: Some("2026-04-22T21:54:52.028Z"),
            last_assistant_msg_at: Some("2026-04-22T21:54:55.561Z"),
            user_msg_count: 1,
            is_live: false,
            is_sidechain: false,
        },
    );
}
