# Plan: 24h "sessions started" sparkline strip

## Goal
One-row visual at the top of the TUI showing how many sessions were started in each of the last 24 hours. Aggregates across the **filtered** session set so it reacts to chips/search.

## Data
- Source: `Session.started_at: DateTime<Utc>` over `app.filtered_indices` (so the spark reflects what the user is currently looking at).
- Buckets: 24 hourly bins, bin `i` = `[now - (24-i)h, now - (23-i)h)`. Last bucket = current partial hour.
- Values: `Vec<u32>`, count of sessions whose `started_at` falls in that bin.

## Rendering
- Hand-drawn with block chars `▁▂▃▄▅▆▇█` (avoids relying on ratatui's `Sparkline` widget, which isn't guaranteed under `default-features = false`).
- Scale: `level = round((count / max) * 7)`, then index into the block char array; zero counts render as a dim `·`.
- Single row layout: `last 24h  ▁▁▂▁▃▂▅▄▃▆█▇▅▃▂▁▁▁▂▄▇▆▃·   N starts`
- Color: blocks use accent color, label dim, count muted.
- Hidden when `filtered_count < 2` (no signal worth showing) — same condition as chips visibility logic.

## Files
- New: `src/ui/sparkline.rs` — pure `hourly_start_buckets(sessions, now, hours) -> Vec<u32>` + `render_strip(frame, area, buckets, total)`.
- Edit: `src/ui/mod.rs` — add `pub(super) mod sparkline;`.
- Edit: `src/ui/render.rs` — insert a 1-row constraint above the input row when spark is shown; call `sparkline::render_strip`.
- Edit: `src/ui/theme.rs` — small `spark()` style.

## Tests
- `hourly_start_buckets` — empty, all-in-current-hour, exactly bucketed times, out-of-range filtered out.

## Non-goals
- Per-session timeline (data not available).
- Multi-day mode (start with 24h; the same primitive supports 30d/N later).
- Interactive hover.
