use std::path::Path;

use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Position, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
};

use crate::{Agent, Session};

use super::{
    app::App,
    filter::{Chip, chip_label},
    fork_picker, sparkline, theme,
};

const SPARK_HOURS: usize = 24;
const SPARK_MIN_SESSIONS: usize = 2;

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let full = frame.area();
    let area = full.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let show_chips = !app.chips.is_empty() || app.toast_text().is_some();
    let spark_buckets = if app.filtered_count() >= SPARK_MIN_SESSIONS {
        let b = sparkline::hourly_start_buckets(app.filtered_sessions(), Utc::now(), SPARK_HOURS);
        (b.iter().sum::<u32>() > 0).then_some(b)
    } else {
        None
    };
    let show_spark = spark_buckets.is_some();

    let mut constraints = Vec::new();
    if show_chips {
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));
    }
    if show_spark {
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));
    }
    constraints.extend([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut row_idx = 0;
    if show_chips {
        render_chip_strip(frame, rows[row_idx], app);
        row_idx += 2;
    }
    if let Some(buckets) = spark_buckets.as_ref() {
        sparkline::render_strip(frame, rows[row_idx], buckets);
        row_idx += 2;
    }
    render_input(frame, rows[row_idx], app);
    row_idx += 2;
    render_middle(frame, rows[row_idx], app);
    row_idx += 2;
    render_hint(frame, rows[row_idx]);

    if let Some(picker) = app.fork_picker_mut() {
        fork_picker::render_overlay(frame, full, picker);
    }
}

fn render_chip_strip(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let mut spans = Vec::new();
    let mut used = 0usize;

    if let Some(toast) = app.toast_text() {
        push_text(&mut spans, &mut used, " ", theme::text_style());
        push_text(&mut spans, &mut used, toast, theme::toast());
        push_text(&mut spans, &mut used, "  ", theme::text_style());
    }

    for chip in &app.chips {
        push_text(&mut spans, &mut used, " ", theme::text_style());
        push_text(&mut spans, &mut used, &chip_label(chip), chip_style(chip));
    }

    let count = format!("{} sessions", app.filtered_count());
    let spacer = (area.width as usize).saturating_sub(used + count.chars().count());
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.push(Span::styled(count, theme::dim()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_input(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let prefix = "search › ";
    let line = if app.composer.is_empty() {
        Line::from(vec![
            Span::styled(prefix, theme::accent()),
            Span::styled("type to search, @tag to filter", theme::dim()),
        ])
    } else {
        Line::from(vec![
            Span::styled(prefix, theme::accent()),
            Span::raw(app.composer.input().to_owned()),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);

    if area.width > 0 {
        let cursor_x = area
            .x
            .saturating_add((prefix.chars().count() + app.composer.cursor()) as u16)
            .min(area.x.saturating_add(area.width.saturating_sub(1)));
        frame.set_cursor_position(Position {
            x: cursor_x,
            y: area.y,
        });
    }
}

fn render_middle(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    render_list(frame, area, app);
}

fn render_list(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .padding(Padding::new(2, 2, 1, 1))
        .title(" sessions ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.set_list_height(inner.height as usize);

    if app.filtered_indices.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("No sessions match.", theme::dim()))),
            inner,
        );
        return;
    }

    let selected_session_idx = app.filtered_indices.get(app.selected).copied();
    let lines: Vec<_> = app
        .visible_indices()
        .map(|(idx, session)| {
            let mut line = list_row(session, inner.width as usize);
            if Some(idx) == selected_session_idx {
                line = line.style(theme::selected());
            }
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_hint(frame: &mut Frame<'_>, area: Rect) {
    let hint = "enter resume   ctrl-f fork   ctrl-e export   ↑/↓ select   pgup/pgdn page   ctrl-r/F5 reindex   esc/ctrl-c quit";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, theme::dim()))),
        area,
    );
}

fn list_row(session: &Session, width: usize) -> Line<'static> {
    let live = if session.is_live { "●" } else { " " };
    let agent = agent_name(&session.agent);
    let relative = relative_time(activity_time(session));
    let cwd_tail = session.cwd.as_deref().map(cwd_tail).unwrap_or_default();
    let branch = session.git_branch.as_deref().unwrap_or_default();

    let cwd_w = width.saturating_sub(48).min(22);
    let branch_w = width.saturating_sub(62).min(22);
    let fixed = 2 + 8 + 5 + cwd_w + branch_w + 6;
    let title_w = width.saturating_sub(fixed).max(8);
    let title = truncate(&session_title(session), title_w);

    Line::from(vec![
        Span::styled(
            format!("{live} "),
            if session.is_live {
                theme::live()
            } else {
                theme::dim()
            },
        ),
        Span::styled(format!("{agent:<6}"), theme::agent(&session.agent)),
        Span::raw("  "),
        Span::styled(format!("{:>4}", truncate(&relative, 4)), theme::muted()),
        Span::raw("  "),
        Span::raw(format!("{title:<title_w$}")),
        Span::raw("  "),
        Span::styled(
            format!("{:<cwd_w$}", truncate(&cwd_tail, cwd_w)),
            theme::dim(),
        ),
        Span::raw("  "),
        Span::styled(truncate(branch, branch_w), theme::branch()),
    ])
}

fn push_text(
    spans: &mut Vec<Span<'static>>,
    used: &mut usize,
    text: &str,
    style: ratatui::style::Style,
) {
    *used += text.chars().count();
    spans.push(Span::styled(text.to_owned(), style));
}

fn chip_style(chip: &Chip) -> ratatui::style::Style {
    match chip {
        Chip::Agent(agent) => theme::agent(agent),
        Chip::Running => theme::live(),
        Chip::Branch(_) => theme::branch(),
        _ => theme::chip(),
    }
}

fn agent_name(agent: &Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}

fn cwd_tail(path: &Path) -> String {
    let parts: Vec<_> = path
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .filter(|part| !part.is_empty() && *part != "/")
        .collect();
    let start = parts.len().saturating_sub(2);
    parts[start..].join("/")
}

fn session_title(session: &Session) -> String {
    session
        .title
        .as_deref()
        .or(session.first_user_prompt.as_deref())
        .unwrap_or(&session.id)
        .to_owned()
}

fn activity_time(session: &Session) -> DateTime<Utc> {
    session.last_user_msg_at.unwrap_or(session.started_at)
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
    if width == 0 {
        return String::new();
    }
    let len = value.chars().count();
    if len <= width {
        return value.to_owned();
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut out: String = value.chars().take(width - 1).collect();
    out.push('…');
    out
}
