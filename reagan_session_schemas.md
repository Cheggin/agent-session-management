# Session schema reference — Claude Code & Codex

Empirical survey of session files on this machine (2026-05-22).
- Claude Code: 717 session files across 66 project dirs at `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`
- Codex: 843 rollout files at `~/.codex/sessions/YYYY/MM/DD/rollout-<ISO-ts>-<id>.jsonl`

Sample size: 100 files each, plus 50 files for type enumeration. Field unions reported below are exhaustive across the sample.

---

## Claude Code

### File location & naming

```
~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl
```

- `encoded-cwd` is the cwd with `/` replaced by `-`. Example: `-Users-reagan--superset-projects-better-linear` ↔ `/Users/reagan/.superset/projects/better-linear`. The double-dash encodes a leading dot directory. Reversible by string replacement; not always 1:1 if a real path component contained `-`, so prefer `cwd` field inside events when available.
- `session-uuid` matches `sessionId` inside every event.

### Event types (top-level `type` field)

Counts are from a 100-file sample:

| `type` | count | role |
|---|---|---|
| `attachment` | 30,369 | hook outputs, SessionStart artifacts |
| `assistant` | 6,948 | model turn (carries usage, model name) |
| `user` | 4,271 | user input or tool result |
| `last-prompt` | 1,755 | denormalized "most recent user prompt" |
| `permission-mode` | 1,425 | permission mode set/changed |
| `system` | 1,027 | stop-hook summaries |
| `file-history-snapshot` | 720 | tracked-file backup records |
| `ai-title` | 595 | **Claude-generated session title** |
| `queue-operation` | 455 | enqueued user inputs |
| `pr-link` | 11 | linked PR |

No `summary` events observed in current Claude Code versions (2.1.118+). Older versions had them; do not rely on it.

### Fields available at the event level (no semantic parsing needed)

Most non-trivial events carry the same envelope. Union of keys across `assistant` / `user` / `attachment` / `system`:

| Field | Source events | Notes |
|---|---|---|
| `sessionId` | all | session uuid, matches filename |
| `uuid` | most | per-event uuid |
| `parentUuid` | most | links into the conversation tree |
| `timestamp` | most | ISO-8601 UTC, ms precision |
| `cwd` | envelope events | working directory — authoritative, no decoding needed |
| `gitBranch` | envelope events | current branch; observed value `"HEAD"` when detached |
| `version` | envelope events | Claude Code version (e.g. `"2.1.118"`, `"2.1.126"`) |
| `entrypoint` | envelope events | one of: `cli`, `sdk-cli`, `claude-desktop` |
| `userType` | envelope events | observed: `external` |
| `isSidechain` | envelope events | true = subagent/Task tool side conversation |
| `isMeta` | user events | hidden system-injected messages |

### `assistant` event — model + tokens

```jsonc
{
  "type": "assistant",
  "message": {
    "model": "claude-opus-4-7",          // or claude-sonnet-4-6, claude-opus-4-6, "<synthetic>"
    "id": "msg_...",
    "role": "assistant",
    "content": [...],
    "stop_reason": "tool_use",
    "usage": {
      "input_tokens": 6,
      "output_tokens": 6,
      "cache_creation_input_tokens": 24895,
      "cache_read_input_tokens": 20839,
      "service_tier": "standard",
      "speed": "standard",
      "cache_creation": { "ephemeral_1h_input_tokens": ..., "ephemeral_5m_input_tokens": ... },
      "server_tool_use": { "web_search_requests": 0, "web_fetch_requests": 0 },
      "inference_geo": "",
      "iterations": [ { ...per-iteration breakdown... } ]
    },
    "requestId": "..."
  },
  ...envelope fields...
}
```

Model values observed (top of distribution): `claude-opus-4-7` (overwhelming majority), `claude-sonnet-4-6`, `claude-opus-4-6`, and the literal string `<synthetic>` for fake/locally-generated assistant turns.

### `user` event — first/last human prompt

`message.content` is either a string (simple text turn) or an array of typed parts. `isMeta: true` marks system-injected user turns (CLAUDE.md, hook output) — filter these out when computing "first user prompt".

### `last-prompt` — denormalized last prompt (use this!)

```jsonc
{
  "type": "last-prompt",
  "lastPrompt": "Reply with exactly the word OK and nothing else.",
  "leafUuid": "...",
  "sessionId": "..."
}
```

