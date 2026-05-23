use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration as StdDuration, Instant},
};

use chrono::{DateTime, Utc};

use crate::{Session, liveness::mark_live_sessions, reindex::ReindexStats};

use super::{
    composer::Composer,
    filter::{Chip, apply_filters_with_haystacks, build_session_haystacks, parse_filter_text},
    fork_picker::ForkPicker,
};

#[derive(Debug)]
pub struct App {
    pub(crate) sessions: Vec<Session>,
    session_haystacks: Vec<String>,
    pub(crate) filtered_indices: Vec<usize>,
    pub(crate) selected: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) list_height: usize,
    pub(crate) composer: Composer,
    pub(crate) chips: Vec<Chip>,
    pub(crate) search: String,
    pub(crate) current_cwd: PathBuf,
    pub(crate) last_reindex_time: Option<DateTime<Utc>>,
    fork_picker: Option<ForkPicker>,
    seeded_chips: Vec<Chip>,
    toast: Option<Toast>,
}

#[derive(Debug)]
struct Toast {
    message: String,
    until: Instant,
}

impl App {
    pub fn new(sessions: Vec<Session>, current_cwd: PathBuf) -> Self {
        Self::new_with_chips(sessions, current_cwd, Vec::new())
    }

    pub fn new_with_chips(
        sessions: Vec<Session>,
        current_cwd: PathBuf,
        seeded_chips: Vec<Chip>,
    ) -> Self {
        Self::new_with_chips_and_bodies(sessions, current_cwd, seeded_chips, HashMap::new())
    }

    pub fn new_with_chips_and_bodies(
        mut sessions: Vec<Session>,
        current_cwd: PathBuf,
        seeded_chips: Vec<Chip>,
        message_bodies: HashMap<String, String>,
    ) -> Self {
        let current_cwd = current_cwd.canonicalize().unwrap_or(current_cwd);
        mark_live_sessions(&mut sessions);
        let session_haystacks = build_session_haystacks(&sessions, &message_bodies);
        let mut app = Self {
            sessions,
            session_haystacks,
            filtered_indices: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            list_height: 1,
            composer: Composer::default(),
            chips: Vec::new(),
            search: String::new(),
            current_cwd,
            last_reindex_time: None,
            fork_picker: None,
            seeded_chips,
            toast: None,
        };
        app.refresh_filters();
        app
    }

    pub fn set_sessions(&mut self, mut sessions: Vec<Session>) {
        mark_live_sessions(&mut sessions);
        self.session_haystacks = build_session_haystacks(&sessions, &HashMap::new());
        self.sessions = sessions;
        self.refresh_filters();
    }

    pub fn set_sessions_and_bodies(
        &mut self,
        mut sessions: Vec<Session>,
        message_bodies: HashMap<String, String>,
    ) {
        mark_live_sessions(&mut sessions);
        self.session_haystacks = build_session_haystacks(&sessions, &message_bodies);
        self.sessions = sessions;
        self.refresh_filters();
    }

    pub fn refresh_filters(&mut self) {
        let parsed = parse_filter_text(self.composer.input(), &self.current_cwd);
        self.chips = self
            .seeded_chips
            .iter()
            .cloned()
            .chain(parsed.chips)
            .collect();
        self.search = parsed.search;
        self.filtered_indices = apply_filters_with_haystacks(
            &self.sessions,
            &self.chips,
            &self.search,
            &self.session_haystacks,
        );
        self.clamp_selection();
    }

    pub fn remove_seeded_chip_at_cursor_start(&mut self) -> bool {
        if self.composer.cursor() != 0 || self.seeded_chips.is_empty() {
            return false;
        }
        self.seeded_chips.pop();
        self.refresh_filters();
        true
    }

    pub fn chips(&self) -> &[Chip] {
        &self.chips
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn composer_input(&self) -> &str {
        self.composer.input()
    }

    pub fn record_reindex(&mut self, stats: ReindexStats) {
        self.last_reindex_time = Some(Utc::now());
        self.toast = Some(Toast {
            message: format!(
                "reindexed: {} parsed, {} skipped, {} failed",
                stats.parsed, stats.skipped_unchanged, stats.failed
            ),
            until: Instant::now() + StdDuration::from_secs(3),
        });
    }

    pub fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast {
            message: message.into(),
            until: Instant::now() + StdDuration::from_secs(3),
        });
    }

    pub fn clear_expired_toast(&mut self) {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| Instant::now() >= toast.until)
        {
            self.toast = None;
        }
    }

    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(|toast| toast.message.as_str())
    }

    pub fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|idx| self.sessions.get(*idx))
    }

    pub fn open_fork_picker(&mut self) -> anyhow::Result<()> {
        let Some(session) = self.selected_session() else {
            self.show_toast("no session selected");
            return Ok(());
        };
        self.fork_picker = Some(ForkPicker::from_session(session)?);
        Ok(())
    }

    pub fn close_fork_picker(&mut self) {
        self.fork_picker = None;
    }

    pub fn fork_picker(&self) -> Option<&ForkPicker> {
        self.fork_picker.as_ref()
    }

    pub fn fork_picker_mut(&mut self) -> Option<&mut ForkPicker> {
        self.fork_picker.as_mut()
    }

    pub fn selected_fork_cut_index(&self) -> Option<usize> {
        self.fork_picker
            .as_ref()
            .map(ForkPicker::selected_cut_index)
    }

    pub fn move_fork_up(&mut self) {
        if let Some(picker) = &mut self.fork_picker {
            picker.move_up();
        }
    }

    pub fn move_fork_down(&mut self) {
        if let Some(picker) = &mut self.fork_picker {
            picker.move_down();
        }
    }

    pub fn set_list_height(&mut self, height: usize) {
        self.list_height = height.max(1);
        self.ensure_selected_visible();
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.ensure_selected_visible();
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
            self.ensure_selected_visible();
        }
    }

    pub fn page_up(&mut self) {
        let page = self.list_height.saturating_sub(1).max(1);
        self.selected = self.selected.saturating_sub(page);
        self.ensure_selected_visible();
    }

    pub fn page_down(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let page = self.list_height.saturating_sub(1).max(1);
        self.selected = (self.selected + page).min(self.filtered_indices.len() - 1);
        self.ensure_selected_visible();
    }

    pub fn visible_indices(&self) -> impl Iterator<Item = (usize, &Session)> {
        self.filtered_indices
            .iter()
            .skip(self.scroll_offset)
            .take(self.list_height)
            .filter_map(|idx| self.sessions.get(*idx).map(|session| (*idx, session)))
    }

    fn clamp_selection(&mut self) {
        if self.filtered_indices.is_empty() {
            self.selected = 0;
            self.scroll_offset = 0;
            return;
        }
        if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len() - 1;
        }
        self.ensure_selected_visible();
    }

    fn ensure_selected_visible(&mut self) {
        if self.filtered_indices.is_empty() {
            self.scroll_offset = 0;
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        let bottom = self.scroll_offset + self.list_height;
        if self.selected >= bottom {
            self.scroll_offset = self.selected + 1 - self.list_height;
        }
    }
}
