use std::path::PathBuf;

use asm::{ClaudeParser, CodexParser, Parser, db::Index};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn sqlite_index_round_trips_all_session_fields() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let session = ClaudeParser::parse(&fixture(
        "tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl",
    ))
    .unwrap();

    index.upsert_session(&session, 123_456_789, &[]).unwrap();

    assert_eq!(
        index.get_path_mtime(&session.path).unwrap(),
        Some(123_456_789)
    );
    let sessions = index.list_all().unwrap();
    assert_eq!(sessions[0].recent_user_prompts, session.recent_user_prompts);
    assert_eq!(sessions, vec![session]);
}

#[test]
fn sqlite_index_round_trips_codex_session_fields() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let session = CodexParser::parse(&fixture(
        "tests/fixtures/codex/rollout-2026-04-14T18-08-53-019d8eaf-1998-76f2-a95e-8b65fadc3a1c.jsonl",
    ))
    .unwrap();

    index.upsert_session(&session, 987_654_321, &[]).unwrap();

    assert_eq!(
        index.get_path_mtime(&session.path).unwrap(),
        Some(987_654_321)
    );
    let sessions = index.list_all().unwrap();
    assert_eq!(sessions, vec![session]);
}

#[test]
fn sqlite_index_upserting_same_path_updates_mtime_without_duplicate_rows() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let session = ClaudeParser::parse(&fixture(
        "tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl",
    ))
    .unwrap();

    index.upsert_session(&session, 1, &[]).unwrap();
    index.upsert_session(&session, 2, &[]).unwrap();

    let sessions = index.list_all().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0], session);
    assert_eq!(index.get_path_mtime(&session.path).unwrap(), Some(2));
}

#[test]
fn sqlite_index_tracks_last_reindex_age() {
    let temp = tempfile::tempdir().unwrap();
    let index = Index::open_in_dir(temp.path()).unwrap();

    assert!(index.last_reindex_age().unwrap().is_none());

    index.mark_reindexed_now().unwrap();

    assert!(index.last_reindex_age().unwrap().unwrap().as_secs() < 5);
}

#[test]
fn sqlite_index_stores_message_bodies_for_dump_search_path() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let path = fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let session = ClaudeParser::parse(&path).unwrap();
    let bodies = ClaudeParser::extract_message_bodies(&path).unwrap();

    index.upsert_session(&session, 1, &bodies).unwrap();

    let bodies_by_session = index.dump_message_bodies().unwrap();
    let stored_body = bodies_by_session.get(&session.id).unwrap();
    assert!(!stored_body.trim().is_empty());
    assert!(stored_body.contains("github"));
}
