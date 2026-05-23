use std::{collections::HashMap, path::Path};

use asm::{
    Agent, Entrypoint, Session,
    ui::filter::{
        Chip, apply_filters, apply_filters_with_haystacks, build_session_haystacks,
        parse_filter_text,
    },
};
use chrono::{DateTime, Duration, Utc};

fn cwd() -> &'static Path {
    Path::new("/tmp/current")
}

#[test]
fn empty_string_has_no_chips_or_search() {
    let parsed = parse_filter_text("", cwd());
    assert!(parsed.chips.is_empty());
    assert_eq!(parsed.search, "");
}

#[test]
fn claude_token_becomes_agent_chip() {
    let parsed = parse_filter_text("@claude", cwd());
    assert_eq!(parsed.chips, vec![Chip::Agent(Agent::Claude)]);
    assert_eq!(parsed.search, "");
}

#[test]
fn codex_branch_and_search_split() {
    let parsed = parse_filter_text("@codex @branch:main auth", cwd());
    assert_eq!(
        parsed.chips,
        vec![Chip::Agent(Agent::Codex), Chip::Branch("main".to_owned())]
    );
    assert_eq!(parsed.search, "auth");
}

#[test]
fn path_agent_and_multiword_search_split() {
    let parsed = parse_filter_text("@path:repo @claude foo bar", cwd());
    assert_eq!(
        parsed.chips,
        vec![
            Chip::PathSubstring("repo".to_owned()),
            Chip::Agent(Agent::Claude),
        ]
    );
    assert_eq!(parsed.search, "foo bar");
}

#[test]
fn partial_branch_token_stays_search_text() {
    let parsed = parse_filter_text("@bran", cwd());
    assert!(parsed.chips.is_empty());
    assert_eq!(parsed.search, "@bran");
}

#[test]
fn today_token_becomes_recency_chip() {
    let parsed = parse_filter_text("@today", cwd());
    assert_eq!(parsed.chips, vec![Chip::Recency(Duration::days(1))]);
    assert_eq!(parsed.search, "");
}

#[test]
fn apply_filters_keeps_visible_sessions_sorted_live_then_recent() {
    let sessions = filter_fixture_sessions();

    assert_eq!(
        ids(&sessions, &apply_filters(&sessions, &[], "")),
        ["live", "none", "repo", "main"]
    );
    assert_eq!(
        ids(
            &sessions,
            &apply_filters(&sessions, &[Chip::Agent(Agent::Claude)], "")
        ),
        ["live", "main"]
    );
    assert_eq!(
        ids(
            &sessions,
            &apply_filters(&sessions, &[Chip::Branch("main".to_owned())], "")
        ),
        ["live", "main"]
    );
    assert_eq!(
        ids(
            &sessions,
            &apply_filters(&sessions, &[Chip::PathSubstring("repo".to_owned())], "")
        ),
        ["repo"]
    );
}

#[test]
fn apply_filters_drops_sdk_entrypoint_by_default() {
    let mut sdk = session("sdk", Agent::Claude, 10);
    sdk.entrypoint = Some(Entrypoint::Sdk);
    let mut cli = session("cli", Agent::Claude, 20);
    cli.entrypoint = Some(Entrypoint::Cli);
    let sessions = vec![sdk, cli];

    assert_eq!(ids(&sessions, &apply_filters(&sessions, &[], "")), ["cli"]);
}

#[test]
fn apply_filters_drops_exec_entrypoint_by_default() {
    let mut exec = session("exec", Agent::Codex, 10);
    exec.entrypoint = Some(Entrypoint::Exec);
    let mut tui = session("tui", Agent::Codex, 20);
    tui.entrypoint = Some(Entrypoint::Tui);
    let sessions = vec![exec, tui];

    assert_eq!(ids(&sessions, &apply_filters(&sessions, &[], "")), ["tui"]);
}

#[test]
fn search_matches_concatenated_message_body_haystack() {
    let mut sessions = filter_fixture_sessions();
    let repo = sessions
        .iter_mut()
        .find(|session| session.id == "repo")
        .unwrap();
    repo.title = Some("plain title".to_owned());
    repo.first_user_prompt = Some("plain prompt".to_owned());
    repo.cwd = Some(Path::new("/Users/foo/plain-project").to_path_buf());
    repo.git_branch = Some("dev".to_owned());

    let bodies = HashMap::from([(
        "repo".to_owned(),
        "the only hidden word here is storage".to_owned(),
    )]);
    let haystacks = build_session_haystacks(&sessions, &bodies);

    assert_eq!(
        ids(
            &sessions,
            &apply_filters_with_haystacks(&sessions, &[], "storage", &haystacks)
        ),
        ["repo"]
    );
}

fn filter_fixture_sessions() -> Vec<Session> {
    let mut main = session("main", Agent::Claude, 10);
    main.cwd = Some(Path::new("/Users/foo/main").to_path_buf());
    main.git_branch = Some("MAIN".to_owned());

    let mut repo = session("repo", Agent::Codex, 20);
    repo.cwd = Some(Path::new("/Users/foo/repo-bar").to_path_buf());
    repo.git_branch = Some("dev".to_owned());

    let mut live = session("live", Agent::Claude, 5);
    live.cwd = Some(Path::new("/Users/foo/live").to_path_buf());
    live.is_live = true;

    let mut none = session("none", Agent::Codex, 30);
    none.cwd = Some(Path::new("/Users/foo/other").to_path_buf());
    none.git_branch = None;

    let mut side = session("side", Agent::Claude, 40);
    side.cwd = Some(Path::new("/Users/foo/repo-side").to_path_buf());
    side.is_sidechain = true;

    let mut empty = session("empty", Agent::Codex, 50);
    empty.cwd = Some(Path::new("/Users/foo/repo-empty").to_path_buf());
    empty.user_msg_count = 0;

    vec![main, repo, live, none, side, empty]
}

fn session(id: &str, agent: Agent, minute: u32) -> Session {
    let last_user = parse_utc(&format!("2026-05-22T12:{minute:02}:00Z"));
    Session {
        id: id.to_owned(),
        agent,
        path: Path::new("/tmp").join(format!("{id}.jsonl")),
        cwd: Some(Path::new("/Users/foo").join(id)),
        git_branch: Some("main".to_owned()),
        entrypoint: None,
        title: Some(id.to_owned()),
        first_user_prompt: Some(format!("prompt {id}")),
        recent_user_prompts: vec![format!("prompt {id}")],
        last_assistant_text: None,
        started_at: parse_utc("2026-05-22T12:00:00Z"),
        last_user_msg_at: Some(last_user),
        last_assistant_msg_at: None,
        user_msg_count: 1,
        is_live: false,
        is_sidechain: false,
    }
}

fn parse_utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn ids(sessions: &[Session], indices: &[usize]) -> Vec<String> {
    indices
        .iter()
        .map(|idx| sessions[*idx].id.clone())
        .collect()
}
