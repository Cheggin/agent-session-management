use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    Session,
    fork::{end_cut_index, read_turns},
};

use super::theme;

#[derive(Debug, Clone)]
pub struct ForkPicker {
    rows: Vec<ForkRow>,
    selected: usize,
    scroll_offset: usize,
    height: usize,
}

#[derive(Debug, Clone)]
struct ForkRow {
    cut_index: usize,
    label: String,
}

impl ForkPicker {
    pub fn from_session(session: &Session) -> Result<Self> {
        let end = end_cut_index(&session.path)?;
        let mut turns = read_turns(&session.agent, &session.path)?;
        if turns.is_empty() {
            bail!("source session {} has 0 user turns", session.path.display());
        }
        turns.reverse();

        let mut rows = vec![ForkRow {
            cut_index: end,
            label: "end of session (full copy)".to_owned(),
        }];
        rows.extend(turns.into_iter().map(|turn| ForkRow {
            cut_index: turn.cut_index,
            label: format!(
                "{:>4}  {}",
                turn.timestamp
                    .map(relative_time)
                    .unwrap_or_else(|| "n/a".to_owned()),
                truncate(&turn.message.replace('\n', " "), 80)
            ),
        }));

        Ok(Self {
            rows,
            selected: 0,
            scroll_offset: 0,
            height: 1,
        })
    }

    pub fn selected_cut_index(&self) -> usize {
        self.rows
            .get(self.selected)
            .map(|row| row.cut_index)
            .unwrap_or(0)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.ensure_selected_visible();
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.rows.len() {
            self.selected += 1;
            self.ensure_selected_visible();
        }
    }

    fn set_height(&mut self, height: usize) {
        self.height = height.max(1);
        self.ensure_selected_visible();
    }

    fn ensure_selected_visible(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        let bottom = self.scroll_offset + self.height;
        if self.selected >= bottom {
            self.scroll_offset = self.selected + 1 - self.height;
        }
    }
}

pub fn render_overlay(frame: &mut Frame<'_>, area: Rect, picker: &mut ForkPicker) {
    let available_width = area.width.saturating_sub(4);
    let width_limit = if available_width == 0 {
        area.width
    } else {
        available_width
    };
    let width = width_limit.min(96);

    let available_height = area.height.saturating_sub(4);
    let height_limit = if available_height == 0 {
        area.height
    } else {
        available_height
    };
    let wanted_height = (picker.rows.len() as u16).saturating_add(4);
    let height = wanted_height.min(height_limit).max(1);
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(" fork at... ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    picker.set_height(rows[0].height as usize);

    let selected = picker.selected;
    let lines: Vec<_> = picker
        .rows
        .iter()
        .enumerate()
        .skip(picker.scroll_offset)
        .take(picker.height)
        .map(|(idx, row)| {
            let marker = if idx == selected { "› " } else { "  " };
            let style = if idx == selected {
                theme::selected()
            } else {
                theme::text_style()
            };
            Line::from(vec![
                Span::styled(marker, theme::accent()),
                Span::raw(format!("line {:<4} ", row.cut_index)),
                Span::raw(row.label.clone()),
            ])
            .style(style)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows[0]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "enter fork   esc cancel   ↑/↓ select",
            theme::dim(),
        )))
        .alignment(Alignment::Center),
        rows[1],
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn relative_time(time: DateTime<Utc>) -> String {
    let delta = Utc::now().signed_duration_since(time);
    if delta.num_minutes() < 1 {
        "now".to_owned()
    } else if delta.num_hours() < 1 {
        format!("{}m", delta.num_minutes())
    } else if delta.num_days() < 1 {
        format!("{}h", delta.num_hours())
    } else {
        format!("{}d", delta.num_days())
    }
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    let mut out: String = value.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
