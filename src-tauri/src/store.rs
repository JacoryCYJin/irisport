use crate::sources;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub type StoreResult<T> = Result<T, String>;

const SEEN_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const SEEN_LIMIT: i64 = 50;
const HANDLED_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const HANDLED_LIMIT: i64 = 500;
const MODEL_FAILURE_GRACE_MS: i64 = 5_000;
const MODEL_FAILURE_PREFIX: &str = "model_failure_pending:codex:";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PendingModelFailure {
    turn_id: String,
    failed_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HookEvent {
    pub t: i64,
    pub event: String,
    pub session_id: String,
    pub turn_id: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub session_title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub client_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub session_id: String,
    pub source: String,
    pub source_label: String,
    pub session_title: Option<String>,
    pub prompt_excerpt: Option<String>,
    pub last_prompt_at: Option<i64>,
    pub last_result_at: Option<i64>,
    pub result_seq: i64,
    pub unread: bool,
    pub pending: bool,
    pub state: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct SessionIdentity {
    pub source: String,
    pub session_id: String,
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: PathBuf) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let conn = Connection::open(path).map_err(|error| error.to_string())?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> StoreResult<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|error| error.to_string())?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if version < 3 && table_exists(&conn, "sessions")? {
            migrate_v3(&mut conn)?;
        } else if version == 3 {
            migrate_v4(&mut conn)?;
            migrate_v5(&mut conn)?;
            migrate_v6(&mut conn)?;
        } else if version == 4 {
            migrate_v5(&mut conn)?;
            migrate_v6(&mut conn)?;
        } else if version == 5 {
            migrate_v6(&mut conn)?;
        } else {
            create_schema(&conn)?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| "数据库锁不可用".to_string())
    }

    pub fn apply(&self, event: &HookEvent) -> StoreResult<bool> {
        let kind = match event.event.as_str() {
            "UserPromptSubmit" | "TurnStarted" => "prompt",
            "Stop" => "stop",
            "Notification" => "background",
            "StopFailure" | "Interrupt" => "end",
            "PreModelRequest" => "model_start",
            "ModelRequestFailed" => "model_failure",
            _ => return Ok(false),
        };
        let source = sources::canonical(&event.source).to_string();

        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        if matches!(kind, "stop" | "end") {
            let handled: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM handled_turns
                     WHERE source = ?1 AND session_id = ?2 AND turn_id = ?3",
                    params![source, event.session_id, event.turn_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            if handled.is_some() {
                tx.commit().map_err(|error| error.to_string())?;
                return Ok(false);
            }
        }

        if !matches!(kind, "model_start" | "model_failure") {
            let inserted = tx
                .execute(
                    "INSERT OR IGNORE INTO events(source, session_id, turn_id, kind, processed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![source, event.session_id, event.turn_id, kind, now_ms()],
                )
                .map_err(|error| error.to_string())?;

            if inserted == 0 {
                tx.commit().map_err(|error| error.to_string())?;
                return Ok(false);
            }
        }

        match kind {
            "prompt" => {
                tx.execute(
                    "INSERT INTO sessions(
                       source, session_id, session_title, prompt_excerpt, cwd, client_type,
                       last_prompt_at, last_result_at, result_seq, unread, state, active_turn_id, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, 0, 0, 'running', ?8, ?7)
                     ON CONFLICT(source, session_id) DO UPDATE SET
                       session_title = COALESCE(excluded.session_title, sessions.session_title),
                       prompt_excerpt = COALESCE(excluded.prompt_excerpt, sessions.prompt_excerpt),
                       cwd = COALESCE(excluded.cwd, sessions.cwd),
                       client_type = COALESCE(excluded.client_type, sessions.client_type),
                       last_prompt_at = excluded.last_prompt_at,
                       state = 'running',
                       active_turn_id = excluded.active_turn_id,
                       updated_at = excluded.updated_at",
                    params![
                        source,
                        event.session_id,
                        event.session_title,
                        event.prompt,
                        event.cwd,
                        event.client_type,
                        event.t,
                        event.turn_id
                    ],
                )
                .map_err(|error| error.to_string())?;
            }
            "stop" => {
                tx.execute(
                    "INSERT INTO sessions(
                       source, session_id, session_title, prompt_excerpt, cwd, client_type,
                       last_prompt_at, last_result_at, result_seq, unread, opened_at, state, active_turn_id, updated_at
                     ) VALUES (?1, ?2, ?3, NULL, ?4, ?5, NULL, ?6, 1, 1, NULL, 'completed', ?7, ?6)
                     ON CONFLICT(source, session_id) DO UPDATE SET
                       session_title = COALESCE(excluded.session_title, sessions.session_title),
                       cwd = COALESCE(excluded.cwd, sessions.cwd),
                       client_type = COALESCE(excluded.client_type, sessions.client_type),
                       last_result_at = excluded.last_result_at,
                       result_seq = sessions.result_seq + 1,
                       unread = 1,
                       opened_at = NULL,
                       state = 'completed',
                       active_turn_id = excluded.active_turn_id,
                       updated_at = excluded.updated_at
                     WHERE sessions.state = 'running'
                       AND (sessions.active_turn_id IS NULL
                            OR sessions.active_turn_id = excluded.active_turn_id
                            OR sessions.source IN ('kimi-code', 'workbuddy'))",
                    params![
                        source,
                        event.session_id,
                        event.session_title,
                        event.cwd,
                        event.client_type,
                        event.t,
                        event.turn_id
                    ],
                )
                .map_err(|error| error.to_string())?;
                if matches!(source.as_str(), sources::WORKBUDDY | sources::KIMI_CODE) {
                    tx.execute(
                        "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, '0')",
                        params![external_unread_key(&source, &event.session_id)],
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
            "end" => {
                let state = if event.event == "Interrupt" {
                    "interrupted"
                } else {
                    "failed"
                };
                tx.execute(
                    "INSERT INTO sessions(
                       source, session_id, session_title, cwd, client_type, state, active_turn_id, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?8, ?7)
                     ON CONFLICT(source, session_id) DO UPDATE SET
                       state = excluded.state,
                       active_turn_id = excluded.active_turn_id,
                       updated_at = excluded.updated_at
                     WHERE sessions.state = 'running'
                       AND (sessions.active_turn_id IS NULL
                            OR sessions.active_turn_id = excluded.active_turn_id
                            OR sessions.source = 'kimi-code')",
                    params![
                        source,
                        event.session_id,
                        event.session_title,
                        event.cwd,
                        event.client_type,
                        state,
                        event.t,
                        event.turn_id
                    ],
                )
                .map_err(|error| error.to_string())?;
            }
            "model_start" => {
                if current_turn_id(&tx, &source, &event.session_id)? == Some(event.turn_id.clone())
                {
                    tx.execute(
                        "DELETE FROM meta WHERE key = ?1",
                        params![model_failure_key(&source, &event.session_id)],
                    )
                    .map_err(|error| error.to_string())?;
                    tx.execute(
                        "UPDATE sessions SET state = 'running', updated_at = ?1
                         WHERE source = ?2 AND session_id = ?3
                           AND state IN ('running', 'failed')",
                        params![event.t, source, event.session_id],
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
            "model_failure" => {
                let running: Option<i64> = tx
                    .query_row(
                        "SELECT 1 FROM sessions
                         WHERE source = ?1 AND session_id = ?2 AND state = 'running'",
                        params![source, event.session_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| error.to_string())?;
                if running.is_some()
                    && current_turn_id(&tx, &source, &event.session_id)?
                        == Some(event.turn_id.clone())
                {
                    let pending = PendingModelFailure {
                        turn_id: event.turn_id.clone(),
                        failed_at: event.t,
                    };
                    tx.execute(
                        "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
                        params![
                            model_failure_key(&source, &event.session_id),
                            serde_json::to_string(&pending).map_err(|error| error.to_string())?
                        ],
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
            _ => {}
        }

        if matches!(kind, "prompt" | "stop" | "end") {
            tx.execute(
                "DELETE FROM meta WHERE key = ?1",
                params![model_failure_key(&source, &event.session_id)],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![format!("last_event_at:{source}"), event.t.to_string()],
        )
        .map_err(|error| error.to_string())?;

        let verified_key = format!("access_verified_at:{source}");
        let verified: Option<String> = tx
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![verified_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if verified.is_none() {
            tx.execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2)",
                params![verified_key, now_ms().to_string()],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        Ok(true)
    }

    pub fn list_sessions(&self) -> StoreResult<Vec<SessionView>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT source, session_id, session_title, prompt_excerpt, last_prompt_at,
                        last_result_at, result_seq, unread, state, updated_at
                 FROM sessions
                 WHERE state = 'running' OR (unread = 1 AND state != 'running')
                 ORDER BY updated_at DESC",
            )
            .map_err(|error| error.to_string())?;

        let rows = statement
            .query_map([], |row| {
                let last_prompt_at: Option<i64> = row.get(4)?;
                let last_result_at: Option<i64> = row.get(5)?;
                let state: String = row.get(8)?;
                let source: String = row.get(0)?;
                let source_label = sources::label(&source).to_string();
                Ok(SessionView {
                    source: source.clone(),
                    session_id: row.get(1)?,
                    source_label,
                    session_title: row.get(2)?,
                    prompt_excerpt: row.get(3)?,
                    last_prompt_at,
                    last_result_at,
                    result_seq: row.get(6)?,
                    unread: row.get::<_, i64>(7)? != 0,
                    pending: state == "running",
                    state,
                    updated_at: row.get(9)?,
                })
            })
            .map_err(|error| error.to_string())?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn count_unread(&self) -> StoreResult<i64> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE unread = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
    }

    pub fn unread_sessions(&self) -> StoreResult<Vec<(String, String)>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT source, session_id FROM sessions WHERE unread = 1")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn session_identity(
        &self,
        source: &str,
        session_id: &str,
    ) -> StoreResult<Option<SessionIdentity>> {
        let source = sources::canonical(source);
        let conn = self.lock()?;
        conn.query_row(
            "SELECT 1 FROM sessions WHERE source = ?1 AND session_id = ?2",
            params![source, session_id],
            |_| {
                Ok(SessionIdentity {
                    source: source.to_string(),
                    session_id: session_id.to_string(),
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    pub fn mark_interrupted_session(
        &self,
        source: &str,
        session_id: &str,
        ended_at: i64,
    ) -> StoreResult<bool> {
        let conn = self.lock()?;
        let updated = conn
            .execute(
                "UPDATE sessions SET state = 'interrupted', updated_at = ?1
                 WHERE source = ?2 AND session_id = ?3 AND state = 'running'
                   AND COALESCE(last_prompt_at, 0) <= ?1",
                params![ended_at, sources::canonical(source), session_id],
            )
            .map_err(|error| error.to_string())?;
        Ok(updated > 0)
    }

    pub fn complete_running_session_from_external(
        &self,
        source: &str,
        session_id: &str,
        ended_at: i64,
    ) -> StoreResult<Option<SessionView>> {
        let source = sources::canonical(source).to_string();
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let updated = tx
            .execute(
                "UPDATE sessions SET
                   state = 'completed', last_result_at = ?1, result_seq = result_seq + 1,
                   unread = 1, opened_at = NULL, updated_at = ?1
                 WHERE source = ?2 AND session_id = ?3 AND state = 'running'
                   AND COALESCE(last_prompt_at, 0) <= ?1",
                params![ended_at, source, session_id],
            )
            .map_err(|error| error.to_string())?;
        if updated > 0 {
            tx.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, '0')",
                params![external_unread_key(&source, session_id)],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        if updated == 0 {
            return Ok(None);
        }
        drop(conn);
        Ok(self.list_sessions()?.into_iter().find(|session| {
            session.source == source
                && session.session_id == session_id
                && session.state == "completed"
        }))
    }

    pub fn mark_seen_sessions(&self, sessions: &[(String, String)]) -> StoreResult<usize> {
        self.mark_seen_sessions_inner(sessions, false, None)
    }

    pub fn mark_observed_session(&self, source: &str, session_id: &str) -> StoreResult<bool> {
        let sessions = vec![(source.to_string(), session_id.to_string())];
        Ok(self.mark_seen_sessions_inner(&sessions, true, None)? == 1)
    }

    pub fn mark_observed_session_turn(
        &self,
        source: &str,
        session_id: &str,
        started_at: i64,
    ) -> StoreResult<bool> {
        let sessions = vec![(source.to_string(), session_id.to_string())];
        Ok(self.mark_seen_sessions_inner(&sessions, true, Some(started_at))? == 1)
    }

    fn mark_seen_sessions_inner(
        &self,
        sessions: &[(String, String)],
        require_observed_unread: bool,
        expected_started_at: Option<i64>,
    ) -> StoreResult<usize> {
        if sessions.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let now = now_ms();
        let mut updated = 0;
        for (source, session_id) in sessions {
            let source = sources::canonical(source);
            if require_observed_unread {
                let observed: Option<String> = tx
                    .query_row(
                        "SELECT value FROM meta WHERE key = ?1",
                        params![external_unread_key(source, session_id)],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| error.to_string())?;
                if observed.as_deref() != Some("1") {
                    continue;
                }
                let ready: Option<i64> = tx
                    .query_row(
                        "SELECT 1 FROM sessions
                         WHERE source = ?1 AND session_id = ?2
                           AND unread = 1 AND state != 'running'",
                        params![source, session_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| error.to_string())?;
                if ready.is_none() {
                    continue;
                }
                if let Some(expected) = expected_started_at {
                    let matches_turn: Option<i64> = tx
                        .query_row(
                            "SELECT 1 FROM sessions
                             WHERE source = ?1 AND session_id = ?2 AND last_prompt_at = ?3",
                            params![source, session_id, expected],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(|error| error.to_string())?;
                    if matches_turn.is_none() {
                        continue;
                    }
                }
            }
            let mut turn_ids = Vec::new();
            {
                let mut statement = tx
                    .prepare(
                        "SELECT turn_id FROM events
                         WHERE source = ?1 AND session_id = ?2 AND kind = 'stop'",
                    )
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![source, session_id], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    turn_ids.push(row.map_err(|error| error.to_string())?);
                }
            }
            for turn_id in turn_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO handled_turns(source, session_id, turn_id, handled_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![source, session_id, turn_id, now],
                )
                .map_err(|error| error.to_string())?;
            }
            tx.execute(
                "DELETE FROM events WHERE source = ?1 AND session_id = ?2",
                params![source, session_id],
            )
            .map_err(|error| error.to_string())?;
            let changed = tx
                .execute(
                    "UPDATE sessions SET unread = 0, opened_at = ?1
                     WHERE source = ?2 AND session_id = ?3
                       AND unread = 1 AND state != 'running'",
                    params![now, source, session_id],
                )
                .map_err(|error| error.to_string())?;
            if require_observed_unread && changed > 0 {
                tx.execute(
                    "DELETE FROM meta WHERE key = ?1",
                    params![external_unread_key(source, session_id)],
                )
                .map_err(|error| error.to_string())?;
            }
            updated += changed;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(updated)
    }

    pub fn finalize_model_failures(&self, now: i64) -> StoreResult<usize> {
        let candidates = {
            let conn = self.lock()?;
            let mut statement = conn
                .prepare("SELECT key, value FROM meta WHERE key LIKE ?1")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![format!("{MODEL_FAILURE_PREFIX}%")], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };

        let mut finalized = 0;
        for (key, raw) in candidates {
            let Ok(pending) = serde_json::from_str::<PendingModelFailure>(&raw) else {
                continue;
            };
            if now - pending.failed_at < MODEL_FAILURE_GRACE_MS {
                continue;
            }
            let Some(session_id) = key.strip_prefix(MODEL_FAILURE_PREFIX) else {
                continue;
            };

            let mut conn = self.lock()?;
            let tx = conn.transaction().map_err(|error| error.to_string())?;
            let current: Option<String> = tx
                .query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            if current.as_deref() != Some(raw.as_str())
                || current_turn_id(&tx, sources::CODEX, session_id)?
                    != Some(pending.turn_id.clone())
            {
                tx.commit().map_err(|error| error.to_string())?;
                continue;
            }
            let updated = tx
                .execute(
                    "UPDATE sessions SET state = 'failed', updated_at = ?1
                     WHERE source = ?2 AND session_id = ?3 AND state = 'running'",
                    params![pending.failed_at, sources::CODEX, session_id],
                )
                .map_err(|error| error.to_string())?;
            if updated > 0 {
                tx.execute("DELETE FROM meta WHERE key = ?1", params![key])
                    .map_err(|error| error.to_string())?;
                finalized += 1;
            }
            tx.commit().map_err(|error| error.to_string())?;
        }
        Ok(finalized)
    }

    pub fn prune_seen(&self) -> StoreResult<usize> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let cutoff = now_ms() - SEEN_RETENTION_MS;
        let mut ids = HashSet::new();

        {
            let mut statement = tx
                .prepare(
                    "SELECT source, session_id FROM sessions
                     WHERE unread = 0
                       AND last_result_at IS NOT NULL
                       AND COALESCE(last_prompt_at, 0) <= COALESCE(last_result_at, 0)
                       AND COALESCE(opened_at, 0) < ?1",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![cutoff], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                ids.insert(row.map_err(|error| error.to_string())?);
            }
        }

        {
            let mut statement = tx
                .prepare(
                    "SELECT source, session_id FROM sessions
                     WHERE unread = 0
                       AND last_result_at IS NOT NULL
                       AND COALESCE(last_prompt_at, 0) <= COALESCE(last_result_at, 0)
                     ORDER BY COALESCE(opened_at, 0) DESC
                     LIMIT -1 OFFSET ?1",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![SEEN_LIMIT], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                ids.insert(row.map_err(|error| error.to_string())?);
            }
        }

        for (source, session_id) in &ids {
            tx.execute(
                "DELETE FROM events WHERE source = ?1 AND session_id = ?2",
                params![source, session_id],
            )
            .map_err(|error| error.to_string())?;
            tx.execute(
                "DELETE FROM sessions WHERE source = ?1 AND session_id = ?2",
                params![source, session_id],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        Ok(ids.len())
    }

    pub fn prune_handled(&self) -> StoreResult<usize> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let cutoff = now_ms() - HANDLED_RETENTION_MS;
        let mut removed = tx
            .execute(
                "DELETE FROM handled_turns WHERE handled_at < ?1",
                params![cutoff],
            )
            .map_err(|error| error.to_string())?;
        removed += tx
            .execute(
                "DELETE FROM handled_turns WHERE rowid IN (
                   SELECT rowid FROM handled_turns
                   ORDER BY handled_at DESC
                   LIMIT -1 OFFSET ?1
                 )",
                params![HANDLED_LIMIT],
            )
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(removed)
    }

    pub fn migrate_seen_events(&self) -> StoreResult<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let now = now_ms();

        tx.execute(
            "UPDATE sessions SET opened_at = ?1
             WHERE unread = 0 AND last_result_at IS NOT NULL AND opened_at IS NULL",
            params![now],
        )
        .map_err(|error| error.to_string())?;

        let mut seen_ids = Vec::new();
        {
            let mut statement = tx
                .prepare(
                    "SELECT source, session_id FROM sessions
                     WHERE unread = 0 AND last_result_at IS NOT NULL
                       AND COALESCE(last_prompt_at, 0) <= COALESCE(last_result_at, 0)",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                seen_ids.push(row.map_err(|error| error.to_string())?);
            }
        }

        for (source, session_id) in seen_ids {
            let mut turn_ids = Vec::new();
            {
                let mut statement = tx
                    .prepare(
                        "SELECT turn_id FROM events
                         WHERE source = ?1 AND session_id = ?2 AND kind = 'stop'",
                    )
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![source, session_id], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    turn_ids.push(row.map_err(|error| error.to_string())?);
                }
            }
            for turn_id in turn_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO handled_turns(source, session_id, turn_id, handled_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![source, session_id, turn_id, now],
                )
                .map_err(|error| error.to_string())?;
            }
            tx.execute(
                "DELETE FROM events WHERE source = ?1 AND session_id = ?2",
                params![source, session_id],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn storage_version(&self) -> StoreResult<i64> {
        Ok(self
            .meta_get("storage_version")?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(1))
    }

    pub fn set_storage_version(&self, version: i64) -> StoreResult<()> {
        self.meta_set("storage_version", &version.to_string())
    }

    pub fn increment_rejected(&self, source: &str) -> StoreResult<()> {
        let source = sources::canonical(source);
        let key = format!("rejected_count:{source}");
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            params![key],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[cfg(test)]
    pub fn session_count(&self) -> StoreResult<i64> {
        let conn = self.lock()?;
        conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .map_err(|error| error.to_string())
    }

    pub fn source_meta_get(&self, source: &str, key: &str) -> StoreResult<Option<String>> {
        let source = sources::canonical(source);
        let value = self.meta_get(&format!("{key}:{source}"))?;
        if value.is_none() && source == sources::CODEX {
            return self.meta_get(key);
        }
        Ok(value)
    }

    pub fn meta_get(&self, key: &str) -> StoreResult<Option<String>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    pub fn meta_set(&self, key: &str, value: &str) -> StoreResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn meta_remove(&self, key: &str) -> StoreResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM meta WHERE key = ?1", params![key])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn preference_bool(&self, key: &str, default: bool) -> bool {
        self.meta_get(key)
            .ok()
            .flatten()
            .map(|value| value == "true")
            .unwrap_or(default)
    }

    pub fn compact_database(&self) -> StoreResult<()> {
        let conn = self.lock()?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn model_failure_key(source: &str, session_id: &str) -> String {
    format!(
        "model_failure_pending:{}:{session_id}",
        sources::canonical(source)
    )
}

fn current_turn_id(
    tx: &rusqlite::Transaction,
    source: &str,
    session_id: &str,
) -> StoreResult<Option<String>> {
    tx.query_row(
        "SELECT active_turn_id FROM sessions WHERE source = ?1 AND session_id = ?2",
        params![source, session_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn external_unread_key(source: &str, session_id: &str) -> String {
    format!(
        "external_unread_observed:{}:{session_id}",
        sources::canonical(source)
    )
}

fn table_exists(conn: &Connection, name: &str) -> StoreResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |row| row.get(0),
    )
    .map_err(|error| error.to_string())
}

fn create_schema(conn: &Connection) -> StoreResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
           source TEXT NOT NULL,
           session_id TEXT NOT NULL,
           session_title TEXT,
           prompt_excerpt TEXT,
           cwd TEXT,
           client_type TEXT,
           last_prompt_at INTEGER,
           last_result_at INTEGER,
           result_seq INTEGER NOT NULL DEFAULT 0,
           unread INTEGER NOT NULL DEFAULT 0,
           opened_at INTEGER,
           dismissed_at INTEGER,
           state TEXT NOT NULL DEFAULT 'completed',
           active_turn_id TEXT,
           activity TEXT,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(source, session_id)
         );
         CREATE TABLE IF NOT EXISTS events (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           source TEXT NOT NULL,
           session_id TEXT NOT NULL,
           turn_id TEXT NOT NULL,
           kind TEXT NOT NULL,
           processed_at INTEGER NOT NULL,
           UNIQUE(source, session_id, turn_id, kind)
         );
         CREATE TABLE IF NOT EXISTS handled_turns (
           source TEXT NOT NULL,
           session_id TEXT NOT NULL,
           turn_id TEXT NOT NULL,
           handled_at INTEGER NOT NULL,
           PRIMARY KEY(source, session_id, turn_id)
         );
         CREATE INDEX IF NOT EXISTS idx_handled_turns_at
           ON handled_turns(handled_at);
         CREATE TABLE IF NOT EXISTS meta (
           key TEXT PRIMARY KEY,
           value TEXT NOT NULL
         );
         PRAGMA user_version = 6;",
    )
    .map_err(|error| error.to_string())
}

fn migrate_v3(conn: &mut Connection) -> StoreResult<()> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    tx.execute_batch(
        "ALTER TABLE sessions RENAME TO sessions_v2;
         ALTER TABLE events RENAME TO events_v2;
         ALTER TABLE handled_turns RENAME TO handled_turns_v2;",
    )
    .map_err(|error| error.to_string())?;
    create_schema(&tx)?;
    tx.execute_batch(
        "INSERT INTO sessions(
           source, session_id, prompt_excerpt, last_prompt_at, last_result_at,
           result_seq, unread, opened_at, updated_at
         )
         SELECT CASE WHEN source = 'desktop' THEN 'codex' ELSE source END,
                session_id, prompt_excerpt, last_prompt_at, last_result_at,
                result_seq, unread, opened_at, updated_at
         FROM sessions_v2;
         INSERT INTO events(source, session_id, turn_id, kind, processed_at)
         SELECT 'codex', session_id, turn_id, kind, processed_at FROM events_v2;
         INSERT INTO handled_turns(source, session_id, turn_id, handled_at)
         SELECT 'codex', session_id, turn_id, handled_at FROM handled_turns_v2;
         INSERT OR REPLACE INTO meta(key, value)
         SELECT 'last_event_at:codex', value FROM meta WHERE key = 'last_event_at';
         INSERT OR REPLACE INTO meta(key, value)
         SELECT 'access_verified_at:codex', value FROM meta WHERE key = 'access_verified_at';
         INSERT OR REPLACE INTO meta(key, value)
         SELECT 'rejected_count:codex', value FROM meta WHERE key = 'rejected_count';
         UPDATE sessions SET state = 'running', activity = '正在处理当前请求'
         WHERE COALESCE(last_prompt_at, 0) > COALESCE(last_result_at, 0);
         DROP TABLE sessions_v2;
         DROP TABLE events_v2;
         DROP TABLE handled_turns_v2;",
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate_v4(conn: &mut Connection) -> StoreResult<()> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    tx.execute_batch(
        "ALTER TABLE sessions ADD COLUMN state TEXT NOT NULL DEFAULT 'completed';
         ALTER TABLE sessions ADD COLUMN activity TEXT;
         UPDATE sessions SET state = 'running', activity = '正在处理当前请求'
         WHERE COALESCE(last_prompt_at, 0) > COALESCE(last_result_at, 0);
         PRAGMA user_version = 4;",
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate_v5(conn: &mut Connection) -> StoreResult<()> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    tx.execute_batch(
        "ALTER TABLE sessions ADD COLUMN dismissed_at INTEGER;
         PRAGMA user_version = 5;",
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn migrate_v6(conn: &mut Connection) -> StoreResult<()> {
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    tx.execute_batch(
        "ALTER TABLE sessions ADD COLUMN active_turn_id TEXT;
         UPDATE sessions SET active_turn_id = (
           SELECT turn_id FROM events
           WHERE events.source = sessions.source
             AND events.session_id = sessions.session_id
             AND kind = 'prompt'
           ORDER BY processed_at DESC, rowid DESC LIMIT 1
         );
         PRAGMA user_version = 6;",
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::init(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn event(kind: &str, session: &str, turn: &str, t: i64) -> HookEvent {
        source_event("desktop", kind, session, turn, t)
    }

    fn source_event(source: &str, kind: &str, session: &str, turn: &str, t: i64) -> HookEvent {
        HookEvent {
            t,
            event: kind.to_string(),
            session_id: session.to_string(),
            turn_id: turn.to_string(),
            source: source.to_string(),
            prompt: Some("帮我整理发布清单并且检查所有阻塞项".to_string()),
            session_title: Some("整理发布清单".to_string()),
            cwd: Some("/tmp/project".to_string()),
            client_type: Some("test".to_string()),
        }
    }

    fn seen(source: &str, session: &str) -> (String, String) {
        (source.to_string(), session.to_string())
    }

    fn set_opened_at(store: &Store, source: &str, session: &str, value: i64) {
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE sessions SET opened_at = ?1 WHERE source = ?2 AND session_id = ?3",
                params![value, source, session],
            )
            .unwrap();
    }

    #[test]
    fn prompt_then_stop_shows_in_list() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        let running = store.list_sessions().unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].state, "running");
        assert!(!running[0].unread);
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].result_seq, 1);
        assert_eq!(sessions[0].source, "codex");
        assert_eq!(sessions[0].session_title.as_deref(), Some("整理发布清单"));
    }

    #[test]
    fn same_session_id_from_two_sources_stays_separate() {
        let store = store();
        store.apply(&event("Stop", "shared", "c1", 100)).unwrap();
        store
            .apply(&source_event("kimi-code", "Stop", "shared", "k1", 200))
            .unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        store
            .mark_seen_sessions(&[seen("kimi-code", "shared")])
            .unwrap();
        let remaining = store.list_sessions().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].source, "codex");
    }

    #[test]
    fn interrupted_session_stops_running_without_becoming_a_result() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store.apply(&event("Interrupt", "s1", "t1", 200)).unwrap();
        assert!(store.list_sessions().unwrap().is_empty());
        assert_eq!(store.count_unread().unwrap(), 0);
    }

    #[test]
    fn running_session_cannot_be_marked_seen() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE sessions SET unread = 1 WHERE source = 'codex' AND session_id = 's1'",
                [],
            )
            .unwrap();
        assert_eq!(store.mark_seen_sessions(&[seen("codex", "s1")]).unwrap(), 0);
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, "running");
        assert!(sessions[0].unread);
    }

    #[test]
    fn model_failure_finalizes_only_after_grace() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("ModelRequestFailed", "s1", "t1", 200))
            .unwrap();
        assert_eq!(store.finalize_model_failures(5_199).unwrap(), 0);
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
        assert_eq!(store.finalize_model_failures(5_200).unwrap(), 1);
        assert!(store.list_sessions().unwrap().is_empty());
        assert_eq!(store.count_unread().unwrap(), 0);
    }

    #[test]
    fn new_model_attempt_cancels_pending_failure() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("ModelRequestFailed", "s1", "t1", 200))
            .unwrap();
        store
            .apply(&event("PreModelRequest", "s1", "t1", 300))
            .unwrap();
        assert_eq!(store.finalize_model_failures(6_000).unwrap(), 0);
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
    }

    #[test]
    fn normal_stop_clears_pending_failure() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("ModelRequestFailed", "s1", "t1", 200))
            .unwrap();
        store.apply(&event("Stop", "s1", "t1", 300)).unwrap();
        assert_eq!(store.finalize_model_failures(6_000).unwrap(), 0);
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions[0].state, "completed");
        assert!(sessions[0].unread);
    }

    #[test]
    fn delayed_retry_restores_a_finalized_failure() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("ModelRequestFailed", "s1", "t1", 200))
            .unwrap();
        store.finalize_model_failures(5_200).unwrap();
        store
            .apply(&event("PreModelRequest", "s1", "t1", 5_300))
            .unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
    }

    #[test]
    fn previous_turn_failure_does_not_end_a_new_turn() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("UserPromptSubmit", "s1", "t2", 200))
            .unwrap();
        store
            .apply(&event("ModelRequestFailed", "s1", "t1", 300))
            .unwrap();
        assert_eq!(store.finalize_model_failures(6_000).unwrap(), 0);
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
    }

    #[test]
    fn previous_turn_stop_does_not_complete_the_new_turn() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("UserPromptSubmit", "s1", "t2", 200))
            .unwrap();
        store.apply(&event("Stop", "s1", "t1", 300)).unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
        store.apply(&event("Stop", "s1", "t2", 400)).unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].state, "completed");
    }

    #[test]
    fn previous_turn_interrupt_does_not_cancel_the_new_turn() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&event("UserPromptSubmit", "s1", "t2", 200))
            .unwrap();
        store.apply(&event("Interrupt", "s1", "t1", 300)).unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
    }

    #[test]
    fn kimi_background_notification_does_not_complete_the_main_turn() {
        let store = store();
        store
            .apply(&source_event("kimi-code", "TurnStarted", "s1", "t1", 100))
            .unwrap();
        store
            .apply(&source_event(
                "kimi-code",
                "Notification",
                "s1",
                "task-1",
                200,
            ))
            .unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions[0].state, "running");
        assert_eq!(sessions[0].result_seq, 0);
        assert!(!sessions[0].unread);
    }

    #[test]
    fn workbuddy_stop_is_accepted_without_a_source_turn_id() {
        let store = store();
        store
            .apply(&source_event(
                "workbuddy",
                "UserPromptSubmit",
                "s1",
                "prompt-1",
                100,
            ))
            .unwrap();
        store
            .apply(&source_event("workbuddy", "Stop", "s1", "stop-1", 200))
            .unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].state, "completed");
    }

    #[test]
    fn kimi_stop_is_not_blocked_by_generated_turn_ids() {
        let store = store();
        store
            .apply(&source_event(
                "kimi-code",
                "TurnStarted",
                "s1",
                "turnstarted-100",
                100,
            ))
            .unwrap();
        store
            .apply(&source_event("kimi-code", "Stop", "s1", "stop-200", 200))
            .unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].state, "completed");
    }

    #[test]
    fn seen_session_hides_but_is_retained() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        assert_eq!(store.mark_seen_sessions(&[seen("codex", "s1")]).unwrap(), 1);
        assert!(store.list_sessions().unwrap().is_empty());
        assert_eq!(store.session_count().unwrap(), 1);
        assert_eq!(store.count_unread().unwrap(), 0);
    }

    #[test]
    fn duplicate_stop_after_seen_is_ignored() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        store.mark_seen_sessions(&[seen("codex", "s1")]).unwrap();
        assert!(!store.apply(&event("Stop", "s1", "t1", 300)).unwrap());
        assert!(store.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn new_turn_after_seen_reappears() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        store.mark_seen_sessions(&[seen("codex", "s1")]).unwrap();
        store
            .apply(&event("UserPromptSubmit", "s1", "t2", 300))
            .unwrap();
        let running = store.list_sessions().unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].state, "running");
        assert!(!running[0].unread);
        store.apply(&event("Stop", "s1", "t2", 400)).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].result_seq, 2);
    }

    #[test]
    fn prune_keeps_unread_and_pending() {
        let store = store();
        store.apply(&event("Stop", "unread", "t1", 200)).unwrap();
        store
            .apply(&event("UserPromptSubmit", "pending", "t2", 300))
            .unwrap();
        store.apply(&event("Stop", "pending", "t3", 400)).unwrap();
        store
            .apply(&event("UserPromptSubmit", "pending", "t4", 500))
            .unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE sessions SET opened_at = 1 WHERE source = 'codex' AND session_id = 'pending'",
                [],
            )
            .unwrap();
        assert_eq!(store.prune_seen().unwrap(), 0);
        assert_eq!(store.session_count().unwrap(), 2);
    }

    #[test]
    fn prune_seen_removes_old_and_excess() {
        let store = store();
        let recent = now_ms() - 100_000;
        for index in 0..55 {
            let session = format!("s{index}");
            store
                .apply(&event("Stop", &session, &format!("t{index}"), 1000 + index))
                .unwrap();
            store
                .mark_seen_sessions(&[seen("codex", &session)])
                .unwrap();
            set_opened_at(&store, "codex", &session, recent + index);
        }
        assert_eq!(store.prune_seen().unwrap(), 5);
        assert_eq!(store.session_count().unwrap(), 50);

        store.apply(&event("Stop", "old", "t-old", 10)).unwrap();
        store.mark_seen_sessions(&[seen("codex", "old")]).unwrap();
        set_opened_at(&store, "codex", "old", now_ms() - SEEN_RETENTION_MS - 1);
        assert_eq!(store.prune_seen().unwrap(), 1);
        assert!(!store
            .list_sessions()
            .unwrap()
            .iter()
            .any(|row| row.session_id == "old"));
    }

    #[test]
    fn list_includes_more_than_fifty_unread() {
        let store = store();
        for index in 0..60 {
            store
                .apply(&event(
                    "Stop",
                    &format!("s{index}"),
                    &format!("t{index}"),
                    1000 + index,
                ))
                .unwrap();
        }
        assert_eq!(store.list_sessions().unwrap().len(), 60);
    }

    #[test]
    fn handled_turns_are_capped() {
        let store = store();
        let recent = now_ms() - 100_000;
        {
            let conn = store.lock().unwrap();
            for index in 0..510 {
                conn.execute(
                    "INSERT INTO handled_turns(source, session_id, turn_id, handled_at)
                     VALUES ('codex', ?1, ?2, ?3)",
                    params![format!("s{index}"), format!("t{index}"), recent + index],
                )
                .unwrap();
            }
        }
        store.prune_handled().unwrap();
        let count: i64 = store
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM handled_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 500);
    }

    #[test]
    fn legacy_codex_rows_survive_source_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
               session_id TEXT PRIMARY KEY,
               source TEXT NOT NULL,
               prompt_excerpt TEXT,
               last_prompt_at INTEGER,
               last_result_at INTEGER,
               result_seq INTEGER NOT NULL DEFAULT 0,
               unread INTEGER NOT NULL DEFAULT 0,
               opened_at INTEGER,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               session_id TEXT NOT NULL,
               turn_id TEXT NOT NULL,
               kind TEXT NOT NULL,
               processed_at INTEGER NOT NULL,
               UNIQUE(session_id, turn_id, kind)
             );
             CREATE TABLE handled_turns (
               session_id TEXT NOT NULL,
               turn_id TEXT NOT NULL,
               handled_at INTEGER NOT NULL,
               PRIMARY KEY(session_id, turn_id)
             );
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             PRAGMA user_version = 2;
             INSERT INTO sessions(
               session_id, source, prompt_excerpt, last_result_at,
               result_seq, unread, updated_at
             ) VALUES ('legacy', 'desktop', '旧任务', 100, 1, 1, 100);
             INSERT INTO meta VALUES ('last_event_at', '100');",
        )
        .unwrap();
        let store = Store::init(conn).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].source, "codex");
        assert_eq!(sessions[0].prompt_excerpt.as_deref(), Some("旧任务"));
        assert_eq!(
            store.source_meta_get("codex", "last_event_at").unwrap(),
            Some("100".to_string())
        );
    }

    #[test]
    fn v5_running_session_gets_its_turn_id_during_migration() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute_batch(
            "ALTER TABLE sessions DROP COLUMN active_turn_id;
             PRAGMA user_version = 5;
             INSERT INTO sessions(
               source, session_id, last_prompt_at, state, updated_at
             ) VALUES ('codex', 's1', 100, 'running', 100);
             INSERT INTO events(source, session_id, turn_id, kind, processed_at)
             VALUES ('codex', 's1', 't1', 'prompt', 100);",
        )
        .unwrap();
        let store = Store::init(conn).unwrap();
        let active: Option<String> = store
            .lock()
            .unwrap()
            .query_row(
                "SELECT active_turn_id FROM sessions WHERE source = 'codex' AND session_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active.as_deref(), Some("t1"));
    }

    #[test]
    fn source_status_uses_namespaced_metadata() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        store
            .apply(&source_event("kimi-code", "Stop", "s2", "k1", 400))
            .unwrap();
        assert_eq!(
            store.source_meta_get("codex", "last_event_at").unwrap(),
            Some("200".to_string())
        );
        assert_eq!(
            store.source_meta_get("kimi-code", "last_event_at").unwrap(),
            Some("400".to_string())
        );
    }
}
