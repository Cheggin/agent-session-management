use std::{fs, path::Path};

use asm::{Agent, Entrypoint, Session, export::export_session};
use chrono::{DateTime, Utc};

#[test]
fn export_session_writes_bookend_markdown() {
    let temp = tempfile::tempdir().unwrap();
    let session = session_literal(temp.path());

    let path = export_session(&session, temp.path()).unwrap();
    let markdown = fs::read_to_string(&path).unwrap();

    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("asm-export-claude-019d8eaf.md")
    );
    assert!(markdown.contains("# Ship export support"));
    assert!(markdown.contains("- **Agent:** claude"));
    assert!(markdown.contains("- **Session id:** 019d8eaf-1998-76f2-a95e-8b65fadc3a1c"));
    assert!(markdown.contains(&format!(
        "- **Cwd:** {}",
        temp.path().join("repo").display()
    )));
    assert!(markdown.contains("- **Branch:** main"));
    assert!(markdown.contains("- **Started:** 2026-05-22T12:00:00Z"));
    assert!(markdown.contains("- **Last activity:** 2026-05-22T12:12:00Z"));
    assert!(markdown.contains(&format!(
        "- **Source file:** {}",
        temp.path().join("source.jsonl").display()
    )));
    assert!(markdown.contains("### You\nAdd markdown export with full first prompt."));
    assert!(markdown.contains("### Assistant (most recent)\nImplemented the exporter."));
    assert!(markdown.contains("Note: this export shows the bookends only."));
}

#[test]
fn export_session_disambiguates_existing_default_path() {
    let temp = tempfile::tempdir().unwrap();
    let session = session_literal(temp.path());

    let first = export_session(&session, temp.path()).unwrap();
    let second = export_session(&session, temp.path()).unwrap();

    assert_ne!(first, second);
    assert_eq!(
        first.file_name().and_then(|name| name.to_str()),
        Some("asm-export-claude-019d8eaf.md")
    );
    assert_eq!(
        second.file_name().and_then(|name| name.to_str()),
        Some("asm-export-claude-019d8eaf-1.md")
    );
    assert!(first.exists());
    assert!(second.exists());
}

fn session_literal(root: &Path) -> Session {
    Session {
        id: "019d8eaf-1998-76f2-a95e-8b65fadc3a1c".to_owned(),
        agent: Agent::Claude,
        path: root.join("source.jsonl"),
        cwd: Some(root.join("repo")),
        git_branch: Some("main".to_owned()),
        entrypoint: Some(Entrypoint::Cli),
        title: Some("Ship export support".to_owned()),
        first_user_prompt: Some("Add markdown export with full first prompt.".to_owned()),
        last_assistant_text: Some("Implemented the exporter.".to_owned()),
        started_at: parse_utc("2026-05-22T12:00:00Z"),
        last_user_msg_at: Some(parse_utc("2026-05-22T12:10:00Z")),
        last_assistant_msg_at: Some(parse_utc("2026-05-22T12:12:00Z")),
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
