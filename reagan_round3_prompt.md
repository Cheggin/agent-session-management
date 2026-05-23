# Round 3 dispatch (pre-staged)

Fire immediately after round 2 (b3141yly6) lands and is verified.

## Scope

Two parallel concerns, kept in one round for atomicity:

### A. Build-profile + allocator perf
Cold start is dominated by FTS write (~6s on first reindex). Warm start has process spawn / dynlinker / ratatui init overhead beyond what tracing captures.

1. Cargo.toml release profile:
   ```toml
   [profile.release]
   lto = "fat"
   codegen-units = 1
   panic = "abort"
   strip = true
   ```
2. Add `mimalloc = { version = "0.1", default-features = false }`, set as global allocator in `src/main.rs`:
   ```rust
   #[global_allocator]
   static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
   ```
3. Defer FTS population:
   - During reindex, write the `sessions` table rows immediately.
   - Push `messages_fts` writes onto a separate background queue that runs after first paint.
   - If user types a search before FTS is fully populated, fall back to in-memory haystack match for that turn; show a tiny "indexing search content…" indicator.
   - On Ctrl-R, wait for the FTS queue to drain before reporting "reindex complete".

### B. Responsive preview + inline chip rework
4. Bring back a **minimal** preview pane: only the last 1-3 user messages, nothing else. Source from a new `Session.recent_user_prompts: Vec<String>` (last 3) populated by parsers. No assistant responses, no tokens, no metadata.
5. **Hide preview on narrow terminals** — when `terminal_width < 100` cols or `terminal_height < 20` rows, render list full-width with no preview pane. Match the responsive pattern in `/Users/reagan/.superset/projects/browser-use-terminal/crates/browser-use-tui/src/render.rs` (look for `Constraint::Percentage` + small-screen branches).
6. **Inline chips** — render chips as colored badges inline with the search input, before the cursor (Slack/Gmail style), not above it.
7. **Two-tap delete on chips** — backspace at cursor-start: first press highlights the rightmost chip (inverted colors), second press deletes it. Any other key cancels the highlight.
8. **`cmd+backspace` (delete word backward) must NOT touch chips** — only delete typed text.

## Validation (must pass)

- `cargo build --release` succeeds.
- `cargo test` passes — all prior + new responsive-preview, inline-chip, two-tap-delete tests.
- `cargo clippy --all-targets -- -D warnings` clean.
- `task bench` (or `time target/release/asm --benchmark`) wall-clock end-to-end shows improvement vs round 2.
- Manual: run `asm` in a narrow tmux pane (e.g. 80x24) — confirm no preview. Resize to 120 cols, preview appears.

## Reporting

Append `## Round 3: build profile + lazy FTS + responsive preview` to `reagan_perf_report.md` with before/after `task bench` numbers and any unexpected regressions.

## Constraints

- Do not regress the warm startup time established in round 2.
- Do not break SQL pushdown for @here / @claude / @codex / @branch.
- Do not break chip excision, @here default, --global, lazy bodies, deferred reindex.
- Recent user prompts must be populated at parse time, not lazily — they're cheap (last 3 strings).
- Inline-chip rendering must keep the existing tests (chip parsing, removal) green.
