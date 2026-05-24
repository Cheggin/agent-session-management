# Plan: measure + optimize resume/fork latency

## Phase 1 — Measure (this step)

Goal: get reproducible per-phase numbers for both code paths before touching
optimizations. Without baselines "50% faster" is meaningless.

### Add bench CLI commands
- `asm bench-fork --id <session-id> [--at <n>] [--repeat <n>]`
  - Looks up the session by id, runs `fork_session(..., no_resume=true)` and
    prints per-phase timing in microseconds. Repeats N times (default 5) to
    expose variance; reports min / median / mean.
  - Phases timed:
    - `read_jsonl` — `read_jsonl_lines`
    - `validate` — `validate_cut_index`
    - `transform_write` — `write_*_fork_from_lines`
    - (Codex only) `git_meta` — `current_git`
    - `total` — wall-clock for `fork_session`
- `asm bench-resume --id <session-id> [--repeat <n>]`
  - Reproduces everything the resume path does up to (but not including)
    `Command::exec`: index open + list_all lookup + Session clone + argv build.
    Reports per-phase timing.

### Why CLI commands (not interactive TUI testing)
- Reproducible without keyboard input.
- Avoids confounders from terminal teardown.
- Matches the pattern already used by `--benchmark` for startup.
- Can be wrapped in `hyperfine` for full wall-clock if needed.

### Real-keypress timing (sanity check)
Also add `tracing::info!` instrumentation in `ui/mod.rs` at the moment
KeyCode::Enter triggers Resume / Fork actions, and at the entry/exit of
`dispatch_resume` and `fork_session`. Logs go to `~/.config/asm/asm.log`, so
`task up` → press Enter → tail the log to see real keypress→exec time.

### Output
Run baseline locally on this repo's sessions (the 449-line fork-target one
from the prior memory is ideal). Record numbers in
`reagan_resume_fork_baseline.md` so we know what we're optimizing against.

## Phase 2 — Optimize (after baseline)

Will be planned once numbers are in. Likely candidates by inspection:
1. Static (Lazy) regex in `write_claude_fork_from_lines` — saves regex compile
   per call.
2. Single streaming pass for fork: read line → check first-user-turn-found →
   transform → write. Eliminates the validate-then-rewrite double parse.
3. For Codex: replace `current_git` (two `git` subprocesses) with direct reads
   of `.git/HEAD` (+ ref file). Subprocess fork+exec is ~5–20ms each.
4. For Resume: only thing to optimize is TUI teardown; expected wins <5ms.

Each will be applied only if its phase actually dominates the baseline.
