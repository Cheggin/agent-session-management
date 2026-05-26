pub mod filter;

mod app;
mod composer;
mod fork_picker;
mod render;
mod sparkline;
mod theme;

use std::{
    io::{self, IsTerminal, Stdout},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend, TestBackend},
};
use tracing::{info, info_span, warn};

use crate::{
    Session, db::Index, export::export_session, fork::fork_session, liveness::LivenessSnapshot,
    reindex::reindex_all, resume::dispatch_resume, update_check,
};

pub use app::App;
use filter::Chip;

type Tui = Terminal<CrosstermBackend<Stdout>>;
const RECENT_REINDEX_THRESHOLD: Duration = Duration::from_secs(5);

enum UiAction {
    Resume(Session),
    Fork { session: Session, cut_index: usize },
    Export(Session),
}

struct DeferredReindex {
    claude_root: PathBuf,
    codex_root: PathBuf,
}

struct BackgroundReindexComplete {
    result: std::result::Result<crate::reindex::ReindexStats, String>,
}

type BackgroundReindexReceiver = mpsc::Receiver<BackgroundReindexComplete>;
type LiveSessionReceiver = mpsc::Receiver<LivenessSnapshot>;

enum StartupReindex {
    Complete(Option<crate::reindex::ReindexStats>),
    Deferred(DeferredReindex),
}

pub fn run() -> Result<()> {
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    let current_cwd = current_cwd.canonicalize().unwrap_or(current_cwd);
    run_with_filter(vec![Chip::HereCwd(current_cwd)])
}

pub fn run_with_filter(seeded_chips: Vec<Chip>) -> Result<()> {
    run_with_filter_started(seeded_chips, Instant::now())
}

pub fn run_benchmark_with_filter_started(
    seeded_chips: Vec<Chip>,
    startup_started: Instant,
) -> Result<()> {
    let (claude_root, codex_root) = default_agent_roots()?;
    let mut index = Index::open()?;
    let startup_reindex = maybe_reindex_on_startup(&mut index, &claude_root, &codex_root)?;

    let sessions = index.list_filtered(&seeded_chips)?;
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    let mut app = {
        let span = info_span!("asm.app.new");
        let _enter = span.enter();
        let started = Instant::now();
        let app = App::new_with_chips(sessions, current_cwd, seeded_chips);
        info!(elapsed_ms = elapsed_ms(started), "startup phase complete");
        app
    };
    match startup_reindex {
        StartupReindex::Complete(Some(stats)) => app.record_reindex(stats),
        StartupReindex::Complete(None) => {}
        StartupReindex::Deferred(_) => app.show_toast("updating index..."),
    }

    let elapsed = draw_benchmark_frame(&mut app, startup_started)?;
    println!("asm benchmark: {}ms", elapsed.as_millis());
    Ok(())
}

