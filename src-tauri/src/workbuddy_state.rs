use crate::paths;
use crate::sources;
use crate::store::{external_unread_key, SessionView, Store, StoreResult};
use crate::tray;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub fn start(app: AppHandle, store: Store) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));

        let unread = store
            .unread_sessions()
            .unwrap_or_default()
            .into_iter()
            .filter(|(source, _)| source == sources::WORKBUDDY)
            .collect::<Vec<_>>();
        let running = store
            .list_sessions()
            .unwrap_or_default()
            .into_iter()
            .filter(|session| session.source == sources::WORKBUDDY && session.pending)
            .collect::<Vec<_>>();
        if unread.is_empty() && running.is_empty() {
            continue;
        }

        let path = paths::workbuddy_home().join("workbuddy.db");
        if !path.is_file() {
            continue;
        }
        let Ok(conn) = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        let _ = conn.busy_timeout(Duration::from_millis(200));

        let mut changed = false;
        for session in running {
            let state = conn
                .query_row(
                    "SELECT status, last_activity_at FROM sessions WHERE id = ?1",
                    params![session.session_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional();
            if let Ok(Some((status, last_activity_at))) = state {
                if let Ok(Some(completed)) =
                    apply_external_lifecycle(&store, &session.session_id, &status, last_activity_at)
                {
                    changed = true;
                    let _ = app.emit("task-completed", &completed);
                }
            }
        }

        for (_, session_id) in unread {
            let state = conn
                .query_row(
                    "SELECT unread FROM sessions WHERE id = ?1",
                    params![session_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional();
            if let Ok(Some(external_unread)) = state {
                changed |=
                    apply_external_unread(&store, &session_id, external_unread).unwrap_or(false);
            }
        }

        if changed {
            let _ = store.prune_seen();
            let _ = store.prune_handled();
            if let Ok(count) = store.count_unread() {
                tray::set_count(&app, count);
            }
            let _ = app.emit("sessions-updated", ());
        }
    });
}

fn apply_external_lifecycle(
    store: &Store,
    session_id: &str,
    status: &str,
    last_activity_at: i64,
) -> StoreResult<Option<SessionView>> {
    if status == "completed" {
        return store.complete_running_session_from_external(
            sources::WORKBUDDY,
            session_id,
            last_activity_at,
        );
    }
    Ok(None)
}

fn apply_external_unread(
    store: &Store,
    session_id: &str,
    external_unread: i64,
) -> StoreResult<bool> {
    match external_unread {
        1 => {
            store.meta_set(&external_unread_key(sources::WORKBUDDY, session_id), "1")?;
            Ok(false)
        }
        0 => store.mark_observed_session(sources::WORKBUDDY, session_id),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::HookEvent;

    fn store(name: &str) -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "irisport-workbuddy-state-{name}-{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        (Store::open(path.clone()).unwrap(), path)
    }

    fn stop(session_id: &str, turn_id: &str, t: i64) -> HookEvent {
        HookEvent {
            t,
            event: "Stop".to_string(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            source: sources::WORKBUDDY.to_string(),
            prompt: None,
            session_title: Some("WorkBuddy 任务".to_string()),
            cwd: None,
            client_type: None,
        }
    }

    fn start(session_id: &str, turn_id: &str, t: i64) -> HookEvent {
        HookEvent {
            event: "UserPromptSubmit".to_string(),
            prompt: Some("WorkBuddy 任务".to_string()),
            ..stop(session_id, turn_id, t)
        }
    }

    #[test]
    fn external_completed_state_finishes_a_running_task_once() {
        let (store, path) = store("external-completed");
        store.apply(&start("s1", "t1", 100)).unwrap();
        let completed = apply_external_lifecycle(&store, "s1", "completed", 500)
            .unwrap()
            .unwrap();
        assert_eq!(completed.state, "completed");
        assert_eq!(completed.result_seq, 1);
        assert_eq!(completed.last_result_at, Some(500));
        assert!(apply_external_lifecycle(&store, "s1", "completed", 500)
            .unwrap()
            .is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn active_or_working_state_keeps_the_task_running() {
        let (store, path) = store("external-working");
        store.apply(&start("s1", "t1", 100)).unwrap();
        for status in ["active", "working"] {
            assert!(apply_external_lifecycle(&store, "s1", status, 500)
                .unwrap()
                .is_none());
        }
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn stale_external_completion_does_not_finish_a_newer_turn() {
        let (store, path) = store("external-stale");
        store.apply(&start("s1", "t1", 500)).unwrap();
        assert!(apply_external_lifecycle(&store, "s1", "completed", 100)
            .unwrap()
            .is_none());
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn default_read_state_does_not_clear_a_result() {
        let (store, path) = store("default-read");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        assert!(!apply_external_unread(&store, "s1", 0).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn observed_unread_then_read_clears_the_result() {
        let (store, path) = store("observed");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        assert!(!apply_external_unread(&store, "s1", 1).unwrap());
        assert!(apply_external_unread(&store, "s1", 0).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        assert!(store.list_sessions().unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_new_result_requires_a_new_unread_observation() {
        let (store, path) = store("new-result");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        apply_external_unread(&store, "s1", 1).unwrap();
        store.apply(&stop("s1", "t2", 200)).unwrap();
        assert!(!apply_external_unread(&store, "s1", 0).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
        let _ = std::fs::remove_file(path);
    }
}
