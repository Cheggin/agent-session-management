# Step 10/11 Report — FTS Body Search + Shell Hooks

Implemented Step 10 and Step 11 together.

## Part A — FTS/message body search

- Added `messages_fts` as an FTS5 virtual table with `session_id UNINDEXED`, `body`, and `tokenize='porter unicode61'`.
- Kept `rusqlite` on `features = ["bundled"]`; the build and tests verify bundled SQLite includes FTS5 on this machine.
- Added `ClaudeParser::extract_message_bodies(path)` and `CodexParser::extract_message_bodies(path)` without expanding `Session`.
  - Claude extraction indexes all non-meta user text bodies plus assistant text blocks, in file order.
  - Codex extraction indexes `event_msg/user_message`, `event_msg/agent_message`, and `event_msg/task_complete.last_agent_message`, in file order.
- `Index::upsert_session` / `upsert_sessions` now replace message FTS rows in the same SQLite transaction as the session upsert.
- Incremental reindex now reparses unchanged files that were indexed before the FTS table existed, so old indexes get backfilled instead of leaving `messages_fts` sparse.
- Added `Index::fts_match(pattern)` for FTS tests and `Index::dump_message_bodies()` for the TUI's simple in-memory `@find` filter path.
- Added `Chip::Find(String)` with `@find:<single-token>` parsing. Multi-word find queries are intentionally not supported in v1 because filter text is whitespace-tokenized.
- The TUI preloads `session_id -> concatenated bodies` after reindex and applies `@find` as a case-insensitive substring filter in memory.

Real index FTS size after backfill reindex:

```text
messages_fts rows: 25,634
messages_fts distinct sessions: 1,288
approx body text stored: 14.11 MiB
sessions table rows: 1,569
raw FTS MATCH 'storage': 343 sessions
```

Manual filter check:

```text
./target/release/asm --filter "find:storage"
```

A PTY smoke captured the TUI rendering the seeded `@find:storage` chip and narrowing the visible list to `207 sessions` after the inline reindex. The lower visible count is expected because the UI still applies the default sidechain and empty-user-session filters.

## Part B — `asm install` / `asm uninstall`

- Added `src/shell.rs` with zsh hook install/uninstall helpers and testable temp-home variants.
- Wired CLI commands:
  - `asm install`
  - `asm uninstall`
- CLI install/uninstall check `$SHELL` and exit 1 with `asm install currently supports zsh only; your shell is X` when the active shell is not zsh.
- `asm install` writes `~/.asm/shell-hooks.zsh` and appends the marker/source lines to `~/.zshrc` only when the source line is absent.
- `asm uninstall` removes the marker/source lines and deletes `~/.asm/shell-hooks.zsh` if present.
- Shell hook functions use `exec asm --filter claude` and `exec asm --filter codex`, so resume interception replaces the shell process rather than spawning asm as a child.
- Validation did not touch the real `~/.zshrc`; tests use temp homes only.
- Idempotence confirmed by tests: running install twice keeps exactly one source line, and running uninstall twice succeeds with the hook absent and no source line remaining.

## Tests and validation

- `cargo build --release` passed.
- `cargo test` passed: 29 integration tests total.
  - Prior tests: 23 still passing.
  - New FTS tests: 3 passing.
  - New shell tests: 3 passing.
- `cargo clippy --all-targets -- -D warnings` passed.
- Real reindex after FTS backfill passed with one known pre-existing unparsable Claude session file; second run returned to incremental behavior (`parsed=1`, `skipped_unchanged=1568`, `failed=1`).

## Files touched

- `src/claude.rs`
- `src/codex.rs`
- `src/db.rs`
- `src/reindex.rs`
- `src/ui/app.rs`
- `src/ui/filter.rs`
- `src/ui/mod.rs`
- `src/ui/render.rs`
- `src/shell.rs`
- `src/lib.rs`
- `src/main.rs`
- `src/fork.rs` (stabilized Codex fork timestamp precision to avoid millisecond-truncation test flake)
- `tests/fts.rs`
- `tests/shell.rs`
- `tests/filter.rs`
- `tests/index.rs`

## Remaining risks / limitations

- `@find` supports only one whitespace-free token in v1 (`@find:storage`, not `@find:cloud storage`).
- `Index::fts_match` accepts raw FTS5 query syntax and is covered for simple single-word patterns; UI filtering intentionally uses in-memory substring matching instead.

## Followups (2026-05-23)

- Removed the user-facing `@find` chip. Plain search now fuzzy-matches title, first user prompt, cwd, git branch, and precomputed concatenated message-body haystacks.
- Removed remaining non-report `was_compacted` references from the planning/schema docs.
- Added a root `Taskfile.yml` with build, test, watch, lint, format, run, reindex, install, shell hook, and clean tasks.
- Final `cargo test` count: 30 tests passed.