pub fn run_with_filter_started(seeded_chips: Vec<Chip>, startup_started: Instant) -> Result<()> {
    let (claude_root, codex_root) = default_agent_roots()?;
    let mut index = Index::open()?;
    let startup_reindex = maybe_reindex_on_startup(&mut index, &claude_root, &codex_root)?;

    let sessions = index.list_filtered(&seeded_chips)?;
    let current_cwd = std::env::current_dir().context("failed to read current directory")?;
    let mut app = {
        let span = info_span!("asm.app.new");
        let _enter = span.enter();
        let started = Instant::now();
        let app = App::new_with_chips(sessions, current_cwd, seeded_chips);
        info!(elapsed_ms = elapsed_ms(started), "startup phase complete");
        app
    };
    let deferred_reindex = match startup_reindex {
        StartupReindex::Complete(Some(stats)) => {
            app.record_reindex(stats);
            None
        }
        StartupReindex::Complete(None) => None,
        StartupReindex::Deferred(deferred) => {
            app.show_toast("updating index...");
            Some(deferred)
        }
    };

    let mut terminal = TerminalGuard::enter()?;
    let action = run_event_loop(
        terminal.terminal_mut(),
        &mut app,
        &mut index,
        &claude_root,
        &codex_root,
        startup_started,
        deferred_reindex,
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
    startup_started: Instant,
    deferred_reindex: Option<DeferredReindex>,
) -> Result<Option<UiAction>> {
    let event_loop_started = Instant::now();
    let mut first_render_logged = false;
    let mut deferred_reindex = deferred_reindex;
    let mut background_reindex: Option<BackgroundReindexReceiver> = None;
    loop {
        app.clear_expired_toast();
        if first_render_logged {
            terminal.draw(|frame| render::render(frame, app))?;
        } else {
            let span = info_span!("asm.first_render");
            let _enter = span.enter();
            let render_result = draw_first_frame(terminal, app);
            info!(
                elapsed_ms = elapsed_ms(event_loop_started),
                ok = render_result.is_ok(),
                "startup phase complete"
            );
            render_result?;
            first_render_logged = true;

            let span = info_span!("asm.startup");
            let _enter = span.enter();
            info!(
                elapsed_ms = elapsed_ms(startup_started),
                "first frame drawn"
            );
            start_live_session_scan(app);
            start_update_check(app);
            if let Some(deferred) = deferred_reindex.take() {
                background_reindex = Some(start_background_reindex(deferred));
            }
        }

        let mut needs_redraw = false;
        if let Some(receiver) = background_reindex.take() {
            match receiver.try_recv() {
                Ok(complete) => {
                    if handle_background_reindex_complete(complete, index, app)? {
                        start_live_session_scan(app);
                    }
                    needs_redraw = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    background_reindex = Some(receiver);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    warn!("background reindex worker disconnected");
                    app.show_toast("index update failed");
                    needs_redraw = true;
                }
            }
        }

        if poll_live_session_scan(app) {
            needs_redraw = true;
        }

        if poll_update_check(app) {
            needs_redraw = true;
        }

        if needs_redraw {
            continue;
        }

        let event_poll_timeout = if app.live_rx.is_some() {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(100)
        };
        if !event::poll(event_poll_timeout)? {
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
                    start_live_session_scan(app);
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
                        if app.handle_composer_key(key) {
                            if maybe_requery_filtered_sessions(index, app)? {
                                start_live_session_scan(app);
                            }
                            maybe_load_message_bodies(index, app)?;
                        }
                    }
                }
            }
            Event::Paste(text)
                if app.fork_picker().is_none() && app.insert_composer_paste(&text) =>
            {
                if maybe_requery_filtered_sessions(index, app)? {
                    start_live_session_scan(app);
                }
                maybe_load_message_bodies(index, app)?;
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

fn draw_benchmark_frame(app: &mut App, startup_started: Instant) -> Result<Duration> {
    if io::stdout().is_terminal() {
        let mut terminal = TerminalGuard::enter()?;
        let render_result = draw_first_frame(terminal.terminal_mut(), app);
        let elapsed = startup_started.elapsed();
        log_first_render_for_benchmark(render_result.is_ok(), elapsed);
        render_result?;
        terminal.restore()?;
        Ok(elapsed)
    } else {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).context("failed to initialize test terminal")?;
        let render_result = draw_first_frame(&mut terminal, app);
        let elapsed = startup_started.elapsed();
        log_first_render_for_benchmark(render_result.is_ok(), elapsed);
        render_result?;
        Ok(elapsed)
    }
}

fn draw_first_frame<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    B::Error: std::fmt::Debug,
{
    terminal
        .draw(|frame| render::render(frame, app))
        .map_err(|error| anyhow::anyhow!("failed to draw first TUI frame: {error:?}"))?;
    Ok(())
}

fn log_first_render_for_benchmark(ok: bool, elapsed: Duration) {
    let span = info_span!("asm.first_render");
    let _enter = span.enter();
    info!(
        elapsed_ms = elapsed.as_millis(),
        ok, "startup phase complete"
    );
    let span = info_span!("asm.startup");
    let _enter = span.enter();
    info!(elapsed_ms = elapsed.as_millis(), "first frame drawn");
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

fn maybe_reindex_on_startup(
    index: &mut Index,
    claude_root: &Path,
    codex_root: &Path,
) -> Result<StartupReindex> {
    if let Some(cache_age) = index.last_reindex_age()? {
        if cache_age < RECENT_REINDEX_THRESHOLD {
            info!(
                cache_age_secs = cache_age.as_secs(),
                threshold_secs = RECENT_REINDEX_THRESHOLD.as_secs(),
                "reindex skipped"
            );
            return Ok(StartupReindex::Complete(None));
        }

        info!(
            cache_age_secs = cache_age.as_secs(),
            threshold_secs = RECENT_REINDEX_THRESHOLD.as_secs(),
            "reindex deferred until after first render"
        );
        return Ok(StartupReindex::Deferred(DeferredReindex {
            claude_root: claude_root.to_path_buf(),
            codex_root: codex_root.to_path_buf(),
        }));
    }

    let stats = reindex_all(index, claude_root, codex_root)?;
    info!(
        discovered = stats.discovered,
        parsed = stats.parsed,
        skipped_unchanged = stats.skipped_unchanged,
        failed = stats.failed,
        "initial TUI reindex complete"
    );
    Ok(StartupReindex::Complete(Some(stats)))
}

fn start_background_reindex(deferred: DeferredReindex) -> BackgroundReindexReceiver {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut index = Index::open()?;
            reindex_all(&mut index, &deferred.claude_root, &deferred.codex_root)
        })()
        .map_err(|error| error.to_string());
        let _ = sender.send(BackgroundReindexComplete { result });
    });
    receiver
}