Claude writes this convenience event so you don't need to walk the message tree to find the most recent user turn. Use the most recent `last-prompt` event in the file.

### `ai-title` — Claude's own auto-title

```jsonc
{ "type": "ai-title", "aiTitle": "Improve sidebar navigation to dashboard", "sessionId": "..." }
```

**This is the killer field.** Claude already generates a human-readable title for each session. We don't need our own LLM summarizer. Take the most recent `ai-title` event.

### `permission-mode`

```jsonc
{ "type": "permission-mode", "permissionMode": "default" | "bypassPermissions" | ..., "sessionId": "..." }
```

---

## Codex

### File location & naming

```
~/.codex/sessions/YYYY/MM/DD/rollout-<ISO-ts>-<session-id>.jsonl
```

- Date partitioned by **session start time**, in **local time** (verified from filenames).
- `session-id` is a UUIDv7 — first 48 bits encode ms timestamp, so session IDs sort lexicographically by start time.
- The filename's ISO timestamp matches `payload.timestamp` inside the first event, not file mtime.

### Top-level event types

| `type` | count | role |
|---|---|---|
| `response_item` | 7,341 | model output stream (messages, function calls, reasoning) |
| `event_msg` | 4,768 | UI-facing events (token counts, user input, task lifecycle) |
| `turn_context` | 169 | per-turn config snapshot (model, sandbox, effort) |
| `session_meta` | 50 | **one per file, line 1** — session header |
| `compacted` | 16 | history compaction record |

Every event has `{timestamp, type, payload}` at the top level. `timestamp` is ISO-8601 UTC.

### `session_meta` — the metadata jackpot (line 1 of every file)

```jsonc
{
  "timestamp": "...",
  "type": "session_meta",
  "payload": {
    "id": "019da466-...",                    // session uuid
    "timestamp": "2026-04-19T06:21:06.469Z", // session start
    "cwd": "/Users/reagan/...",
    "originator": "codex-tui" | "codex_exec" | "codex_cli_rs" | "stress",
    "cli_version": "0.121.0",                // Codex CLI version
    "source": "vscode" | "cli" | "exec" | { "subagent": {...} } | null,
    "model_provider": "openai",
    "agent_nickname": "McClintock",          // present for subagents
    "agent_role": "executor" | "explore" | "verifier" | "dependency-expert" | ...
    "thread_source": "user" | "subagent",
    "git": {
      "commit_hash": "03cd5638...",
      "branch": "feature/chat-view",         // omitted when detached
      "repository_url": "https://github.com/..."
    },
    "base_instructions": { "text": "...you are Codex..." }
  }
}
```

**Everything we need for the list comes from this one line.** No file walk required for cwd / branch / repo / start time / cli version / subagent identity.

`source` is polymorphic:
- string `"vscode"` / `"cli"` / `"exec"` for top-level sessions started from those clients
- object `{ "subagent": { "thread_spawn": { "parent_thread_id": "...", "depth": N, "agent_path": null, "agent_nickname": "...", "agent_role": "..." } } }` for subagent rollouts. This lets us link forked/sub sessions back to their parent.

### `turn_context` — model can change between turns

```jsonc
{
  "type": "turn_context",
  "payload": {
    "turn_id": "...",
    "cwd": "/Users/reagan/...",
    "current_date": "2026-04-18",
    "timezone": "America/Los_Angeles",
    "approval_policy": "never",
    "sandbox_policy": { "type": "danger-full-access" },
    "model": "gpt-5.4",                     // ← per-turn model
    "personality": "pragmatic",
    "effort": "high" | "medium" | "low",
    "summary": "none",
    "collaboration_mode": { "mode": "default", "settings": {...} },
    "user_instructions": "...AGENTS.md content...",
    "realtime_active": false
  }
}
```

Take the most recent `turn_context.payload.model` for "current model". Model differs across turns in some sessions, so a single-shot read isn't enough — but for a list view, the last value is what's used.

### `event_msg/token_count` — token usage (the cumulative one)

Two shapes observed:

1. Early in the session, `info` is `null` (only `rate_limits` filled in).
2. Once the model has run, `info` contains:

