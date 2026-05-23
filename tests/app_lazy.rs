use std::{collections::HashMap, path::Path};

use asm::{Agent, Session, ui::App};
use chrono::{DateTime, Utc};

#[test]
fn app_loads_message_body_haystacks_lazily_for_search() {
    let session = Session {
        id: "session-1".to_owned(),
        agent: Agent::Claude,
        path: Path::new("/tmp/session-1.jsonl").to_path_buf(),
        cwd: Some(Path::new("/tmp/plain-project").to_path_buf()),
        git_branch: Some("main".to_owned()),
        entrypoint: None,
        title: Some("plain title".to_owned()),
        first_user_prompt: Some("plain prompt".to_owned()),
        recent_user_prompts: vec!["plain prompt".to_owned()],
        last_assistant_text: None,
        started_at: parse_utc("2026-05-22T12:00:00Z"),
        last_user_msg_at: Some(parse_utc("2026-05-22T12:01:00Z")),
        last_assistant_msg_at: None,
        user_msg_count: 1,
        is_live: false,
        is_sidechain: false,
    };
    let mut app = App::new(vec![session], Path::new("/tmp").to_path_buf());

    assert!(app.insert_composer_paste("storage"));
    assert!(app.needs_message_bodies());
    assert_eq!(app.filtered_count(), 0);

    app.set_message_bodies(HashMap::from([(
        "session-1".to_owned(),
        "hidden body text mentions storage".to_owned(),
    )]));

    assert!(!app.needs_message_bodies());
    assert_eq!(app.filtered_count(), 1);
}

fn parse_utc(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}
