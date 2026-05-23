# asm startup performance report

Date: 2026-05-23
Binary: `target/release/asm`
Method: pseudo-TTY launch, wait for `asm.startup` in `~/.config/asm/asm.log`, then send Ctrl-C. Cold runs removed `~/.config/asm/index.db*` first.

## Phase A baseline timings

### Cold baseline, before optimization

| phase | elapsed ms | notes |
| --- | ---: | --- |
| `asm.startup` | 6080 | first frame drawn |
| `asm.tracing_init` | 1 | file tracing setup |
| `asm.db.open` | 2 | open + migrate |
| `asm.reindex.discover` | 3 | 1488 files discovered |
| `asm.reindex.filter` | 7 | extra instrumentation: stat + cache checks; 1488 changed |
| `asm.reindex.parse` | 278 | rayon parse; 1488 candidates |
| `asm.reindex.write` | 5751 | SQLite transaction + FTS population; dominant cold phase |
| `asm.db.list_all` | 5 | load sessions from cache |
| `asm.db.dump_bodies` | 9 | eager FTS body dump |
| `asm.app.new` | 17 | haystacks + initial filters |
| `asm.first_render` | 0 | event loop start to first draw return |

### Warm baseline, before optimization

| phase | elapsed ms | notes |
| --- | ---: | --- |
| `asm.startup` | 2094 | first frame drawn |
| `asm.tracing_init` | 0 | file tracing setup |
| `asm.db.open` | 0 | open + migrate |
| `asm.reindex.discover` | 3 | 1488 files discovered |
| `asm.reindex.filter` | 2045 | extra instrumentation: per-file mtime + DB cache checks; dominant warm phase |
| `asm.reindex.parse` | 1 | 2 candidates |
| `asm.reindex.write` | 9 | 1 parsed session |
| `asm.db.list_all` | 5 | load sessions from cache |
| `asm.db.dump_bodies` | 9 | eager FTS body dump |
| `asm.app.new` | 18 | haystacks + initial filters |
| `asm.first_render` | 0 | event loop start to first draw return |

Baseline finding: warm startup was dominated by the reindex unchanged-file check loop, not by `dump_message_bodies()` on this machine. Cold startup was dominated by FTS write population.

## Fixes applied

1. Lazy message bodies:
   - Removed startup `Index::dump_message_bodies()`.
   - `App` now starts with `Option<Arc<HashMap<String, String>>> = None` for message bodies.
   - Startup haystacks are cheap: title, first prompt, cwd, branch.
   - First non-empty search lazily loads bodies on the main thread and rebuilds haystacks.
   - If bodies are already loaded, Ctrl-R refresh reloads them; otherwise Ctrl-R keeps startup/search memory cheap.

2. Recent reindex skip:
   - Added `meta(last_reindex_at_secs)` to the SQLite index.
   - Full reindex records `last_reindex_at_secs` after successful write.
   - Startup skips reindex when cache age is under 5 seconds and logs `reindex skipped`.
   - Ctrl-R still forces `reindex_all()`.
   - First-ever run with no meta row still does a full foreground reindex.

3. Deferred stale-cache reindex:
   - If the cache exists but is older than 5 seconds, startup now draws from the existing index immediately.
   - Reindex starts in a background thread after the first frame is drawn.
   - The TUI shows `updating index...` and refreshes sessions when the background reindex completes.

## After timings

### Cold after optimization

| phase | elapsed ms | notes |
| --- | ---: | --- |
| `asm.startup` | 6475 | first frame drawn; cold still foreground reindexes as required |
| `asm.tracing_init` | 0 | file tracing setup |
| `asm.db.open` | 2 | open + migrate |
| `asm.reindex.discover` | 3 | 1488 files discovered |
| `asm.reindex.filter` | 7 | 1488 changed |
| `asm.reindex.parse` | 347 | rayon parse; 1488 candidates |
| `asm.reindex.write` | 6090 | SQLite transaction + FTS population; dominant cold phase |
| `asm.db.list_all` | 4 | load sessions from cache |
| `asm.db.dump_bodies` | N/A | removed from startup |
| `asm.app.new` | 15 | cheap haystacks + initial filters |
| `asm.first_render` | 0 | event loop start to first draw return |

Cold was within run-to-run variance and remains dominated by mandatory first-run FTS writes. It is not structurally slower; the eager body dump is gone.

### Warm after optimization: fresh cache (`last_reindex_at_secs` age 0s)

| phase | elapsed ms | notes |
| --- | ---: | --- |
| `asm.startup` | 23 | first frame drawn |
| `asm.tracing_init` | 0 | file tracing setup |
| `asm.db.open` | 0 | open + migrate |
| reindex | skipped | `cache_age_secs=0 threshold_secs=5` |
| `asm.db.list_all` | 5 | load sessions from cache |
| `asm.db.dump_bodies` | N/A | lazy only |
| `asm.app.new` | 16 | cheap haystacks + initial filters |
| `asm.first_render` | 0 | event loop start to first draw return |

### Warm after optimization: stale cache (`last_reindex_at_secs` age 6s)