```jsonc
{
  "type": "token_count",
  "info": {
    "total_token_usage": {
      "input_tokens": 76948,
      "cached_input_tokens": 42496,
      "output_tokens": 796,
      "reasoning_output_tokens": 546,
      "total_tokens": 77744
    },
    "last_token_usage": { ...same shape, this-turn only... },
    "model_context_window": 121600
  },
  "rate_limits": {
    "limit_id": "codex",
    "primary":   { "used_percent": 15.0, "window_minutes": 300,  "resets_at": 1778710758 },
    "secondary": { "used_percent": 14.0, "window_minutes": 10080,"resets_at": 1779146194 },
    "plan_type": "pro",
    "rate_limit_reached_type": null
  }
}
```

For session-total tokens, scan for the **last** `event_msg/token_count` where `info` is non-null and read `info.total_token_usage`.

### `event_msg/task_started` and `task_complete` — turn timing

```jsonc
// task_started
{ "type": "task_started", "turn_id": "...", "started_at": 1776579666, "model_context_window": 950000, "collaboration_mode_kind": "default" }

// task_complete
{ "type": "task_complete", "turn_id": "...", "completed_at": ..., "duration_ms": 12345, "time_to_first_token_ms": 800, "last_agent_message": "..." }

// turn_aborted (alternative end)
{ "type": "turn_aborted", "turn_id": "...", "completed_at": ..., "duration_ms": ..., "reason": "..." }
```

Useful derived metadata, no semantic work:
- **Active wall-clock time** = sum of `task_complete.duration_ms` + `turn_aborted.duration_ms`
- **Turn count** = number of `task_started` events
- **Last user-visible reply** = `task_complete.last_agent_message` of the latest completion (use as a "what did it last say" preview without parsing response_items)

### `event_msg/user_message` — first/last user prompt

```jsonc
{ "type": "user_message", "message": "...", "images": [...], "local_images": [...], "text_elements": [...] }
```

- First user prompt = first `event_msg/user_message`.
- Last user prompt = last `event_msg/user_message`.

### `event_msg/agent_message` — last assistant text

```jsonc
{ "type": "agent_message", "message": "...", "phase": "...", "memory_citation": ... }
```

(Or use `task_complete.last_agent_message`, which is denormalized for exactly this purpose.)

---

## What we get deterministically — final field map for `asm`

The `Session` struct we can populate **without** any LLM/semantic processing, just by reading specific event fields:

