pub mod filter;

mod app;
mod composer;
mod fork_picker;
mod preview;
mod render;
mod theme;

use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::info;

use crate::{
    Session, db::Index, export::export_session, fork::fork_session, reindex::reindex_all,
    resume::dispatch_resume,
};

pub use app::App;
use filter::Chip;

type Tui = Terminal<CrosstermBackend<Stdout>>;

enum UiAction {
    Resume(Session),
    Fork { session: Session, cut_index: usize },
    Export(Session),
}

pub fn run() -> Result<()> {
    run_with_filter(Vec::new())
}

pub fn run_with_filter(seeded_chips: Vec<Chip>) -> Result<()> {
    let (claude_root, codex_root) = default_agent_roots()?;
    let mut index = Index::open()?;
    let stats = reindex_all(&mut index, &claude_root, &codex_root)?;
    info!(
        discovered = stats.discovered,
        parsed = stats.parsed,
        skipped_unchanged = stats.skipped_unchanged,
        failed = stats.failed,
        "initial TUI reindex complete"
    );

    let sessions = index.list_all()?;
    let message_bodies = index.dump_message_bodies()?;
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    let mut app =
        App::new_with_chips_and_bodies(sessions, current_cwd, seeded_chips, message_bodies);
    app.record_reindex(stats);

    let mut terminal = TerminalGuard::enter()?;
    let action = run_event_loop(
        terminal.terminal_mut(),
        &mut app,
        &mut index,
        &claude_root,
        &codex_root,
    )?;

    if let Some(action) = action {
        terminal.restore()?;
        match action {
            UiAction::Resume(session) => dispatch_resume(&session)?,
            UiAction::Fork { session, cut_index } => {
                fork_session(&session, &session.path, cut_index, &app.current_cwd, false)?;
            }
            UiAction::Export(session) => {
                let path = export_session(&session, &app.current_cwd)?;
                eprintln!("exported to {}", path.display());
            }
        }
    }

    Ok(())
}

fn run_event_loop(
    terminal: &mut Tui,
    app: &mut App,
    index: &mut Index,
    claude_root: &Path,
    codex_root: &Path,
) -> Result<Option<UiAction>> {
    loop {
        app.clear_expired_toast();
        terminal.draw(|frame| render::render(frame, app))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if is_key_press(key) => {
                if should_quit(key) {
                    return Ok(None);
                }
                if app.fork_picker().is_some() {
                    match key.code {
                        KeyCode::Enter => {
                            if let (Some(session), Some(cut_index)) = (
                                app.selected_session().cloned(),
                                app.selected_fork_cut_index(),
                            ) {
                                return Ok(Some(UiAction::Fork { session, cut_index }));
                            }
                        }
                        KeyCode::Esc => app.close_fork_picker(),
                        KeyCode::Up => app.move_fork_up(),
                        KeyCode::Down => app.move_fork_down(),
                        _ => {}
                    }
                    continue;
                }
                if should_reindex(key) {
                    refresh_index(index, claude_root, codex_root, app)?;
                    continue;
                }
                if should_fork(key) {
                    if let Err(error) = app.open_fork_picker() {
                        app.show_toast(error.to_string());
                    }
                    continue;
                }
                if should_export(key) {
                    return Ok(app.selected_session().cloned().map(UiAction::Export));
                }
                match key.code {
                    KeyCode::Enter => {
                        return Ok(app.selected_session().cloned().map(UiAction::Resume));
                    }
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Up => app.move_up(),
                    KeyCode::Down => app.move_down(),
                    KeyCode::PageUp => app.page_up(),
                    KeyCode::PageDown => app.page_down(),
                    _ => {
                        if matches!(key.code, KeyCode::Backspace)
                            && app.remove_seeded_chip_at_cursor_start()
                        {
                            continue;
                        }
                        if app.composer.handle_key(key) {
                            app.refresh_filters();
                        }
                    }
                }
            }
            Event::Paste(text)
                if app.fork_picker().is_none() && app.composer.insert_paste(&text) =>
            {
                app.refresh_filters();
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn refresh_index(
    index: &mut Index,
    claude_root: &Path,
    codex_root: &Path,
    app: &mut App,
) -> Result<()> {
    let stats = reindex_all(index, claude_root, codex_root)?;
    info!(
        discovered = stats.discovered,
        parsed = stats.parsed,
        skipped_unchanged = stats.skipped_unchanged,
        failed = stats.failed,
        "inline TUI reindex complete"
    );
    app.set_sessions_and_bodies(index.list_all()?, index.dump_message_bodies()?);
    app.record_reindex(stats);
    Ok(())
}

struct TerminalGuard {
    terminal: Tui,
    restored: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable terminal raw mode")?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(error).context("failed to enter alternate terminal screen");
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self {
                terminal,
                restored: false,
            }),
            Err(error) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
                Err(error).context("failed to initialize terminal")
            }
        }
    }

    fn terminal_mut(&mut self) -> &mut Tui {
        &mut self.terminal
    }

    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        let raw_result = disable_raw_mode().context("failed to disable terminal raw mode");
        let screen_result = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)
            .context("failed to leave alternate terminal screen");
        let cursor_result = self
            .terminal
            .show_cursor()
            .context("failed to show terminal cursor");
        self.restored = true;

        raw_result?;
        screen_result?;
        cursor_result?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn should_quit(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn should_reindex(key: KeyEvent) -> bool {
    key.code == KeyCode::F(5)
        || (key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn should_fork(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn should_export(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_key_press(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn default_agent_roots() -> Result<(PathBuf, PathBuf)> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok((home.join(".claude"), home.join(".codex")))
}
