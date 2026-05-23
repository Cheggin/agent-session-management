# Step 7 Report

Step 7 added the fork backend, the `ctrl-f` fork picker overlay, and the non-interactive `asm fork <session-id> [--at <n>] [--no-resume]` command without touching export, liveness, FTS, or shell hooks. Test debt was handled first: 4 coverage tests were added for filter application, Codex index round-trip, same-path mtime upsert idempotence, and incremental reindex skip/reparse behavior. Forking added 2 provider tests, bringing the integration suite to 20 tests total.

`src/fork.rs` is 362 LOC. It uses injectable `ForkRoots` for tests, UUIDv4 Claude ids, UUIDv7 Codex ids, Claude cwd encoding, Codex date-partitioned rollout paths, source turn extraction, cut-index validation, provider-specific writers, and shared resume dispatch unless `--no-resume` is set. `src/ui/fork_picker.rs` is 184 LOC and renders the end-of-session default plus latest-to-earliest user turns with relative timestamps and truncated prompt labels.

Validation passed:
- `cargo build --release`
- `cargo test` (20 passed)
- `cargo clippy --all-targets -- -D warnings`

Fork tests passed:
- `claude_fork_at_end_writes_to_current_cwd_project_dir`
- `codex_fork_at_end_rewrites_session_meta`

Format notes: Claude lines can be copied verbatim with only the plain `sessionId` field rewritten by regex, so embedded escaped JSON remains untouched. Codex resume safety is better if the source `session_meta.payload` is cloned and only identity, cwd, timestamp, and git are replaced; non-meta events are copied verbatim so turn ids and call ids remain source-history-internal as required. If the current cwd is not a git repo, the Codex fork removes the old `git` payload instead of carrying stale source repo metadata forward.