| `Session` field | Claude source | Codex source |
|---|---|---|
| `id` | filename uuid = `sessionId` in events | `session_meta.payload.id` |
| `agent` | derived from path | derived from path |
| `path` | filename | filename |
| `cwd` | any envelope event `.cwd` (consistent within a file) | `session_meta.payload.cwd` |
| `git_branch` | any envelope event `.gitBranch` (filter `"HEAD"` → None) | `session_meta.payload.git.branch` |
| `git_commit` | — (not in JSONL) | `session_meta.payload.git.commit_hash` |
| `git_repo_url` | — | `session_meta.payload.git.repository_url` |
| `started_at` | timestamp of first envelope event | `session_meta.payload.timestamp` (or filename) |
| `last_user_msg_at` | timestamp of last non-`isMeta` `user` event (or last `last-prompt`'s file position → use file event timestamp before it) | timestamp of last `event_msg/user_message` |
| `last_assistant_msg_at` | timestamp of last `assistant` event | timestamp of last `event_msg/agent_message` or `task_complete` |
| `last_turn_end_at` | timestamp of last `system/stop_hook_summary` | timestamp of last `task_complete`/`turn_aborted` |
| `first_user_prompt` | first non-`isMeta` `user.message.content` (or string of it) | first `event_msg/user_message.message` |
| `last_prompt` | last `last-prompt.lastPrompt` (denormalized!) | last `event_msg/user_message.message` |
| `ai_title` | last `ai-title.aiTitle` (**use this as primary display label**) | — (Codex doesn't generate titles; fall back to first prompt or last `task_complete.last_agent_message`) |
| `msg_count` | count of `user` + `assistant` events | count of `event_msg/user_message` + `event_msg/agent_message` |
| `user_msg_count` | count of `user` events where `isMeta != true` | count of `event_msg/user_message` |
| `turn_count` | count of `assistant` events (rough) | count of `task_started` events (exact) |
| `tokens_in` | sum of `assistant.message.usage.input_tokens` (+ cache_read + cache_creation if we want "real" tokens) | last `event_msg/token_count.info.total_token_usage.input_tokens` |
| `tokens_out` | sum of `assistant.message.usage.output_tokens` | last `event_msg/token_count.info.total_token_usage.output_tokens` |
| `cached_tokens` | sum of `cache_read_input_tokens` | last `total_token_usage.cached_input_tokens` |
| `context_window` | — | `event_msg/task_started.model_context_window` (or `token_count.info.model_context_window`) |
| `active_duration_ms` | sum of `system/stop_hook_summary` durations | sum of `task_complete.duration_ms` + `turn_aborted.duration_ms` |
| `model` | most-recent `assistant.message.model` (skip `<synthetic>`) | most-recent `turn_context.payload.model` |
| `cli_version` | any envelope event `.version` | `session_meta.payload.cli_version` |
| `entrypoint` | any envelope event `.entrypoint` (`cli` / `sdk-cli` / `claude-desktop`) | `session_meta.payload.originator` (`codex-tui` / `codex_exec` / `codex_cli_rs`) + `source` (`vscode` / `cli` / `exec`) |
| `permission_mode` | last `permission-mode.permissionMode` | `turn_context.payload.approval_policy` + `sandbox_policy.type` |
| `is_sidechain` | true if any event has `isSidechain: true` | true if `session_meta.payload.thread_source == "subagent"` |
| `parent_session_id` | — (not exposed for Claude Task-tool sidechains in the JSONL — they get their own file? TBD) | `session_meta.payload.source.subagent.thread_spawn.parent_thread_id` |
| `subagent_role` | — | `session_meta.payload.agent_role` |
| `subagent_nickname` | — | `session_meta.payload.agent_nickname` |
| `last_event_at` | timestamp of last event | timestamp of last event |
| `was_aborted` | — (Claude has no clear marker) | true if last lifecycle event is `turn_aborted`, false if `task_complete` |

### Notes the parsers must handle

1. **Claude `cwd` is per-event** — it's redundantly repeated. We can grab it from any envelope event. If the user `cd`s mid-session (rare but possible), prefer the *first* `cwd` for filesystem indexing and surface a flag if it ever changes.
2. **Claude `version` can shift mid-file** if Claude upgrades and the user resumes; same — take the first.
3. **`gitBranch: "HEAD"` means detached HEAD** — treat as `None`.
4. **Empty lines and partial JSON** at end-of-file happen (write-in-progress). Skip with try/parse.
5. **Codex `info: null` token_count events** are common; skip those when computing totals — look for the last one with `info != null`.
6. **Codex session files for subagents** live in the *same* day directory as the parent — no nesting. They're linked only via `source.subagent.thread_spawn.parent_thread_id`. We should display these in a parent/child view, not flat.
7. **Sort key sanity:** `last_user_msg_at` for "when did the human last speak" is what we want. Claude's `last-prompt` event is a denormalized version of the same thing — its mere presence at file position N tells us the timestamp of the surrounding events.
8. **Claude has no `git_commit_hash` or `repo_url`** in the JSONL — git info is just the branch name. If we want full git metadata for Claude sessions, we'd have to shell into the cwd ourselves (out of scope per the user's "ignore manually extracted fields" rule).
9. **`<synthetic>` model** appears in Claude when something locally fakes an assistant turn (e.g., resumed-context synthesis). Skip when computing "actual model used".

### What we are NOT extracting (per user direction)

- Files touched (would require parsing every tool call's args)
- Commands run (same)
- Semantic summary or topic
- Diff-style "what changed in the repo"
- Cost (would require pricing tables × token columns)

These can come later as derived fields once the readily-available metadata above is in place.

---

## Implications for `asm`

1. **Display label priority for the list:**
   - Claude: `ai_title` → `first_user_prompt` (truncated 80 chars) → `id`
   - Codex: `first_user_prompt` (truncated 80 chars) → `subagent_role` + `agent_nickname` → `id`
2. **Sort key:** `last_user_msg_at`, then by `is_live` priority on top.
3. **Filter chips powered by data we just confirmed:**
   - `@claude` / `@codex` — agent
   - `@here` — cwd matches
   - `@branch:foo` — gitBranch / git.branch
   - `@subagent` — sidechain or thread_source == subagent
   - `@aborted` — codex turn_aborted as last lifecycle
   - `@today` / `@week` — last_user_msg_at recency
4. **Fork source-of-truth:** for both agents, every event we'd need to slice on (turn boundaries, user messages, assistant messages) is identifiable by `type` alone. No regex parsing of message bodies needed.
5. **Resume command shape:**
   - Claude: `claude --resume <session-uuid>` (sessionId from filename or events)
   - Codex: `codex resume <session-id>` (id from `session_meta.payload.id`)
