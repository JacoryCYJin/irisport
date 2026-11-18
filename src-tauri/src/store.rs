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
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub session_id: String,
    pub source: String,
    pub prompt_excerpt: Option<String>,
    pub last_prompt_at: Option<i64>,
    pub last_result_at: Option<i64>,
    pub result_seq: i64,
    pub unread: bool,
    pub pending: bool,
    pub updated_at: i64,
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

    fn init(conn: Connection) -> StoreResult<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS sessions (
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
             CREATE TABLE IF NOT EXISTS events (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               session_id TEXT NOT NULL,
               turn_id TEXT NOT NULL,
               kind TEXT NOT NULL,
               processed_at INTEGER NOT NULL,
               UNIQUE(session_id, turn_id, kind)
             );
             CREATE TABLE IF NOT EXISTS handled_turns (
               session_id TEXT NOT NULL,
               turn_id TEXT NOT NULL,
               handled_at INTEGER NOT NULL,
               PRIMARY KEY(session_id, turn_id)
             );
             CREATE INDEX IF NOT EXISTS idx_handled_turns_at
               ON handled_turns(handled_at);
             CREATE TABLE IF NOT EXISTS meta (
               key TEXT PRIMARY KEY,
               value TEXT NOT NULL
             );
             PRAGMA user_version = 2;",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| "数据库锁不可用".to_string())
    }

    pub fn apply(&self, event: &HookEvent) -> StoreResult<bool> {
        let kind = match event.event.as_str() {
            "UserPromptSubmit" => "prompt",
            "Stop" => "stop",
            _ => return Ok(false),
        };

        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        if kind == "stop" {
            let handled: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM handled_turns WHERE session_id = ?1 AND turn_id = ?2",
                    params![event.session_id, event.turn_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            if handled.is_some() {
                tx.commit().map_err(|error| error.to_string())?;
                return Ok(false);
            }
        }

        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO events(session_id, turn_id, kind, processed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![event.session_id, event.turn_id, kind, now_ms()],
            )
            .map_err(|error| error.to_string())?;

        if inserted == 0 {
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(false);
        }

        let source = if event.source.is_empty() {
            "unknown"
        } else {
            &event.source
        };

        match kind {
            "prompt" => {
                let prompt = event.prompt.clone().unwrap_or_default();
                tx.execute(
                    "INSERT INTO sessions(
                       session_id, source, prompt_excerpt, last_prompt_at,
                       last_result_at, result_seq, unread, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, NULL, 0, 0, ?4)
                     ON CONFLICT(session_id) DO UPDATE SET
                       source = excluded.source,
                       prompt_excerpt = excluded.prompt_excerpt,
                       last_prompt_at = excluded.last_prompt_at,
                       updated_at = excluded.updated_at",
                    params![event.session_id, source, prompt, event.t],
                )
                .map_err(|error| error.to_string())?;
            }
            "stop" => {
                tx.execute(
                    "INSERT INTO sessions(
                       session_id, source, prompt_excerpt, last_prompt_at,
                       last_result_at, result_seq, unread, opened_at, updated_at
                     ) VALUES (?1, ?2, NULL, NULL, ?3, 1, 1, NULL, ?3)
                     ON CONFLICT(session_id) DO UPDATE SET
                       source = excluded.source,
                       last_result_at = excluded.last_result_at,
                       result_seq = sessions.result_seq + 1,
                       unread = 1,
                       opened_at = NULL,
                       updated_at = excluded.updated_at",
                    params![event.session_id, source, event.t],
                )
                .map_err(|error| error.to_string())?;
            }
            _ => {}
        }

        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('last_event_at', ?1)",
            params![event.t.to_string()],
        )
        .map_err(|error| error.to_string())?;

        let verified: Option<String> = tx
            .query_row(
                "SELECT value FROM meta WHERE key = 'access_verified_at'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if verified.is_none() {
            tx.execute(
                "INSERT INTO meta(key, value) VALUES ('access_verified_at', ?1)",
                params![now_ms().to_string()],
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
                "SELECT session_id, source, prompt_excerpt, last_prompt_at,
                        last_result_at, result_seq, unread, updated_at
                 FROM sessions
                 WHERE unread = 1
                 ORDER BY COALESCE(last_result_at, updated_at) DESC",
            )
            .map_err(|error| error.to_string())?;

        let rows = statement
            .query_map([], |row| {
                let last_prompt_at: Option<i64> = row.get(3)?;
                let last_result_at: Option<i64> = row.get(4)?;
                Ok(SessionView {
                    session_id: row.get(0)?,
                    source: row.get(1)?,
                    prompt_excerpt: row.get(2)?,
                    last_prompt_at,
                    last_result_at,
                    result_seq: row.get(5)?,
                    unread: row.get::<_, i64>(6)? != 0,
                    pending: last_prompt_at.unwrap_or(0) > last_result_at.unwrap_or(0),
                    updated_at: row.get(7)?,
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

    pub fn unread_session_ids(&self) -> StoreResult<Vec<String>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT session_id FROM sessions WHERE unread = 1")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn mark_seen_sessions(&self, session_ids: &[String]) -> StoreResult<usize> {
        if session_ids.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let now = now_ms();
        let mut updated = 0;
        for session_id in session_ids {
            let mut turn_ids = Vec::new();
            {
                let mut statement = tx
                    .prepare(
                        "SELECT turn_id FROM events
                         WHERE session_id = ?1 AND kind = 'stop'",
                    )
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    turn_ids.push(row.map_err(|error| error.to_string())?);
                }
            }
            for turn_id in turn_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO handled_turns(session_id, turn_id, handled_at)
                     VALUES (?1, ?2, ?3)",
                    params![session_id, turn_id, now],
                )
                .map_err(|error| error.to_string())?;
            }
            tx.execute(
                "DELETE FROM events WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|error| error.to_string())?;
            updated += tx
                .execute(
                    "UPDATE sessions SET unread = 0, opened_at = ?1
                     WHERE session_id = ?2 AND unread = 1",
                    params![now, session_id],
                )
                .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(updated)
    }

    pub fn prune_seen(&self) -> StoreResult<usize> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let cutoff = now_ms() - SEEN_RETENTION_MS;
        let mut ids = HashSet::new();

        {
            let mut statement = tx
                .prepare(
                    "SELECT session_id FROM sessions
                     WHERE unread = 0
                       AND last_result_at IS NOT NULL
                       AND COALESCE(last_prompt_at, 0) <= COALESCE(last_result_at, 0)
                       AND COALESCE(opened_at, 0) < ?1",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![cutoff], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?;
            for row in rows {
                ids.insert(row.map_err(|error| error.to_string())?);
            }
        }

        {
            let mut statement = tx
                .prepare(
                    "SELECT session_id FROM sessions
                     WHERE unread = 0
                       AND last_result_at IS NOT NULL
                       AND COALESCE(last_prompt_at, 0) <= COALESCE(last_result_at, 0)
                     ORDER BY COALESCE(opened_at, 0) DESC
                     LIMIT -1 OFFSET ?1",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![SEEN_LIMIT], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?;
            for row in rows {
                ids.insert(row.map_err(|error| error.to_string())?);
            }
        }

        for session_id in &ids {
            tx.execute(
                "DELETE FROM events WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|error| error.to_string())?;
            tx.execute(
                "DELETE FROM sessions WHERE session_id = ?1",
                params![session_id],
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
                    "SELECT session_id FROM sessions
                     WHERE unread = 0
                       AND last_result_at IS NOT NULL
                       AND COALESCE(last_prompt_at, 0) <= COALESCE(last_result_at, 0)",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?;
            for row in rows {
                seen_ids.push(row.map_err(|error| error.to_string())?);
            }
        }

        for session_id in seen_ids {
            let mut turn_ids = Vec::new();
            {
                let mut statement = tx
                    .prepare(
                        "SELECT turn_id FROM events
                         WHERE session_id = ?1 AND kind = 'stop'",
                    )
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    turn_ids.push(row.map_err(|error| error.to_string())?);
                }
            }
            for turn_id in turn_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO handled_turns(session_id, turn_id, handled_at)
                     VALUES (?1, ?2, ?3)",
                    params![session_id, turn_id, now],
                )
                .map_err(|error| error.to_string())?;
            }
            tx.execute(
                "DELETE FROM events WHERE session_id = ?1",
                params![session_id],
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

    pub fn increment_rejected(&self) -> StoreResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO meta(key, value) VALUES ('rejected_count', '1')
             ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            [],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn session_count(&self) -> StoreResult<i64> {
        let conn = self.lock()?;
        conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .map_err(|error| error.to_string())
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

    pub fn compact_database(&self) -> StoreResult<()> {
        let conn = self.lock()?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
            .map_err(|error| error.to_string())?;
        Ok(())
    }
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
        HookEvent {
            t,
            event: kind.to_string(),
            session_id: session.to_string(),
            turn_id: turn.to_string(),
            source: "desktop".to_string(),
            prompt: Some("帮我整理发布清单并且检查所有阻塞项".to_string()),
        }
    }

    fn set_opened_at(store: &Store, session: &str, value: i64) {
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE sessions SET opened_at = ?1 WHERE session_id = ?2",
                params![value, session],
            )
            .unwrap();
    }

    #[test]
    fn prompt_then_stop_shows_in_list() {
        let store = store();
        store
            .apply(&event("UserPromptSubmit", "s1", "t1", 100))
            .unwrap();
        assert!(store.list_sessions().unwrap().is_empty());
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].result_seq, 1);
    }

    #[test]
    fn seen_session_hides_but_is_retained() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        assert_eq!(
            store.mark_seen_sessions(&["s1".to_string()]).unwrap(),
            1
        );
        assert!(store.list_sessions().unwrap().is_empty());
        assert_eq!(store.session_count().unwrap(), 1);
        assert_eq!(store.count_unread().unwrap(), 0);
    }

    #[test]
    fn duplicate_stop_after_seen_is_ignored() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        store.mark_seen_sessions(&["s1".to_string()]).unwrap();
        assert!(!store.apply(&event("Stop", "s1", "t1", 300)).unwrap());
        assert!(store.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn new_turn_after_seen_reappears() {
        let store = store();
        store.apply(&event("Stop", "s1", "t1", 200)).unwrap();
        store.mark_seen_sessions(&["s1".to_string()]).unwrap();
        store
            .apply(&event("UserPromptSubmit", "s1", "t2", 300))
            .unwrap();
        assert!(store.list_sessions().unwrap().is_empty());
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
        store
            .apply(&event("Stop", "pending", "t3", 400))
            .unwrap();
        store
            .apply(&event("UserPromptSubmit", "pending", "t4", 500))
            .unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE sessions SET opened_at = 1 WHERE session_id = 'pending'",
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
                .mark_seen_sessions(&[session.clone()])
                .unwrap();
            set_opened_at(&store, &session, recent + index);
        }
        assert_eq!(store.prune_seen().unwrap(), 5);
        assert_eq!(store.session_count().unwrap(), 50);

        store
            .apply(&event("Stop", "old", "t-old", 10))
            .unwrap();
        store.mark_seen_sessions(&["old".to_string()]).unwrap();
        set_opened_at(&store, "old", now_ms() - SEEN_RETENTION_MS - 1);
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
                    "INSERT INTO handled_turns(session_id, turn_id, handled_at)
                     VALUES (?1, ?2, ?3)",
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
}
