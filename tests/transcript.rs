use std::path::PathBuf;

use asm::{ClaudeParser, CodexParser, TranscriptRole, TranscriptTurn};

#[test]
fn claude_extract_transcript_returns_user_and_assistant_turns_in_order() {
    let turns = ClaudeParser::extract_transcript(&claude_fixture()).unwrap();

    assert!(has_role(&turns, TranscriptRole::User));
    assert!(has_role(&turns, TranscriptRole::Assistant));
    assert_chronological(&turns);
}

#[test]
fn codex_extract_transcript_returns_user_and_assistant_turns_in_order() {
    let turns = CodexParser::extract_transcript(&codex_fixture()).unwrap();

    assert!(has_role(&turns, TranscriptRole::User));
    assert!(has_role(&turns, TranscriptRole::Assistant));
    assert_chronological(&turns);
}

fn has_role(turns: &[TranscriptTurn], role: TranscriptRole) -> bool {
    turns.iter().any(|turn| turn.role == role)
}

fn assert_chronological(turns: &[TranscriptTurn]) {
    for pair in turns.windows(2) {
        if let (Some(left), Some(right)) = (pair[0].timestamp, pair[1].timestamp) {
            assert!(
                left <= right,
                "transcript timestamps must be chronological: {left} > {right}"
            );
        }
    }
}

fn claude_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl")
}

fn codex_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex/rollout-2026-04-14T18-08-53-019d8eaf-1998-76f2-a95e-8b65fadc3a1c.jsonl")
}