fn start_live_session_scan(app: &mut App) {
    let (sender, receiver): (mpsc::Sender<LivenessSnapshot>, LiveSessionReceiver) = mpsc::channel();
    let sessions = app.sessions.clone();
    app.live_rx = Some(receiver);
    std::thread::spawn(move || {
        let live_ids = crate::liveness::live_session_ids(&sessions);
        let _ = sender.send(live_ids);
    });
}

fn poll_live_session_scan(app: &mut App) -> bool {
    let Some(receiver) = app.live_rx.take() else {
        return false;
    };

    match receiver.try_recv() {
        Ok(live_ids) => {
            app.apply_live_session_ids(&live_ids);
            true
        }
        Err(mpsc::TryRecvError::Empty) => {
            app.live_rx = Some(receiver);
            false
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            warn!("background liveness worker disconnected");
            false
        }
    }
}

fn start_update_check(app: &mut App) {
    app.update_rx = update_check::spawn_check();
}

fn poll_update_check(app: &mut App) -> bool {
    let Some(receiver) = app.update_rx.take() else {
        return false;
    };

    match receiver.try_recv() {
        Ok(update) => {
            app.show_toast_for(
                format!(
                    "update available: v{} \u{2014} {}",
                    update.latest,
                    update_check::update_command_hint()
                ),
                Duration::from_secs(15),
            );
            true
        }
        Err(mpsc::TryRecvError::Empty) => {
            app.update_rx = Some(receiver);
            false
        }
        Err(mpsc::TryRecvError::Disconnected) => false,
    }
}

fn handle_background_reindex_complete(
    complete: BackgroundReindexComplete,
    index: &mut Index,
    app: &mut App,
) -> Result<bool> {
    match complete.result {
        Ok(stats) => {
            info!(
                discovered = stats.discovered,
                parsed = stats.parsed,
                skipped_unchanged = stats.skipped_unchanged,
                failed = stats.failed,
                "background TUI reindex complete"
            );
            let sessions = index.list_filtered(app.chips())?;
            if app.message_bodies_loaded() {
                app.set_sessions_and_bodies(sessions, index.dump_message_bodies()?);
            } else {
                app.set_sessions(sessions);
            }
            app.record_reindex(stats);
            Ok(true)
        }
        Err(error) => {
            warn!(%error, "background TUI reindex failed");
            app.show_toast("index update failed");
            Ok(false)
        }
    }
}

fn maybe_load_message_bodies(index: &Index, app: &mut App) -> Result<()> {
    if !app.needs_message_bodies() {
        return Ok(());
    }

    let message_bodies = index.dump_message_bodies()?;
    let body_session_count = message_bodies.len();
    app.set_message_bodies(message_bodies);
    info!(body_session_count, "lazy message bodies loaded for search");
    Ok(())
}

fn maybe_requery_filtered_sessions(index: &Index, app: &mut App) -> Result<bool> {
    if !app.take_pushdown_filter_dirty() {
        return Ok(false);
    }

    let chips = app.chips().to_vec();
    let sessions = index.list_filtered(&chips)?;
    if app.message_bodies_loaded() {
        app.set_sessions_and_bodies(sessions, index.dump_message_bodies()?);
    } else {
        app.set_sessions(sessions);
    }
    Ok(true)
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
    let sessions = index.list_filtered(app.chips())?;
    if app.message_bodies_loaded() {
        app.set_sessions_and_bodies(sessions, index.dump_message_bodies()?);
    } else {
        app.set_sessions(sessions);
    }
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
