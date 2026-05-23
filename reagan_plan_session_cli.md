# asm — Agent Session Manager

## Goal
One CLI to list, search, resume, and fork Claude Code + Codex sessions. Fix the "which session is actually the latest / right one" problem.

## Decisions
- **Binary name:** `asm`.
- **Language:** Rust. Reuses patterns from `/github/browser-use-terminal`; serde + rusqlite + clap are best-in-class for this workload.
- **UI:** one interactive searchable list is the whole product. `asm` and `asm ls` both open it. Built on `ratatui` + `crossterm`, modeled on `/Users/reagan/.superset/projects/browser-use-terminal` (`crates/browser-use-tui/`) — reuse its `composer.rs` / `render.rs` / `theme.rs` patterns. Aim for ≤500 LoC in `render.rs` (the reference is 2.3k but does much more).
- **Index:** SQLite at `~/.config/asm/index.db`. Incremental — stat mtimes, re-parse only changed files.
- **Provider-agnostic schema:** the `Session` struct is universal. Each provider's parser is an adapter that fills in what it can; unfillable fields are `None`. The list view never branches on provider.

## The list IS the UI

Running `asm` (or `asm ls`) drops you into a single interactive searchable list:

```
  asm  ▸  type to search, @tag to filter                              42 sessions

  ● claude   12m  refactor auth middleware to use new session store     ~/repo       feat/auth
  ● codex     1h  investigating slow query in users table               ~/repo       main
    claude    3h  port the importer to async                            ~/etl        async-pipe
    codex     5h  brainstorm session-management CLI                     ~/asm        main
    claude   1d   draft RFC for billing migration                       ~/billing    rfc/billing
    …

  enter resume   ctrl-f fork   ctrl-e export   ctrl-r reindex   ? help
```

- `●` = live process. Always sorted to the top.
- After that: sorted by `last_user_msg_at` (when the human last spoke).
- Type to fuzzy-match across title + cwd + branch.
- `@tag` filters inline. Combinable — multiple filters stack as AND.
  - **Boolean tags:** `@codex`, `@claude`, `@running`, `@here`, `@today`, `@week`
  - **Valued tags:** `@branch:<name>` (e.g. `@branch:feat/auth`), `@path:<substring>` (e.g. `@path:asm` matches any cwd containing "asm")
  - Example: `@claude @branch:main @path:repo auth` = Claude sessions, on `main`, in any cwd containing `repo`, fuzzy-matching "auth" in title.
- As you type a recognized `@tag`, it renders as a colored chip *above* the input line and is removed from the literal search string. Cleared with backspace from start-of-input or `ctrl-shift-<n>` to remove the Nth chip.
- Subagent sessions are **filtered out by default** — you never resume into one, you resume into the parent.
- Preview pane shows: title, where (cwd / branch / agent / live status), when (last-user / last-assistant timestamps), what (last user prompt + last assistant reply). No tokens, no turn counts, no engineering vanity.

### Why not just `skim`?

Earlier draft used `skim` (Rust fzf). It fails the moment we want **rendered filter chips with typed values**:
- `skim` treats `@branch:feat/auth` as a literal fuzzy-match substring — no separation between filter language and search text.
- No way to render a colored chip strip above the input.
- No per-filter match counts.

So we move to `ratatui` + `crossterm`, modeled on `/Users/reagan/.superset/projects/browser-use-terminal` (`crates/browser-use-tui/`). Lift the patterns:

