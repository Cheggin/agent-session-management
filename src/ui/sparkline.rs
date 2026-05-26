//! 24-hour "sessions started" sparkline strip.
//!
//! Bucketing is pure and lives in `hourly_start_buckets` so it can be unit-
//! tested without a terminal. Rendering reads from the bucket vec and draws a
//! one-row strip using unicode block characters (no dependency on ratatui's
//! `Sparkline` widget, which is gated behind features we don't enable).

use chrono::{DateTime, Duration, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::Session;

use super::theme;

const BLOCKS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];
const EMPTY: char = '\u{00B7}';

/// Count sessions whose `started_at` falls in each of the last `hours` hourly
/// bins ending at `now`. Bucket 0 is the oldest, bucket `hours-1` is the
/// current (partial) hour.
pub fn hourly_start_buckets<'a>(
    sessions: impl IntoIterator<Item = &'a Session>,
    now: DateTime<Utc>,
    hours: usize,
) -> Vec<u32> {
    let mut buckets = vec![0u32; hours];
    if hours == 0 {
        return buckets;
    }
    let window_start = now - Duration::hours(hours as i64);
    for session in sessions {
        let t = session.started_at;
        if t < window_start || t > now {
            continue;
        }
        let minutes_ago = (now - t).num_minutes();
        let hours_ago = (minutes_ago / 60) as usize;
        if hours_ago >= hours {
            continue;
        }
        let bin = hours - 1 - hours_ago;
        buckets[bin] = buckets[bin].saturating_add(1);
    }
    buckets
}

pub fn render_strip(frame: &mut Frame<'_>, area: Rect, buckets: &[u32]) {
    if area.width == 0 || buckets.is_empty() {
        return;
    }
    let label = "last 24h ";
    let total: u32 = buckets.iter().sum();
    let suffix = format!(" {total} starts");
    let label_w = label.chars().count();
    let suffix_w = suffix.chars().count();
    let avail = (area.width as usize).saturating_sub(label_w + suffix_w);
    if avail == 0 {
        return;
    }

    let cells = render_cells(buckets, avail);

    let mut spans = Vec::with_capacity(cells.len() + 2);
    spans.push(Span::styled(label.to_owned(), theme::dim()));
    for ch in cells {
        let style = if ch == EMPTY {
            theme::dim()
        } else {
            theme::spark()
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    spans.push(Span::styled(suffix, theme::muted()));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_cells(buckets: &[u32], width: usize) -> Vec<char> {
    if width == 0 {
        return Vec::new();
    }
    // If buckets fit, render 1:1. Otherwise compress by averaging adjacent
    // buckets into `width` cells.
    let resampled: Vec<f32> = if buckets.len() <= width {
        buckets.iter().map(|v| *v as f32).collect()
    } else {
        let mut out = vec![0f32; width];
        for (i, v) in buckets.iter().enumerate() {
            let target = (i * width) / buckets.len();
            out[target] += *v as f32;
        }
        out
    };

    let max = resampled.iter().copied().fold(0f32, f32::max);
    let mut cells = Vec::with_capacity(resampled.len());
    for v in resampled {
        if v <= 0.0 || max <= 0.0 {
            cells.push(EMPTY);
            continue;
        }
        let ratio = (v / max).clamp(0.0, 1.0);
        let level = ((ratio * (BLOCKS.len() as f32 - 1.0)).round() as usize).min(BLOCKS.len() - 1);
        cells.push(BLOCKS[level]);
    }
    cells
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Agent;

    fn session_started(at: DateTime<Utc>) -> Session {
        Session {
            id: "x".into(),
            agent: Agent::Claude,
            path: PathBuf::from("/tmp/x"),
            cwd: None,
            git_branch: None,
            entrypoint: None,
            title: None,
            first_user_prompt: None,
            recent_user_prompts: Vec::new(),
            last_assistant_text: None,
            started_at: at,
            last_user_msg_at: None,
            last_assistant_msg_at: None,
            user_msg_count: 0,
            is_live: false,
            maybe_live: false,
            is_sidechain: false,
        }
    }

    #[test]
    fn empty_input_yields_zero_buckets() {
        let now = Utc::now();
        let buckets = hourly_start_buckets(std::iter::empty(), now, 24);
        assert_eq!(buckets.len(), 24);
        assert!(buckets.iter().all(|v| *v == 0));
    }

    #[test]
    fn current_hour_lands_in_last_bin() {
        let now = Utc::now();
        let s = session_started(now - Duration::minutes(5));
        let buckets = hourly_start_buckets([&s], now, 24);
        assert_eq!(buckets[23], 1);
        assert_eq!(buckets[..23].iter().sum::<u32>(), 0);
    }

    #[test]
    fn three_hours_ago_lands_in_correct_bin() {
        let now = Utc::now();
        let s = session_started(now - Duration::hours(3) - Duration::minutes(1));
        let buckets = hourly_start_buckets([&s], now, 24);
        // 3h1m ago -> hours_ago = 3 -> bin = 24-1-3 = 20
        assert_eq!(buckets[20], 1);
    }

    #[test]
    fn out_of_window_excluded() {
        let now = Utc::now();
        let old = session_started(now - Duration::hours(30));
        let future = session_started(now + Duration::hours(1));
        let buckets = hourly_start_buckets([&old, &future], now, 24);
        assert!(buckets.iter().all(|v| *v == 0));
    }

    #[test]
    fn multiple_in_same_bin_accumulate() {
        let now = Utc::now();
        let a = session_started(now - Duration::minutes(10));
        let b = session_started(now - Duration::minutes(20));
        let buckets = hourly_start_buckets([&a, &b], now, 24);
        assert_eq!(buckets[23], 2);
    }
}
