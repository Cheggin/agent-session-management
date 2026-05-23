use std::path::{Path, PathBuf};

use asm::{
    ui::{filter::Chip, App},
    Agent,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn cwd() -> &'static Path {
    Path::new("/tmp/current")
}

fn here_cwd() -> PathBuf {
    cwd().canonicalize().unwrap_or_else(|_| cwd().to_path_buf())
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        assert!(app.handle_composer_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE,)));
    }
}

fn backspace(app: &mut App) {
    assert!(app.handle_composer_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE,)));
}

fn alt_backspace(app: &mut App) {
    assert!(app.handle_composer_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT,)));
}

#[test]
fn plain_search_token_stays_in_composer() {
    let mut app = App::new(Vec::new(), cwd().to_path_buf());

    type_text(&mut app, "claude ");

    assert_eq!(app.composer_input(), "claude ");
    assert_eq!(app.chips(), &[]);
    assert_eq!(app.search(), "claude");
}

#[test]
fn terminated_here_token_moves_from_composer_to_live_chip() {
    let mut app = App::new(Vec::new(), cwd().to_path_buf());

    type_text(&mut app, "@here ");

    assert_eq!(app.composer_input(), "");
    assert_eq!(app.chips(), &[Chip::HereCwd(here_cwd())]);
    assert_eq!(app.search(), "");
}

#[test]
fn pasted_multi_chip_string_extracts_each_completed_chip() {
    let mut app = App::new(Vec::new(), cwd().to_path_buf());

    assert!(app.insert_composer_paste("foo @claude bar @branch:main "));

    // Extraction removes completed chip tokens and their terminating spaces.
    // Non-chip text remains visible; search matching normalizes the trailing space.
    assert_eq!(app.composer_input(), "foo bar ");
    assert_eq!(
        app.chips(),
        &[Chip::Agent(Agent::Claude), Chip::Branch("main".to_owned())]
    );
    assert_eq!(app.search(), "foo bar");
}

#[test]
fn partial_chip_token_stays_literal() {
    let mut app = App::new(Vec::new(), cwd().to_path_buf());

    type_text(&mut app, "@cla");

    assert_eq!(app.composer_input(), "@cla");
    assert_eq!(app.chips(), &[]);
    assert_eq!(app.search(), "@cla");
}

#[test]
fn recognized_chip_token_without_terminating_space_stays_literal() {
    let mut app = App::new(Vec::new(), cwd().to_path_buf());

    type_text(&mut app, "@here");

    assert_eq!(app.composer_input(), "@here");
    assert_eq!(app.chips(), &[]);
    assert_eq!(app.search(), "@here");
}

#[test]
fn backspace_at_cursor_start_removes_live_chip_before_seeded_chip() {
    let seeded = Chip::HereCwd(here_cwd());
    let mut app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), vec![seeded.clone()]);

    type_text(&mut app, "@claude ");
    assert_eq!(app.chips(), &[seeded.clone(), Chip::Agent(Agent::Claude)]);

    backspace(&mut app);
    assert_eq!(app.chip_delete_pending, Some(1));
    assert_eq!(app.chips(), &[seeded.clone(), Chip::Agent(Agent::Claude)]);

    backspace(&mut app);
    assert_eq!(app.chips(), &[seeded]);
    assert_eq!(app.chip_delete_pending, None);

    backspace(&mut app);
    assert_eq!(app.chip_delete_pending, Some(0));
    assert_eq!(app.chips(), &[Chip::HereCwd(here_cwd())]);

    backspace(&mut app);
    assert_eq!(app.chips(), &[]);
    assert_eq!(app.chip_delete_pending, None);
}

#[test]
fn backspace_at_cursor_start_with_chip_first_highlights_then_deletes() {
    let seeded = Chip::HereCwd(here_cwd());
    let mut app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), vec![seeded]);

    backspace(&mut app);
    assert_eq!(app.chips().len(), 1);
    assert_eq!(app.chip_delete_pending, Some(0));

    backspace(&mut app);
    assert!(app.chips().is_empty());
    assert_eq!(app.chip_delete_pending, None);
}

#[test]
fn non_backspace_clears_pending() {
    let seeded = Chip::HereCwd(here_cwd());
    let mut app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), vec![seeded.clone()]);

    backspace(&mut app);
    assert_eq!(app.chip_delete_pending, Some(0));

    type_text(&mut app, "x");
    assert_eq!(app.chip_delete_pending, None);
    assert_eq!(app.chips(), &[seeded]);
}

#[test]
fn alt_backspace_with_only_chips_is_noop() {
    let seeded = Chip::HereCwd(here_cwd());
    let mut app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), vec![seeded.clone()]);

    alt_backspace(&mut app);

    assert_eq!(app.chips(), &[seeded]);
}

#[test]
fn alt_backspace_with_text_deletes_word_only() {
    let seeded = Chip::HereCwd(here_cwd());
    let mut app = App::new_with_chips(Vec::new(), cwd().to_path_buf(), vec![seeded.clone()]);

    type_text(&mut app, "foo bar");
    alt_backspace(&mut app);

    assert_eq!(app.composer_input(), "foo ");
    assert_eq!(app.chips(), &[seeded]);
}