- **Workspace layout** — single binary crate for v1 (don't need a workspace yet; split later if other surfaces emerge). Their `crates/browser-use-tui/src/{main,composer,render,theme,palette}.rs` is the reference pattern.
- **`Composer` for the input bar** — see `composer.rs` (745 lines). We don't need most of it; pull the cursor + insert + delete + key-handling skeleton, add `@tag` parsing on each keystroke that extracts recognized tokens into a `Vec<FilterChip>` and strips them from the visible text.
- **Single `render.rs`** that lays out: top = filter chips row, middle = scrollable list, right = preview pane, bottom = keybind hint. Their `render.rs` (2381 lines) is overkill for us — aim for <500 LoC.
- **`theme.rs` + `palette.rs`** — copy the pattern of named colors and reuse-once styles. Tiny files, big readability win.

Dependencies (mirroring the terminal repo):
- `ratatui = "0.30"` (default-features off, `crossterm_0_29` feature)
- `crossterm = "0.29"`
- `nucleo-matcher` for fuzzy matching (lighter than embedding skim)

## Commands (the whole surface)

```
asm                  # open the list (default)
asm ls               # same thing
asm reindex          # force full re-scan
asm resume <id>      # non-interactive resume by id (for scripts)
asm fork <id>        # non-interactive fork by id (for scripts)
```

Everything else (search, filter, resume, fork, export) is keybinds inside the list.

## Forking (in v1)

`ctrl-f` from inside the list:
1. Sub-picker: pick a cut point (defaults to "end of session" = full copy; can scroll back through turns).
2. Copy source JSONL up to and including that point into a new file anchored to **the user's current cwd** (graft semantics — original message paths stay as-is, new working dir is wherever you ran `asm`):
   - Claude: new uuid filename in `~/.claude/projects/<encoded-CURRENT-cwd>/` so `claude --resume` finds it from where you are.
   - Codex: new `rollout-*.jsonl` in `~/.codex/sessions/YYYY/MM/DD/`, with `session_meta` rewritten — new `id`, `timestamp` = now, `cwd` = current cwd, `git` re-read from current cwd's repo.
3. Immediately resume the new session (`--no-resume` flag for scripted use).

Implementation order: Claude format first (simpler), Codex second (must rewrite `session_meta`).

## Data model (universal)

```rust
enum Agent { Claude, Codex }

enum Entrypoint { Cli, Tui, Sdk, Vscode, Exec, Desktop, Other(String) }

struct Session {
    // identity
    id: String,
    agent: Agent,                          // tag for resume dispatch + @filter only
    path: PathBuf,

    // environment
    cwd: Option<PathBuf>,
    git_branch: Option<String>,
    entrypoint: Option<Entrypoint>,

    // display
    title: Option<String>,                 // Claude ai-title when present; else first prompt truncated
    first_user_prompt: Option<String>,
    last_assistant_text: Option<String>,   // truncated ~200 chars

    // timing
    started_at: DateTime<Utc>,
    last_user_msg_at: Option<DateTime<Utc>>,
    last_assistant_msg_at: Option<DateTime<Utc>>,

    // noise filter
    user_msg_count: u32,                   // > 0 = real session; == 0 = dead-on-arrival

    // status
    is_live: bool,                         // process scan, not file
    is_sidechain: bool,                    // hide from default list
}
```

Dropped from earlier drafts: `model`, `files_touched`, `last_tool_call_at`, `parent_session_id`, `subagent_role`, `subagent_nickname`, `was_aborted`, `tokens_in`, `tokens_out`, `cached_tokens`, `context_window`, `msg_count`, `turn_count`, `active_duration_ms`. None of these help the user find / fork / resume — pure noise in the preview.

## Field source map (per provider)

Every field has a path on both providers, or is explicitly `None` with a TODO. See `reagan_session_schemas.md` for the full empirical schema reference.

| Field | Claude source | Codex source |
|---|---|---|
| `id` | filename uuid | `session_meta.id` |
| `cwd` | any envelope `.cwd` (use first) | `session_meta.cwd` |
| `git_branch` | envelope `.gitBranch` (`"HEAD"`→None) | `session_meta.git.branch` |
| `entrypoint` | envelope `.entrypoint` | `session_meta.originator` |
| `title` | last `ai-title.aiTitle` → else first prompt trunc | first prompt trunc |
| `first_user_prompt` | first non-`isMeta` `user.message.content` | first `event_msg/user_message.message` |
| `last_assistant_text` | last `assistant.message.content` text block | `task_complete.last_agent_message` |
| `started_at` | first envelope event ts | `session_meta.timestamp` |
| `last_user_msg_at` | last non-`isMeta` `user` event ts | last `event_msg/user_message` ts |
| `last_assistant_msg_at` | last `assistant` event ts | last `event_msg/agent_message` or `task_complete` ts |
| `user_msg_count` | count of non-`isMeta` `user` events | count of `event_msg/user_message` |
| `is_live` | process scan | process scan |
| `is_sidechain` | any event `isSidechain: true` | `session_meta.thread_source == "subagent"` |

## Source locations
- Claude Code: `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`
- Codex: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`

Two parser modules implementing `trait Parser { fn parse(path: &Path) -> Result<Session>; }`.

## Sorting

Applied in priority:
1. `is_live = true` first.
2. Then `last_user_msg_at` descending.

Subagent sessions (`is_sidechain = true`) are filtered out before sorting unless explicitly toggled on.

No `--sort` flag. The search box and filters cover the rest.

## Liveness detection

`sysinfo` crate walks running processes for `claude` and `codex`. Match:
1. Session id in argv (if present).
2. Process cwd matches session cwd AND session was touched in the last ~5 minutes.

Confirmed-live → solid `●`. Ambiguous match → dim `●`.

## Index schema (SQLite)

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  agent TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  path_mtime INTEGER NOT NULL,
  cwd TEXT,
  git_branch TEXT,
  entrypoint TEXT,
  title TEXT,
  first_user_prompt TEXT,
  last_assistant_text TEXT,
  started_at INTEGER,
  last_user_msg_at INTEGER,
  last_assistant_msg_at INTEGER,
  user_msg_count INTEGER,
  is_sidechain INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_cwd          ON sessions(cwd);
CREATE INDEX idx_last_user    ON sessions(last_user_msg_at DESC);
CREATE INDEX idx_agent        ON sessions(agent);
CREATE INDEX idx_sidechain    ON sessions(is_sidechain);

CREATE VIRTUAL TABLE messages_fts USING fts5(session_id, body);  -- for in-list grep
```

## Crates
- `clap` (derive) — args
- `serde` + `serde_json` — JSONL parsing
- `rusqlite` (bundled, with `fts5`) — index
- `ratatui` (0.30, crossterm backend) — TUI rendering
- `crossterm` (0.29) — terminal events + raw mode
- `nucleo-matcher` — fuzzy matching for the search box
- `chrono` — timestamps
- `walkdir` — directory traversal
- `sysinfo` — process liveness
- `anyhow` + `thiserror` — errors
- `rayon` — parallel JSONL parsing on first index
- `tracing` — verbose logging

## Build order
1. **Universal `Session` type + parser trait + tests** with real samples from both `~/.claude` and `~/.codex`.
2. **Claude parser.** Cover all fields with a Claude source.
3. **Codex parser.** Cover all fields with a Codex source.
4. **Index.** Schema, incremental update, `asm reindex`.
5. **List view (ratatui).** Lift `composer.rs` / `render.rs` / `theme.rs` skeleton from the terminal repo. Build:
   a. Composer that parses `@tag` and `@tag:value` tokens on each keystroke, extracts them into a `Vec<FilterChip>`, and removes them from the visible search string.
   b. Filter chip strip rendered above the input.
   c. Scrollable list with default sort (live first, then `last_user_msg_at` desc) and the sidechain filter always applied.
   d. Right-hand preview pane showing title / where / when / last user prompt / last assistant reply.
   e. `nucleo-matcher` fuzzy match on remaining search text against `title || first_user_prompt || cwd || git_branch`.
6. **Resume keybind** — dispatches to `claude --resume <id>` / `codex resume <id>`.
7. **Fork keybind** — Claude first, then Codex (`session_meta` rewrite).
8. **Export keybind** — markdown dump to stdout or file.
9. **Liveness detection** — bolt on; live-first sort lights up.
10. **FTS message search** inside the list.

## Resolved decisions
- **Resume:** `exec` replaces the current process. Selecting from the list and hitting enter drops you straight into the resumed agent in the same terminal.
- **Fork destination:** current cwd (graft semantics). Claude session lands in `~/.claude/projects/<encoded-CURRENT-cwd>/`; Codex `session_meta` is rewritten with current cwd + fresh `id` + re-read git info.
- **`was_aborted`:** dropped. Closing a terminal mid-session doesn't write `turn_aborted` anyway, so it doesn't capture the "I closed it accidentally" case. Recency sort already does.
- **Context window for Claude:** not derived. Field is `None` for Claude rows; preview just hides the "% used" line and shows absolute token counts.

## Open questions
None — all decisions resolved. Ready to start step 1 (universal `Session` type + parser trait + tests).

## Out of scope (for now)
- Subagent tree view — filtered out entirely.
- Tags/pins (user-defined, sidecar metadata) — v2.
- Watching daemon — manual `reindex` is fine.
- Cross-machine sync.
- Files-touched / commands-run extraction.
- Cost computation.
