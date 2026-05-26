use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, mpsc::Receiver},
    time::{Duration as StdDuration, Instant},
};

use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{Session, reindex::ReindexStats};

use super::{
    composer::Composer,
    filter::{Chip, apply_filters_with_haystacks, build_session_haystacks_optional, parse_chip},
    fork_picker::ForkPicker,
};

#[derive(Debug)]
pub struct App {
    pub(crate) sessions: Vec<Session>,
    session_haystacks: Vec<String>,
    message_bodies: Option<Arc<HashMap<String, String>>>,
    pub(crate) filtered_indices: Vec<usize>,
    pub(crate) selected: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) list_height: usize,
    pub(crate) composer: Composer,
    pub(crate) chips: Vec<Chip>,
    pub chip_delete_pending: Option<usize>,
    pub(crate) search: String,
    pub(crate) current_cwd: PathBuf,
    pub(crate) last_reindex_time: Option<DateTime<Utc>>,
    fork_picker: Option<ForkPicker>,
    seeded_chips: Vec<Chip>,
    live_chips: Vec<Chip>,
    pushdown_filter_dirty: bool,
    toast: Option<Toast>,
    pub(crate) live_rx: Option<Receiver<HashSet<String>>>,
    pub(crate) update_rx: Option<Receiver<crate::update_check::AvailableUpdate>>,
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
        Self::new_inner(sessions, current_cwd, seeded_chips, None)
    }

    pub fn new_with_chips_and_bodies(
        sessions: Vec<Session>,
        current_cwd: PathBuf,
        seeded_chips: Vec<Chip>,
        message_bodies: HashMap<String, String>,
    ) -> Self {
        Self::new_inner(
            sessions,
            current_cwd,
            seeded_chips,
            Some(Arc::new(message_bodies)),
        )
    }

    fn new_inner(
        mut sessions: Vec<Session>,
        current_cwd: PathBuf,
        seeded_chips: Vec<Chip>,
        message_bodies: Option<Arc<HashMap<String, String>>>,
    ) -> Self {
        let current_cwd = current_cwd.canonicalize().unwrap_or(current_cwd);
        clear_live_sessions(&mut sessions);
        let session_haystacks =
            build_session_haystacks_optional(&sessions, message_bodies.as_deref());
        let mut app = Self {
            sessions,
            session_haystacks,
            message_bodies,
            filtered_indices: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            list_height: 1,
            composer: Composer::default(),
            chips: Vec::new(),
            chip_delete_pending: None,
            search: String::new(),
            current_cwd,
            last_reindex_time: None,
            fork_picker: None,
            seeded_chips,
            live_chips: Vec::new(),
            pushdown_filter_dirty: false,
            toast: None,
            live_rx: None,
            update_rx: None,
        };
        app.refresh_filters();
        app
    }

    pub fn set_sessions(&mut self, mut sessions: Vec<Session>) {
        clear_live_sessions(&mut sessions);
        self.session_haystacks =
            build_session_haystacks_optional(&sessions, self.message_bodies.as_deref());
        self.sessions = sessions;
        self.refresh_filters();
    }

    pub fn set_sessions_and_bodies(
        &mut self,
        mut sessions: Vec<Session>,
        message_bodies: HashMap<String, String>,
    ) {
        clear_live_sessions(&mut sessions);
        self.message_bodies = Some(Arc::new(message_bodies));
        self.session_haystacks =
            build_session_haystacks_optional(&sessions, self.message_bodies.as_deref());
        self.sessions = sessions;
        self.refresh_filters();
    }

    pub fn set_message_bodies(&mut self, message_bodies: HashMap<String, String>) {
        self.message_bodies = Some(Arc::new(message_bodies));
        self.session_haystacks =
            build_session_haystacks_optional(&self.sessions, self.message_bodies.as_deref());
        self.refresh_filters();
    }

    pub fn needs_message_bodies(&self) -> bool {
        !self.search.is_empty() && self.message_bodies.is_none()
    }

    pub fn message_bodies_loaded(&self) -> bool {
        self.message_bodies.is_some()
    }

    pub fn refresh_filters(&mut self) {
        self.extract_completed_chips();
        self.chips = self
            .seeded_chips
            .iter()
            .cloned()
            .chain(self.live_chips.iter().cloned())
            .collect();
        if self
            .chip_delete_pending
            .is_some_and(|pending| pending >= self.chips.len())
        {
            self.chip_delete_pending = None;
        }
        self.search = normalize_search_text(self.composer.input());
        self.filtered_indices = apply_filters_with_haystacks(
            &self.sessions,
            &self.chips,
            &self.search,
            &self.session_haystacks,
        );
        self.clamp_selection();
    }

    pub fn handle_composer_key(&mut self, key: KeyEvent) -> bool {
        if !matches!(key.code, KeyCode::Backspace) {
            self.chip_delete_pending = None;
        }

        if matches!(key.code, KeyCode::Backspace) {
            if key.modifiers.contains(KeyModifiers::ALT) {
                if self.composer.is_empty() {
                    return true;
                }
                self.composer.delete_word_backward();
                self.refresh_filters();
                return true;
            }

            if self.composer.cursor() == 0 && !self.chips.is_empty() {
                return self.handle_chip_backspace();
            }
        }

        if !self.composer.handle_key(key) {
            return false;
        }

        self.refresh_filters();
        true
    }

    pub fn insert_composer_paste(&mut self, text: &str) -> bool {
        if !self.composer.insert_paste(text) {
            return false;
        }

        self.refresh_filters();
        true
    }

    pub fn remove_chip_at_cursor_start(&mut self) -> bool {
        if self.composer.cursor() != 0 {
            return false;
        }

        if self.chips.is_empty() {
            return false;
        };

        self.pop_rightmost_chip();
        self.chip_delete_pending = None;
        self.refresh_filters();
        true
    }

    fn handle_chip_backspace(&mut self) -> bool {
        if self.chip_delete_pending.is_none() {
            self.chip_delete_pending = Some(self.chips.len() - 1);
            return true;
        }

        self.pop_rightmost_chip();
        self.chip_delete_pending = None;
        self.refresh_filters();
        true
    }

    fn pop_rightmost_chip(&mut self) {
        let chip = self.live_chips.pop().or_else(|| self.seeded_chips.pop());

        if chip
            .as_ref()
            .is_some_and(super::filter::is_sql_pushdown_chip)
        {
            self.pushdown_filter_dirty = true;
        }
    }

    pub fn take_pushdown_filter_dirty(&mut self) -> bool {
        let dirty = self.pushdown_filter_dirty;
        self.pushdown_filter_dirty = false;
        dirty
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
        self.show_toast_for(message, StdDuration::from_secs(3));
    }

    pub fn show_toast_for(&mut self, message: impl Into<String>, duration: StdDuration) {
        self.toast = Some(Toast {
            message: message.into(),
            until: Instant::now() + duration,
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

    pub(crate) fn apply_live_session_ids(&mut self, live_ids: &HashSet<String>) {
        for session in &mut self.sessions {
            session.is_live = live_ids.contains(&session.id);
        }
        self.refresh_filters();
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

    pub fn filtered_sessions(&self) -> impl Iterator<Item = &Session> {
        self.filtered_indices
            .iter()
            .filter_map(|idx| self.sessions.get(*idx))
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

    fn extract_completed_chips(&mut self) {
        while let Some((start, end, chip)) =
            find_completed_chip(self.composer.input(), &self.current_cwd)
        {
            self.composer.remove_range(start, end);
            self.live_chips.push(chip);
        }
    }
}

fn find_completed_chip(input: &str, current_cwd: &Path) -> Option<(usize, usize, Chip)> {
    let chars = input.chars().collect::<Vec<_>>();
    let mut idx = 0;

    while idx < chars.len() {
        while idx < chars.len() && chars[idx].is_whitespace() {
            idx += 1;
        }

        let start = idx;
        while idx < chars.len() && !chars[idx].is_whitespace() {
            idx += 1;
        }

        if start == idx || idx == chars.len() {
            break;
        }

        let token = chars[start..idx].iter().collect::<String>();
        let remove_end = idx + 1;
        if let Some(chip) = parse_chip(&token, current_cwd) {
            return Some((start, remove_end, chip));
        }

        idx = remove_end;
    }

    None
}

fn normalize_search_text(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clear_live_sessions(sessions: &mut [Session]) {
    for session in sessions {
        session.is_live = false;
    }
}
