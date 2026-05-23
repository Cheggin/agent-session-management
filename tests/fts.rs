use std::path::PathBuf;

use asm::{ClaudeParser, Parser, db::Index};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn fts_match_finds_session_by_message_body() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let path = fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let session = ClaudeParser::parse(&path).unwrap();
    let bodies = ClaudeParser::extract_message_bodies(&path).unwrap();

    index.upsert_session(&session, 1, &bodies).unwrap();

    let matches = index.fts_match("github").unwrap();
    assert!(matches.contains(&session.id));
}

#[test]
fn fts_match_returns_empty_for_missing_body_term() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let path = fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let session = ClaudeParser::parse(&path).unwrap();
    let bodies = ClaudeParser::extract_message_bodies(&path).unwrap();

    index.upsert_session(&session, 1, &bodies).unwrap();

    assert!(index.fts_match("xyznonexistent").unwrap().is_empty());
}

#[test]
fn fts_rows_are_replaced_when_session_is_reupserted() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open_in_dir(temp.path()).unwrap();
    let path = fixture("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let session = ClaudeParser::parse(&path).unwrap();
    let bodies = ClaudeParser::extract_message_bodies(&path).unwrap();

    index.upsert_session(&session, 1, &bodies).unwrap();
    index
        .upsert_session(&session, 2, &["replacement banana body".to_owned()])
        .unwrap();

    assert!(index.fts_match("github").unwrap().is_empty());
    let matches = index.fts_match("banana").unwrap();
    assert_eq!(matches, [session.id].into_iter().collect());
}
