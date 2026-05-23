use std::path::Path;

use asm::{
    Agent,
    cli::parse_filter_arg,
    ui::{App, filter::Chip},
};

fn cwd() -> &'static Path {
    Path::new("/tmp")
}

#[test]
fn filter_flag_claude_seeds_app_agent_chip() {
    let chips = parse_filter_arg("claude", cwd()).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(app.chips(), &[Chip::Agent(Agent::Claude)]);
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn filter_flag_branch_and_path_seed_app_chips() {
    let chips = parse_filter_arg("branch:main path:foo", cwd()).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(
        app.chips(),
        &[
            Chip::Branch("main".to_owned()),
            Chip::PathSubstring("foo".to_owned())
        ]
    );
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn filter_flag_rejects_unrecognized_token() {
    assert!(parse_filter_arg("notathing", cwd()).is_err());
}
