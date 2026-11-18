use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub type StoreResult<T> = Result<T, String>;

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
             CREATE TABLE IF NOT EXISTS meta (
               key TEXT PRIMARY KEY,
               value TEXT NOT NULL
             );
             PRAGMA user_version = 1;",
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
                       last_result_at, result_seq, unread, updated_at
                     ) VALUES (?1, ?2, NULL, NULL, ?3, 1, 1, ?3)
                     ON CONFLICT(session_id) DO UPDATE SET
                       source = excluded.source,
                       last_result_at = excluded.last_result_at,
                       result_seq = sessions.result_seq + 1,
                       unread = 1,
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
                 ORDER BY unread DESC,
                          COALESCE(last_result_at, last_prompt_at, updated_at) DESC
                 LIMIT 50",
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

    pub fn session_state(&self, session_id: &str) -> StoreResult<Option<(bool, i64)>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT unread, result_seq FROM sessions WHERE session_id = ?1",
            params![session_id],
            |row| Ok((row.get::<_, i64>(0)? != 0, row.get(1)?)),
        )
        .optional()
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

    pub fn mark_all_seen(&self) -> StoreResult<usize> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE sessions SET unread = 0, opened_at = ?1 WHERE unread = 1",
            params![now_ms()],
        )
        .map_err(|error| error.to_string())
    }

    pub fn clear_if_version(&self, session_id: &str, expected_seq: i64) -> StoreResult<bool> {
        let conn = self.lock()?;
        let updated = conn
            .execute(
                "UPDATE sessions SET unread = 0, opened_at = ?1
                 WHERE session_id = ?2 AND result_seq = ?3 AND unread = 1",
                params![now_ms(), session_id, expected_seq],
            )
            .map_err(|error| error.to_string())?;
        Ok(updated > 0)
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

    fn event(kind: &str, turn: &str, t: i64) -> HookEvent {
        HookEvent {
            t,
            event: kind.to_string(),
            session_id: "s1".to_string(),
            turn_id: turn.to_string(),
            source: "desktop".to_string(),
            prompt: Some("帮我整理发布清单并且检查所有阻塞项".to_string()),
        }
    }

    #[test]
    fn prompt_then_stop_marks_unread() {
        let store = store();
        assert!(store.apply(&event("UserPromptSubmit", "t1", 100)).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        assert!(store.apply(&event("Stop", "t1", 200)).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].result_seq, 1);
        assert!(!sessions[0].pending);
    }

    #[test]
    fn duplicate_stop_is_ignored() {
        let store = store();
        store.apply(&event("Stop", "t1", 200)).unwrap();
        assert!(!store.apply(&event("Stop", "t1", 300)).unwrap());
        assert_eq!(store.list_sessions().unwrap()[0].result_seq, 1);
    }

    #[test]
    fn second_turn_increments_result_version() {
        let store = store();
        store.apply(&event("Stop", "t1", 200)).unwrap();
        store.apply(&event("UserPromptSubmit", "t2", 300)).unwrap();
        store.apply(&event("Stop", "t2", 400)).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions[0].result_seq, 2);
        assert!(sessions[0].unread);
    }

    #[test]
    fn clear_only_matches_clicked_version() {
        let store = store();
        store.apply(&event("Stop", "t1", 200)).unwrap();
        assert!(store.clear_if_version("s1", 1).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        store.apply(&event("Stop", "t2", 300)).unwrap();
        assert!(!store.clear_if_version("s1", 1).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
    }

    #[test]
    fn pending_state_tracks_new_prompt() {
        let store = store();
        store.apply(&event("Stop", "t1", 200)).unwrap();
        store.apply(&event("UserPromptSubmit", "t2", 300)).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert!(sessions[0].pending);
        assert!(sessions[0].unread);
    }

    #[test]
    fn opening_panel_marks_all_seen() {
        let store = store();
        let mut a = event("Stop", "t1", 100);
        a.session_id = "a".to_string();
        store.apply(&a).unwrap();
        let mut b = event("Stop", "t1", 200);
        b.session_id = "b".to_string();
        store.apply(&b).unwrap();
        assert_eq!(store.mark_all_seen().unwrap(), 2);
        assert_eq!(store.count_unread().unwrap(), 0);
    }

    #[test]
    fn unread_sessions_sort_first() {
        let store = store();
        let mut a = event("Stop", "t1", 100);
        a.session_id = "a".to_string();
        store.apply(&a).unwrap();
        store.clear_if_version("a", 1).unwrap();
        let mut b = event("Stop", "t1", 200);
        b.session_id = "b".to_string();
        store.apply(&b).unwrap();
        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions[0].session_id, "b");
    }
}
