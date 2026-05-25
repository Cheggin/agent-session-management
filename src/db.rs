//! SQLite index for parsed agent sessions.
//!
//! Index path note: the product plan pins the index at `~/.config/asm/index.db`;
//! `dirs` is still used for home-directory discovery, with `dirs::config_dir()`
//! as a last-resort fallback when home cannot be resolved.
//!
//! Timestamp storage note: the plan schema declares the timestamp columns with
//! SQLite `INTEGER` affinity. SQLite is dynamically typed, so Step 4 stores
//! `DateTime<Utc>` values in those columns as serde-backed RFC3339 UTC
//! strings. That keeps millisecond/nanosecond precision for exact `Session`
//! round-trips while still preserving the requested schema text.
//! `path_mtime` is stored separately as a Unix nanosecond integer for
//! incremental reindex checks.
//! `is_live` is not present in the Step 4 schema because liveness is a later
//! process-scan concern; rows loaded from this index reconstruct it as `false`.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use tracing::{info, info_span};

use crate::{Agent, Entrypoint, Session, ui::filter::Chip};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  agent TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  path_mtime INTEGER NOT NULL,
  cwd TEXT,
  git_branch TEXT,
  entrypoint TEXT,
  title TEXT,
  first_user_prompt TEXT,
  recent_user_prompts TEXT,
  last_assistant_text TEXT,
  started_at INTEGER,
  last_user_msg_at INTEGER,
  last_assistant_msg_at INTEGER,
  user_msg_count INTEGER,
  is_sidechain INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_cwd          ON sessions(cwd);
CREATE INDEX IF NOT EXISTS idx_last_user    ON sessions(last_user_msg_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent        ON sessions(agent);
CREATE INDEX IF NOT EXISTS idx_sidechain    ON sessions(is_sidechain);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
USING fts5(session_id UNINDEXED, body, tokenize='porter unicode61');

CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value INTEGER NOT NULL
);
"#;

const META_LAST_REINDEX_AT_SECS: &str = "last_reindex_at_secs";

