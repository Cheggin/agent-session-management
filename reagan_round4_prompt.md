# Round 4 dispatch (pre-staged)

Fire after round 3 (`baa4k0wt4`) lands and is verified. The user wants continuous load-time reduction; round 4 targets sub-10ms steady state and cold-start cuts.

## Scope

### A. Sub-10ms warm path
1. **Lazy DB open.** Today we open `~/.config/asm/index.db` on every startup. When the reindex-skip predicate fires (cache <5s old), we still open the DB to call `list_filtered`. Cache the most recent `list_filtered` result on disk as a serialized Vec<Session> (bincode) so the first paint can come from a single mmap read instead of SQLite open + query.
   - File: `~/.config/asm/latest.bin` (sessions sorted by `last_user_msg_at desc`, filtered to top 200).
   - Invalidate when reindex runs.
   - First paint comes from this file if it exists and is younger than 5s. Background promote to full SQLite session set after first paint.

2. **Direct crossterm first paint.** Skip ratatui's diff machinery for the first frame. Build the initial frame as raw ANSI strings, write to stdout in one `write_all`, then hand off to ratatui for subsequent frames. Pattern from ratatui issue #283 / `ratatui-async` examples.

3. **Don't enter alternate screen for --benchmark.** Currently --benchmark enters alternate screen, draws one frame, leaves alternate screen. The enter/leave each costs 1-3ms on macOS terminals. Single-frame benchmarks can render straight to the default screen.

### B. Cold start
4. **PGO (profile-guided optimization).** Add a `profile.release-pgo` profile and a `task pgo-build` that:
   - Builds with `RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data"`
   - Runs `target/release/asm --benchmark` 50 times to gather profile data
   - Builds with `RUSTFLAGS="-Cprofile-use=/tmp/pgo-data"`
   - Typically 5-15% startup win on instruction-heavy paths.
   - Make it a separate task, not the default, so dev iteration stays fast.

5. **Trim the dependency tree.** Audit and replace where possible:
   - `tracing-subscriber` pulls `regex` and `matchers` — heavy. Switch to a manual writer that just formats JSON lines for our log file.
   - `tracing-appender` keeps a writer thread alive. We don't need rolling — just an append file. Replace with a simple Mutex<File>.
   - `clap` derive is solid but consider `pico-args` for binaries where startup matters; clap adds ~500KB.

### Measurement protocol

For each fix, before AND after:
1. `cargo build --release`
2. `for i in {1..10}; do /usr/bin/time -p target/release/asm --benchmark; done | grep -E "benchmark|real"`
3. Report 10-run median wall-clock and median `asm benchmark:` internal time.
4. Note any regressions in tests (`cargo test`) or warnings (`cargo clippy --all-targets -- -D warnings`).

Append `## Round 4: sub-10ms warm + cold cuts` to `reagan_perf_report.md` with:
- Per-fix before/after timings (10-run median).
- Total round-4 improvement.
- Any fix that didn't move the needle — drop it and document why.
- PGO build time (it's slow; track it).

## Constraints
- Must not regress correctness of the SQL pushdown round 2 nor the lazy bodies / deferred reindex from round 1.
- Must not regress the round 3 responsive preview or chip rendering.
- mimalloc stays as global allocator.
- Lazy-snapshot file must be invalidated atomically when reindex runs — readers should never see torn data.
- `task bench` keeps working.
