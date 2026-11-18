use crate::paths;
use crate::sources::{self, ReadSignal};
use crate::store::Store;
use crate::tray;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub fn start(app: AppHandle, store: Store) {
    thread::spawn(move || {
        let mut ticks = 0u64;
        loop {
        thread::sleep(Duration::from_millis(500));
        ticks += 1;
        if ticks % 1200 == 0 {
            let _ = store.prune_seen();
            let _ = store.prune_handled();
        }

        let Ok(unread) = store.unread_sessions() else {
            continue;
        };
        let unread: Vec<String> = unread
            .into_iter()
            .filter(|(_, source)| sources::read_signal(source) == ReadSignal::Trusted)
            .map(|(session_id, _)| session_id)
            .collect();
        if unread.is_empty() {
            continue;
        }

        let Some(path) = paths::codex_state_path() else {
            continue;
        };
        let Ok(conn) = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) else {
            continue;
        };
        let _ = conn.busy_timeout(Duration::from_millis(200));

        let mut seen = Vec::new();
        for session_id in unread {
            let receipt = conn
                .query_row(
                    "SELECT first_unread_turn FROM thread_read_receipts WHERE thread_id = ?1",
                    params![session_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional();
            if matches!(receipt, Ok(Some(None))) {
                seen.push(session_id);
            }
        }

        if seen.is_empty() {
            continue;
        }

        if let Ok(updated) = store.mark_seen_sessions(&seen) {
            if updated == 0 {
                continue;
            }
            let _ = store.prune_seen();
            let _ = store.prune_handled();
            if let Ok(count) = store.count_unread() {
                tray::set_count(&app, count);
            }
            let _ = app.emit("sessions-updated", ());
        }
        }
    });
}
