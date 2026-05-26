use std::{
    fs,
    path::{Path, PathBuf},
};

use asm::{ClaudeParser, Parser, export::export_session};

#[test]
fn export_session_writes_full_conversation_markdown() {
    let temp = tempfile::tempdir().unwrap();
    let source = multi_turn_claude_source(temp.path());
    let session = ClaudeParser::parse(&source).unwrap();

    let path = export_session(&session, temp.path()).unwrap();
    let markdown = fs::read_to_string(&path).unwrap();

    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("asm-export-claude-b0fd6a29.md")
    );
    assert!(markdown.contains("- **Agent:** claude"));
    assert!(markdown.contains("- **Session id:** b0fd6a29-a517-4325-a68a-fd7c1fbba04b"));
    assert!(markdown.contains(&format!("- **Source file:** {}", source.display())));
    assert!(markdown.contains("## Conversation"));
    assert!(markdown.matches("### You").count() >= 2);
    assert!(markdown.matches("### Assistant").count() >= 2);
    assert!(!markdown.contains("Note: this export shows the bookends only."));
}

#[test]
fn export_session_disambiguates_existing_default_path() {
    let temp = tempfile::tempdir().unwrap();
    let session = ClaudeParser::parse(&claude_fixture()).unwrap();

    let first = export_session(&session, temp.path()).unwrap();
    let second = export_session(&session, temp.path()).unwrap();

    assert_ne!(first, second);
    assert_eq!(
        first.file_name().and_then(|name| name.to_str()),
        Some("asm-export-claude-b0fd6a29.md")
    );
    assert_eq!(
        second.file_name().and_then(|name| name.to_str()),
        Some("asm-export-claude-b0fd6a29-1.md")
    );
    assert!(first.exists());
    assert!(second.exists());
}

fn claude_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude/b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl")
}

fn multi_turn_claude_source(root: &Path) -> PathBuf {
    let source = root.join("b0fd6a29-a517-4325-a68a-fd7c1fbba04b.jsonl");
    let first_turn = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude/47370d97-696e-4a07-a96b-1fdf288749d2.jsonl"),
    )
    .unwrap();
    let later_turn = fs::read_to_string(claude_fixture()).unwrap();

    fs::write(source.as_path(), format!("{first_turn}\n{later_turn}")).unwrap();
    source
}
