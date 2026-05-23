use std::path::{Path, PathBuf};

use asm::{
    Agent,
    cli::{parse_filter_arg, parse_list_filter_chips},
    ui::{App, filter::Chip},
};

fn cwd() -> &'static Path {
    Path::new("/tmp")
}

fn here_cwd() -> PathBuf {
    cwd().canonicalize().unwrap_or_else(|_| cwd().to_path_buf())
}

#[test]
fn bare_list_defaults_to_here_scope() {
    let chips = parse_list_filter_chips(None, cwd(), false).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(app.chips(), &[Chip::HereCwd(here_cwd())]);
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn filter_flag_claude_defaults_to_here_scope() {
    let chips = parse_list_filter_chips(Some("claude"), cwd(), false).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(
        app.chips(),
        &[Chip::Agent(Agent::Claude), Chip::HereCwd(here_cwd())]
    );
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn filter_flag_branch_and_path_seed_app_chips() {
    let chips = parse_list_filter_chips(Some("branch:main path:foo"), cwd(), false).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(
        app.chips(),
        &[
            Chip::Branch("main".to_owned()),
            Chip::PathSubstring("foo".to_owned()),
            Chip::HereCwd(here_cwd())
        ]
    );
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn filter_flag_here_token_is_not_duplicated() {
    let chips = parse_list_filter_chips(Some("claude here"), cwd(), false).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(
        app.chips(),
        &[Chip::Agent(Agent::Claude), Chip::HereCwd(here_cwd())]
    );
}

#[test]
fn filter_flag_at_here_token_is_not_duplicated() {
    let chips = parse_list_filter_chips(Some("claude @here"), cwd(), false).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(
        app.chips(),
        &[Chip::Agent(Agent::Claude), Chip::HereCwd(here_cwd())]
    );
}

#[test]
fn global_flag_without_filter_disables_here_scope() {
    let chips = parse_list_filter_chips(None, cwd(), true).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(app.chips(), &[]);
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn global_flag_with_filter_keeps_filter_only() {
    let chips = parse_list_filter_chips(Some("claude"), cwd(), true).unwrap();
    let app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), chips);

    assert_eq!(app.chips(), &[Chip::Agent(Agent::Claude)]);
    assert_eq!(app.search(), "");
    assert_eq!(app.composer_input(), "");
}

#[test]
fn filter_flag_rejects_unrecognized_token() {
    assert!(parse_filter_arg("notathing", cwd()).is_err());
}
