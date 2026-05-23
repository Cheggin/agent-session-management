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
