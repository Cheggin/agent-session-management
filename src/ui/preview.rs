use chrono::{DateTime, Utc};
use ratatui::text::{Line, Span};

use crate::{Agent, Session};

use super::theme;

pub fn lines(session: &Session, width: usize) -> Vec<Line<'static>> {
    let width = width.max(20);
    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        session_title(session),
        theme::bold(),
    )));
    out.push(Line::from(where_line(session)));
    out.push(Line::from(format!(
        "started: {}     last activity: {} (you: {}, it: {})",
        absolute_time(session.started_at),
        relative_time(activity_time(session)),
        option_relative(session.last_user_msg_at),
        option_relative(session.last_assistant_msg_at),
    )));
    out.push(Line::from(""));
    push_wrapped_prompt(
        &mut out,
        "you: ",
        session.first_user_prompt.as_deref().unwrap_or(""),
        width,
        6,
    );
    out.push(Line::from(""));
    push_wrapped_prompt(
        &mut out,
        "it:  ",
        session.last_assistant_text.as_deref().unwrap_or(""),
        width,
        6,
    );
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        session.path.to_string_lossy().into_owned(),
        theme::dim(),
    )));
    out
}

pub fn session_title(session: &Session) -> String {
    session
        .title
        .as_deref()
        .or(session.first_user_prompt.as_deref())
        .unwrap_or(&session.id)
        .to_owned()
}

pub fn relative_time(time: DateTime<Utc>) -> String {
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

pub fn activity_time(session: &Session) -> DateTime<Utc> {
    session.last_user_msg_at.unwrap_or(session.started_at)
}

fn where_line(session: &Session) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        agent_name(&session.agent),
        theme::agent(&session.agent),
    )];
    spans.push(Span::styled("  •  ", theme::dim()));
    spans.push(Span::raw(
        session
            .cwd
            .as_deref()
            .map(|cwd| cwd.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ));
    if let Some(branch) = &session.git_branch {
        spans.push(Span::styled("  •  ", theme::dim()));
        spans.push(Span::styled(branch.clone(), theme::branch()));
    }
    if session.is_live {
        spans.push(Span::styled("  •  ● live", theme::live()));
    }
    spans
}

fn push_wrapped_prompt(
    out: &mut Vec<Line<'static>>,
    label: &str,
    text: &str,
    width: usize,
    max_lines: usize,
) {
    let quote_budget = width.saturating_sub(label.chars().count() + 3).max(8);
    let (wrapped, truncated) = wrap_text(&text.replace('\n', " "), quote_budget, max_lines);
    if wrapped.is_empty() {
        out.push(Line::from(vec![
            Span::styled(label.to_owned(), theme::muted()),
            Span::raw("\"\""),
        ]));
        return;
    }
    let line_count = wrapped.len();
    for (idx, line) in wrapped.into_iter().enumerate() {
        let prefix = if idx == 0 {
            label.to_owned()
        } else {
            " ".repeat(label.chars().count())
        };
        let open = if idx == 0 { "\"" } else { " " };
        let close = if idx + 1 == line_count {
            if truncated { "…\"" } else { "\"" }
        } else {
            ""
        };
        out.push(Line::from(vec![
            Span::styled(prefix, theme::muted()),
            Span::raw(format!("{open}{line}{close}")),
        ]));
    }
}

fn wrap_text(text: &str, width: usize, max_lines: usize) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut truncated = false;
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_owned();
            if lines.len() == max_lines {
                truncated = true;
                return (lines, truncated);
            }
        }
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    (lines, truncated)
}

fn option_relative(time: Option<DateTime<Utc>>) -> String {
    time.map(relative_time).unwrap_or_else(|| "n/a".to_owned())
}

fn absolute_time(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M UTC").to_string()
}

fn agent_name(agent: &Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}
