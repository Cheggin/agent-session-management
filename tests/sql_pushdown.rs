use std::path::PathBuf;

use asm::{Agent, Entrypoint, Session, db::Index, ui::filter::Chip};
use chrono::{DateTime, Utc};

fn open_seeded_index() -> (tempfile::TempDir, Index) {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    for (mtime, session) in seeded_sessions().into_iter().enumerate() {
        index
            .upsert_session(&session, i64::try_from(mtime).unwrap(), &[])
            .unwrap();
    }
    (temp, index)
}

fn seeded_sessions() -> Vec<Session> {
    let mut sidechain_noise = session(
        "sidechain_noise",
        Agent::Claude,
        "/tmp/foo/sidechain",
        "main",
        Entrypoint::Cli,
    );
    sidechain_noise.is_sidechain = true;

    let mut zero_user_noise = session(
        "zero_user_noise",
        Agent::Claude,
        "/tmp/foo/zero",
        "main",
        Entrypoint::Tui,
    );
    zero_user_noise.user_msg_count = 0;

    vec![
        session(
            "foo_claude",
            Agent::Claude,
            "/tmp/foo",
            "Main",
            Entrypoint::Cli,
        ),
        session(
            "foo_child_codex",
            Agent::Codex,
            "/tmp/foo/child",
            "feature",
            Entrypoint::Tui,
        ),
        session(
            "users_repo_claude",
            Agent::Claude,
            "/Users/x/repo",
            "dev",
            Entrypoint::Vscode,
        ),
        session(
            "tmp_repo_bar_codex",
            Agent::Codex,
            "/tmp/repo-bar",
            "MAIN",
            Entrypoint::Desktop,
        ),
        sidechain_noise,
        zero_user_noise,
        session(
            "sdk_noise",
            Agent::Claude,
            "/tmp/foo/sdk",
            "main",
            Entrypoint::Sdk,
        ),
        session(
            "exec_noise",
            Agent::Codex,
            "/tmp/foo/exec",
            "main",
            Entrypoint::Exec,
        ),
        session(
            "other_codex",
            Agent::Codex,
            "/tmp/other",
            "other",
            Entrypoint::Cli,
        ),
    ]
}

fn session(id: &str, agent: Agent, cwd: &str, branch: &str, entrypoint: Entrypoint) -> Session {
    let timestamp = DateTime::parse_from_rfc3339("2026-05-23T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    Session {
        id: id.to_owned(),
        agent,
        path: PathBuf::from(format!("/sessions/{id}.jsonl")),
        cwd: Some(PathBuf::from(cwd)),
        git_branch: Some(branch.to_owned()),
        entrypoint: Some(entrypoint),
        title: Some(id.to_owned()),
        first_user_prompt: Some(format!("prompt {id}")),
        recent_user_prompts: vec![format!("prompt {id}")],
        last_assistant_text: None,
        started_at: timestamp,
        last_user_msg_at: Some(timestamp),
        last_assistant_msg_at: None,
        user_msg_count: 1,
        is_live: false,
        is_sidechain: false,
    }
}

fn ids(sessions: Vec<Session>) -> Vec<String> {
    let mut ids = sessions
        .into_iter()
        .map(|session| session.id)
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[test]
fn here_cwd_pushdown_returns_exact_cwd_and_descendants() {
    let (_temp, index) = open_seeded_index();

    assert_eq!(
        ids(index
            .list_filtered(&[Chip::HereCwd(PathBuf::from("/tmp/foo"))])
            .unwrap()),
        vec!["foo_child_codex", "foo_claude"]
    );
}

#[test]
fn agent_pushdown_returns_only_requested_agent() {
    let (_temp, index) = open_seeded_index();

    assert_eq!(
        ids(index.list_filtered(&[Chip::Agent(Agent::Claude)]).unwrap()),
        vec!["foo_claude", "users_repo_claude"]
    );
}

#[test]
fn branch_pushdown_is_case_insensitive() {
    let (_temp, index) = open_seeded_index();

    assert_eq!(
        ids(index
            .list_filtered(&[Chip::Branch("main".to_owned())])
            .unwrap()),
        vec!["foo_claude", "tmp_repo_bar_codex"]
    );
}

#[test]
fn path_substring_pushdown_is_case_insensitive() {
    let (_temp, index) = open_seeded_index();

    assert_eq!(
        ids(index
            .list_filtered(&[Chip::PathSubstring("repo".to_owned())])
            .unwrap()),
        vec!["tmp_repo_bar_codex", "users_repo_claude"]
    );
}

#[test]
fn filtered_queries_apply_default_exclusions() {
    let (_temp, index) = open_seeded_index();

    let result_ids = ids(index.list_filtered(&[]).unwrap());

    assert!(!result_ids.iter().any(|id| id == "sidechain_noise"));
    assert!(!result_ids.iter().any(|id| id == "zero_user_noise"));
    assert!(!result_ids.iter().any(|id| id == "sdk_noise"));
    assert!(!result_ids.iter().any(|id| id == "exec_noise"));
}