const UPSERT_SESSION: &str = r#"
INSERT INTO sessions (
  id,
  agent,
  path,
  path_mtime,
  cwd,
  git_branch,
  entrypoint,
  title,
  first_user_prompt,
  recent_user_prompts,
  last_assistant_text,
  started_at,
  last_user_msg_at,
  last_assistant_msg_at,
  user_msg_count,
  is_sidechain
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
ON CONFLICT(path) DO UPDATE SET
  id = excluded.id,
  agent = excluded.agent,
  path_mtime = excluded.path_mtime,
  cwd = excluded.cwd,
  git_branch = excluded.git_branch,
  entrypoint = excluded.entrypoint,
  title = excluded.title,
  first_user_prompt = excluded.first_user_prompt,
  recent_user_prompts = excluded.recent_user_prompts,
  last_assistant_text = excluded.last_assistant_text,
  started_at = excluded.started_at,
  last_user_msg_at = excluded.last_user_msg_at,
  last_assistant_msg_at = excluded.last_assistant_msg_at,
  user_msg_count = excluded.user_msg_count,
  is_sidechain = excluded.is_sidechain
"#;

pub struct Index {
    conn: Connection,
    path: PathBuf,
}

impl Index {
    pub fn open() -> Result<Self> {
        Self::open_at(default_index_path()?)
    }

    pub fn open_in_dir(dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_at(dir.as_ref().join("index.db"))
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let span = info_span!("asm.db.open");
        let _enter = span.enter();
        let started = Instant::now();
        let result = (|| {
            let path = path.as_ref().to_path_buf();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create index directory {}", parent.display())
                })?;
            }

            let conn = Connection::open(&path)
                .with_context(|| format!("failed to open SQLite index {}", path.display()))?;
            let index = Self { conn, path };
            index.migrate()?;
            Ok(index)
        })();
        info!(
            elapsed_ms = elapsed_ms(started),
            ok = result.is_ok(),
            "startup phase complete"
        );
        result
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn upsert_session(
        &mut self,
        session: &Session,
        path_mtime: i64,
        message_bodies: &[String],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut upsert_stmt = tx.prepare(UPSERT_SESSION)?;
            let mut delete_by_path_stmt = tx.prepare(
                "DELETE FROM messages_fts WHERE session_id IN \
                 (SELECT id FROM sessions WHERE path = ?1)",
            )?;
            let mut delete_by_id_stmt =
                tx.prepare("DELETE FROM messages_fts WHERE session_id = ?1")?;
            let mut insert_body_stmt =
                tx.prepare("INSERT INTO messages_fts (session_id, body) VALUES (?1, ?2)")?;
            execute_upsert_with_bodies(
                &mut upsert_stmt,
                &mut delete_by_path_stmt,
                &mut delete_by_id_stmt,
                &mut insert_body_stmt,
                session,
                path_mtime,
                message_bodies,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_sessions_only(&mut self, sessions: &[(Session, i64)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut upsert_stmt = tx.prepare(UPSERT_SESSION)?;
            for (session, path_mtime) in sessions {
                execute_upsert(&mut upsert_stmt, session, *path_mtime)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_messages_fts(&mut self, messages: &[(String, Vec<String>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut delete_by_id_stmt =
                tx.prepare("DELETE FROM messages_fts WHERE session_id = ?1")?;
            let mut insert_body_stmt =
                tx.prepare("INSERT INTO messages_fts (session_id, body) VALUES (?1, ?2)")?;
            for (session_id, message_bodies) in messages {
                delete_by_id_stmt.execute(params![session_id])?;
                for body in message_bodies.iter().filter(|body| !body.trim().is_empty()) {
                    insert_body_stmt.execute(params![session_id, body])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_sessions(&mut self, sessions: &[(Session, i64, Vec<String>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut upsert_stmt = tx.prepare(UPSERT_SESSION)?;
            let mut delete_by_path_stmt = tx.prepare(
                "DELETE FROM messages_fts WHERE session_id IN \
                 (SELECT id FROM sessions WHERE path = ?1)",
            )?;
            let mut delete_by_id_stmt =
                tx.prepare("DELETE FROM messages_fts WHERE session_id = ?1")?;
            let mut insert_body_stmt =
                tx.prepare("INSERT INTO messages_fts (session_id, body) VALUES (?1, ?2)")?;
            for (session, path_mtime, message_bodies) in sessions {
                execute_upsert_with_bodies(
                    &mut upsert_stmt,
                    &mut delete_by_path_stmt,
                    &mut delete_by_id_stmt,
                    &mut insert_body_stmt,
                    session,
                    *path_mtime,
                    message_bodies,
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn fts_match(&self, pattern: &str) -> Result<HashSet<String>> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Ok(HashSet::new());
        }

        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT session_id FROM messages_fts WHERE messages_fts MATCH ?1")?;
        let rows = stmt.query_map(params![pattern], |row| row.get::<_, String>(0))?;
        let mut matches = HashSet::new();
        for row in rows {
            matches.insert(row?);
        }
        Ok(matches)
    }

    pub fn dump_message_bodies(&self) -> Result<HashMap<String, String>> {
        let span = info_span!("asm.db.dump_bodies");
        let _enter = span.enter();
        let started = Instant::now();
        let result = (|| {
            let mut stmt = self
                .conn
                .prepare("SELECT session_id, body FROM messages_fts ORDER BY rowid")?;
            let mut rows = stmt.query([])?;
            let mut bodies = HashMap::<String, String>::new();
            while let Some(row) = rows.next()? {
                let session_id: String = row.get(0)?;
                let body: String = row.get(1)?;
                let entry = bodies.entry(session_id).or_default();
                if !entry.is_empty() {
                    entry.push('\n');
                }
                entry.push_str(&body);
            }
            Ok(bodies)
        })();
        info!(
            elapsed_ms = elapsed_ms(started),
            ok = result.is_ok(),
            "startup phase complete"
        );
        result
    }

    pub fn get_path_mtime(&self, path: impl AsRef<Path>) -> Result<Option<i64>> {
        let path = path_to_db(path.as_ref());
        self.conn
            .query_row(
                "SELECT path_mtime FROM sessions WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn path_has_message_bodies(&self, path: impl AsRef<Path>) -> Result<bool> {
        let path = path_to_db(path.as_ref());
        self.conn
            .query_row(
                r#"
SELECT CASE
  WHEN user_msg_count = 0 THEN 1
  WHEN EXISTS (
    SELECT 1 FROM messages_fts WHERE messages_fts.session_id = sessions.id LIMIT 1
  ) THEN 1
  ELSE 0
END
FROM sessions
WHERE path = ?1
"#,
                params![path],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map(|value| value == Some(1))
            .map_err(Into::into)
    }

    pub fn last_reindex_age(&self) -> Result<Option<Duration>> {
        let Some(last_reindex_at_secs) = self.last_reindex_at_secs()? else {
            return Ok(None);
        };
        let now_secs = unix_now_secs()?;
        let age_secs = now_secs.saturating_sub(last_reindex_at_secs);
        Ok(Some(Duration::from_secs(
            u64::try_from(age_secs).unwrap_or(0),
        )))
    }

    pub fn mark_reindexed_now(&self) -> Result<()> {
        let now_secs = unix_now_secs()?;
        self.conn.execute(
            r#"
INSERT INTO meta (key, value)
VALUES (?1, ?2)
ON CONFLICT(key) DO UPDATE SET value = excluded.value
"#,
            params![META_LAST_REINDEX_AT_SECS, now_secs],
        )?;
        Ok(())
    }

    pub fn list_all(&self) -> Result<Vec<Session>> {
        let span = info_span!("asm.db.list_all");
        let _enter = span.enter();
        let started = Instant::now();
        let result = self.query_sessions(
            r#"
SELECT
  id,
  agent,
  path,
  cwd,
  git_branch,
  entrypoint,
  title,
  first_user_prompt,
  recent_user_prompts,
  last_assistant_text,
  started_at,
  last_user_msg_at,
  last_assistant_msg_at,
  user_msg_count,
  is_sidechain
FROM sessions
ORDER BY last_user_msg_at DESC, started_at DESC, id ASC
"#,
            &[],
        );
        info!(
            elapsed_ms = elapsed_ms(started),
            ok = result.is_ok(),
            "startup phase complete"
        );
        result
    }

    pub fn list_filtered(&self, chips: &[Chip]) -> Result<Vec<Session>> {
        let span = info_span!("asm.db.list_filtered");
        let _enter = span.enter();
        let started = Instant::now();
        let result = {
            let mut sql = String::from(
                r#"
SELECT
  id,
  agent,
  path,
  cwd,
  git_branch,
  entrypoint,
  title,
  first_user_prompt,
  recent_user_prompts,
  last_assistant_text,
  started_at,
  last_user_msg_at,
  last_assistant_msg_at,
  user_msg_count,
  is_sidechain
FROM sessions
WHERE is_sidechain = 0
  AND user_msg_count > 0
  AND (entrypoint IS NULL OR entrypoint NOT IN ('sdk', 'exec'))
"#,
            );
            let mut params = Vec::<String>::new();

            for chip in chips {
                match chip {
                    Chip::HereCwd(path) => {
                        let cwd = path_to_db(path);
                        sql.push_str("  AND (cwd = ? OR cwd LIKE ?)\n");
                        params.push(cwd.clone());
                        params.push(descendant_like_pattern(&cwd));
                    }
                    Chip::Agent(agent) => {
                        sql.push_str("  AND agent = ?\n");
                        params.push(encode_agent(agent).to_owned());
                    }
                    Chip::Branch(branch) => {
                        sql.push_str("  AND LOWER(git_branch) = LOWER(?)\n");
                        params.push(branch.clone());
                    }
                    Chip::PathSubstring(path) => {
                        sql.push_str("  AND LOWER(cwd) LIKE '%' || LOWER(?) || '%'\n");
                        params.push(path.clone());
                    }
                    Chip::Recency(_) | Chip::Running => {}
                }
            }

            sql.push_str("ORDER BY last_user_msg_at DESC");
            self.query_sessions(&sql, &params)
        };
        info!(
            elapsed_ms = elapsed_ms(started),
            ok = result.is_ok(),
            "startup phase complete"
        );
        result
    }

    fn query_sessions(&self, sql: &str, params: &[String]) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query(params_from_iter(params.iter()))?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            sessions.push(session_from_row(row)?);
        }
        Ok(sessions)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -65536;
PRAGMA mmap_size = 268435456;
"#,
        )?;
        self.conn.execute_batch(SCHEMA)?;
        self.ensure_recent_user_prompts_column()?;
        Ok(())
    }

    fn ensure_recent_user_prompts_column(&self) -> Result<()> {
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = ?1",
            params!["recent_user_prompts"],
            |row| row.get(0),
        )?;
        if exists == 0 {
            self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN recent_user_prompts TEXT",
                [],
            )?;
        }
        Ok(())
    }

    fn last_reindex_at_secs(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![META_LAST_REINDEX_AT_SECS],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }
}

fn session_from_row(row: &Row<'_>) -> Result<Session> {
    let id: String = row.get(0)?;
    let agent_raw: String = row.get(1)?;
    let path_raw: String = row.get(2)?;
    let cwd_raw: Option<String> = row.get(3)?;
    let git_branch: Option<String> = row.get(4)?;
    let entrypoint_raw: Option<String> = row.get(5)?;
    let title: Option<String> = row.get(6)?;
    let first_user_prompt: Option<String> = row.get(7)?;
    let recent_user_prompts_raw: Option<String> = row.get(8)?;
    let last_assistant_text: Option<String> = row.get(9)?;
    let started_raw: String = row.get(10)?;
    let last_user_raw: Option<String> = row.get(11)?;
    let last_assistant_raw: Option<String> = row.get(12)?;
    let user_msg_count_raw: i64 = row.get(13)?;
    let is_sidechain_raw: i64 = row.get(14)?;

    Ok(Session {
        id: id.clone(),
        agent: decode_agent(&agent_raw)
            .with_context(|| format!("invalid agent for session {id}"))?,
        path: PathBuf::from(path_raw),
        cwd: cwd_raw.map(PathBuf::from),
        git_branch,
        entrypoint: entrypoint_raw
            .as_deref()
            .map(decode_entrypoint)
            .transpose()
            .with_context(|| format!("invalid entrypoint for session {id}"))?,
        title,
        first_user_prompt,
        recent_user_prompts: decode_recent_user_prompts(recent_user_prompts_raw.as_deref())
            .with_context(|| format!("invalid recent_user_prompts for session {id}"))?,
        last_assistant_text,
        started_at: decode_datetime(&started_raw)
            .with_context(|| format!("invalid started_at for session {id}"))?,
        last_user_msg_at: last_user_raw
            .as_deref()
            .map(decode_datetime)
            .transpose()
            .with_context(|| format!("invalid last_user_msg_at for session {id}"))?,
        last_assistant_msg_at: last_assistant_raw
            .as_deref()
            .map(decode_datetime)
            .transpose()
            .with_context(|| format!("invalid last_assistant_msg_at for session {id}"))?,
        user_msg_count: u32::try_from(user_msg_count_raw).with_context(|| {
            format!("invalid user_msg_count {user_msg_count_raw} for session {id}")
        })?,
        is_live: false,
        is_sidechain: is_sidechain_raw != 0,
    })
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

fn unix_now_secs() -> Result<i64> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before Unix epoch")?
        .as_secs();
    i64::try_from(secs).context("current Unix time overflows i64")
}

fn default_index_path() -> Result<PathBuf> {
    if let Some(home_dir) = dirs::home_dir() {
        return Ok(home_dir.join(".config").join("asm").join("index.db"));
    }

    let config_dir = dirs::config_dir().context("could not determine user config directory")?;
    Ok(config_dir.join("asm").join("index.db"))
}

fn execute_upsert(
    stmt: &mut rusqlite::Statement<'_>,
    session: &Session,
    path_mtime: i64,
) -> Result<()> {
    let path = path_to_db(&session.path);
    let cwd = session.cwd.as_deref().map(path_to_db);
    let entrypoint = session.entrypoint.as_ref().map(encode_entrypoint);
    let recent_user_prompts = encode_recent_user_prompts(&session.recent_user_prompts)?;
    let started_at = encode_datetime(&session.started_at)?;
    let last_user_msg_at = session
        .last_user_msg_at
        .as_ref()
        .map(encode_datetime)
        .transpose()?;
    let last_assistant_msg_at = session
        .last_assistant_msg_at
        .as_ref()
        .map(encode_datetime)
        .transpose()?;

    stmt.execute(params![
        &session.id,
        encode_agent(&session.agent),
        path,
        path_mtime,
        cwd,
        &session.git_branch,
        entrypoint,
        &session.title,
        &session.first_user_prompt,
        recent_user_prompts,
        &session.last_assistant_text,
        started_at,
        last_user_msg_at,
        last_assistant_msg_at,
        session.user_msg_count,
        bool_to_i64(session.is_sidechain),
    ])?;
    Ok(())
}

fn execute_upsert_with_bodies(
    upsert_stmt: &mut rusqlite::Statement<'_>,
    delete_by_path_stmt: &mut rusqlite::Statement<'_>,
    delete_by_id_stmt: &mut rusqlite::Statement<'_>,
    insert_body_stmt: &mut rusqlite::Statement<'_>,
    session: &Session,
    path_mtime: i64,
    message_bodies: &[String],
) -> Result<()> {
    let path = path_to_db(&session.path);
    delete_by_path_stmt.execute(params![path])?;
    execute_upsert(upsert_stmt, session, path_mtime)?;
    delete_by_id_stmt.execute(params![&session.id])?;
    for body in message_bodies.iter().filter(|body| !body.trim().is_empty()) {
        insert_body_stmt.execute(params![&session.id, body])?;
    }
    Ok(())
}

fn path_to_db(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn descendant_like_pattern(cwd: &str) -> String {
    if cwd == "/" {
        "/%".to_owned()
    } else {
        format!("{}/%", cwd.trim_end_matches('/'))
    }
}

fn encode_datetime(datetime: &DateTime<Utc>) -> Result<String> {
    let value = serde_json::to_value(datetime).context("failed to serialize datetime")?;
    value
        .as_str()
        .map(str::to_owned)
        .context("serialized datetime was not a string")
}

fn decode_datetime(value: &str) -> Result<DateTime<Utc>> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .with_context(|| format!("failed to parse RFC3339 datetime {value:?}"))
}

fn encode_recent_user_prompts(recent_user_prompts: &[String]) -> Result<String> {
    serde_json::to_string(recent_user_prompts).context("failed to serialize recent_user_prompts")
}

fn decode_recent_user_prompts(value: Option<&str>) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    serde_json::from_str(value)
        .with_context(|| format!("failed to parse recent_user_prompts JSON {value:?}"))
}

fn encode_agent(agent: &Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}

fn decode_agent(value: &str) -> Result<Agent> {
    match value {
        "claude" => Ok(Agent::Claude),
        "codex" => Ok(Agent::Codex),
        other => bail!("unknown agent {other:?}"),
    }
}

fn encode_entrypoint(entrypoint: &Entrypoint) -> String {
    match entrypoint {
        Entrypoint::Cli => "cli".to_owned(),
        Entrypoint::Tui => "tui".to_owned(),
        Entrypoint::Sdk => "sdk".to_owned(),
        Entrypoint::Vscode => "vscode".to_owned(),
        Entrypoint::Exec => "exec".to_owned(),
        Entrypoint::Desktop => "desktop".to_owned(),
        Entrypoint::Other(value) => format!("other:{value}"),
    }
}

fn decode_entrypoint(value: &str) -> Result<Entrypoint> {
    Ok(match value {
        "cli" => Entrypoint::Cli,
        "tui" => Entrypoint::Tui,
        "sdk" => Entrypoint::Sdk,
        "vscode" => Entrypoint::Vscode,
        "exec" => Entrypoint::Exec,
        "desktop" => Entrypoint::Desktop,
        other => Entrypoint::Other(other.strip_prefix("other:").unwrap_or(other).to_owned()),
    })
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