| phase | elapsed ms | notes |
| --- | ---: | --- |
| `asm.startup` | 24 | first frame drawn |
| `asm.tracing_init` | 0 | file tracing setup |
| `asm.db.open` | 0 | open + migrate |
| reindex | deferred | `cache_age_secs=6 threshold_secs=5` |
| `asm.db.list_all` | 5 | load sessions from cache |
| `asm.db.dump_bodies` | N/A | lazy only |
| `asm.app.new` | 17 | cheap haystacks + initial filters |
| `asm.first_render` | 0 | event loop start to first draw return |

Background verification run showed the deferred worker then ran the old warm-heavy work after first paint: discover 3ms, filter 2182ms, parse 1ms, write 9ms, then refreshed `list_all` in 5ms.

## Target result

Warm startup before: 2094ms.
Warm startup after: 24ms with stale-cache deferred reindex, 23ms with fresh-cache skip.
Improvement: ~98.9% faster warm first paint, so the 50% target was hit.

## Codex resume picker comparison

Codex's resume picker does not eagerly build a whole full-text in-memory search index before first paint. In current `openai/codex`, the picker uses cursor-based pagination with a 25-row page size, asks the app-server `thread/list` backend for sorted/filterable pages, and loads transcript previews/full transcripts on demand. The useful lesson for `asm` is the same shape landed here: first paint should use cheap indexed metadata, while expensive transcript/body work happens lazily or after first render.

Sources inspected:
- https://github.com/openai/codex/blob/main/codex-rs/tui/src/resume_picker.rs
- https://github.com/openai/codex/blob/main/codex-rs/core/src/rollout.rs

## Validation

- `cargo build --release` passed.
- `cargo test` passed: 50 tests.
- `cargo clippy --all-targets -- -D warnings` passed.
- Manual pseudo-TTY timing runs wrote structured spans to `~/.config/asm/asm.log`.

## Remaining risks

- Background reindex uses a second SQLite connection. The verification run completed cleanly, but very large future indexes could still benefit from a dedicated progress indicator or cancellation.
- Search body load is intentionally simple and synchronous on first non-empty search. If real body dumps become much larger than observed here, move only that load to a background worker.

## Round 2: SQL pushdown

Date: 2026-05-23
Method: release build once, then run `/usr/bin/time -p target/release/asm --benchmark` five times before and after moving the default/drop and cheap chip predicates into SQLite. The `--benchmark` path performs normal TUI startup through the first render, restores the terminal when attached to a TTY, prints `asm benchmark: <ms>ms`, and exits. These runs used the prebuilt `target/release/asm` binary rather than `cargo run`.

### End-to-end wall-clock timings

| state | command | `asm benchmark` runs (ms) | median `asm benchmark` | `/usr/bin/time real` runs (s) | median `real` |
| --- | --- | ---: | ---: | ---: | ---: |
| Before SQL pushdown | `/usr/bin/time -p target/release/asm --benchmark` | 25, 22, 23, 24, 22 | 23ms | 0.38, 0.02, 0.02, 0.02, 0.02 | 0.02s |
| After SQL pushdown | `/usr/bin/time -p target/release/asm --benchmark` | 21, 17, 19, 17, 19 | 19ms | 0.41, 0.02, 0.02, 0.02, 0.02 | 0.02s |

The median full-process wall clock stayed at the coarse `/usr/bin/time` floor of ~20ms, while the benchmark's process-internal first-render wall clock improved from 23ms to 19ms on this small local index/window. The main expected win is less row materialization for default `@here` and other cheap chips as the index grows.

### Changes

- Added `asm --benchmark`, which starts the TUI, performs exactly one draw, prints the wall-clock duration, and exits without requiring interactive input.
- Added `task bench`; it builds `target/release/asm`, uses `hyperfine` when present, and falls back to `/usr/bin/time -p target/release/asm --benchmark` when absent.
- Added `Index::list_filtered(&[Chip])`, pushing these predicates into SQLite: `@here`, `@claude`/`@codex`, `@branch:<name>`, and `@path:<substring>`.
- Moved default exclusions into the SQL filtered path: sidechains, zero-user-message sessions, and SDK/exec entrypoints are omitted before rows are materialized.
- Kept recency/running/find behavior in memory; fuzzy search still runs against the smaller filtered set.
- Re-query is lazy on pushdown-chip removal so broadening a filtered list loads the newly eligible rows with the remaining pushdown chips.

### Surprises

- The first `/usr/bin/time` run in both before/after sets was a large outlier (`0.38s` and `0.41s`) even though the benchmark-reported first-render time was only `25ms`/`21ms`. Repeated runs settled at `0.02s`, so the reported wall-clock comparison uses the requested median of five.
- `time task up -- --benchmark` completed and printed a sane benchmark (`asm benchmark: 26ms`), but its full wall time was `0.71s` because `task up` still shells through `cargo run --release`; the measurement table intentionally uses the prebuilt binary to avoid cargo overhead.

### Validation

- `cargo build --release` passed.
- `cargo test` passed: 55 tests, including 5 new `tests/sql_pushdown.rs` tests.
- `cargo clippy --all-targets -- -D warnings` passed.
- `task bench` passed via `/usr/bin/time` fallback on this machine.
- `/usr/bin/time -p task up -- --benchmark` passed and printed `asm benchmark: 26ms`.
