use crate::paths;
use crate::sources;
use crate::store::{external_unread_key, Store, StoreResult};
use crate::tray;
use chromium_storage_localstorage_core::{read_dir, LocalStorageRecord};
use std::collections::HashMap;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const UNREAD_KEY: &str = "kimi-web.unread";

pub fn start(app: AppHandle, store: Store) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));

        let Ok(unread) = store.unread_sessions() else {
            continue;
        };
        let unread: Vec<(String, String)> = unread
            .into_iter()
            .filter(|(source, _)| source == sources::KIMI_CODE)
            .collect();
        if unread.is_empty() {
            continue;
        }

        let path = paths::kimi_code_app_home().join("Local Storage/leveldb");
        let Ok(Some(external_unread)) = read_unread_map(&path) else {
            continue;
        };
        let mut changed = false;
        for (_, session_id) in unread {
            let is_unread = external_unread.get(&session_id).copied();
            changed |= apply_external_unread(&store, &session_id, is_unread).unwrap_or(false);
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

fn read_unread_map(path: &Path) -> Result<Option<HashMap<String, bool>>, String> {
    if !path.is_dir() {
        return Ok(None);
    }
    let records = read_dir(path).map_err(|error| error.to_string())?;
    latest_unread_map(records)
}

fn latest_unread_map(
    records: Vec<LocalStorageRecord>,
) -> Result<Option<HashMap<String, bool>>, String> {
    let latest = records
        .into_iter()
        .filter_map(|record| match record {
            LocalStorageRecord::Data {
                script_key,
                value,
                seq,
                deleted,
                ..
            } if script_key.text == UNREAD_KEY && !script_key.lossy => Some((seq, value, deleted)),
            _ => None,
        })
        .max_by_key(|(seq, _, _)| *seq);

    let Some((_, value, deleted)) = latest else {
        return Ok(None);
    };
    if deleted {
        return Ok(Some(HashMap::new()));
    }
    if value.lossy {
        return Err("Kimi Code 未读状态无法完整解码".to_string());
    }
    serde_json::from_str(&value.text)
        .map(Some)
        .map_err(|error| format!("Kimi Code 未读状态不是合法 JSON: {error}"))
}

fn apply_external_unread(
    store: &Store,
    session_id: &str,
    external_unread: Option<bool>,
) -> StoreResult<bool> {
    match external_unread {
        Some(true) => {
            store.meta_set(&external_unread_key(sources::KIMI_CODE, session_id), "1")?;
            Ok(false)
        }
        Some(false) | None => store.mark_observed_session(sources::KIMI_CODE, session_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::HookEvent;
    use chromium_storage_localstorage_core::{Encoding, StorageValue};

    fn storage_value(text: &str) -> StorageValue {
        StorageValue {
            text: text.to_string(),
            raw: text.as_bytes().to_vec(),
            encoding: Encoding::Latin1,
            lossy: false,
        }
    }

    fn record(seq: u64, value: &str, deleted: bool) -> LocalStorageRecord {
        LocalStorageRecord::Data {
            origin: "app://renderer".to_string(),
            script_key: storage_value(UNREAD_KEY),
            value: storage_value(value),
            seq,
            deleted,
        }
    }

    fn store(name: &str) -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "irisport-kimi-state-{name}-{}.sqlite3",
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
            source: sources::KIMI_CODE.to_string(),
            prompt: None,
            session_title: Some("Kimi 任务".to_string()),
            cwd: None,
            client_type: None,
        }
    }

    #[test]
    fn latest_record_wins_and_tombstone_means_no_unread() {
        let records = vec![
            record(8, r#"{"s1":true}"#, false),
            record(10, r#"{"s1":true}"#, false),
            record(9, "{}", false),
        ];
        assert_eq!(
            latest_unread_map(records).unwrap().unwrap().get("s1"),
            Some(&true)
        );
        assert!(latest_unread_map(vec![record(11, "", true)])
            .unwrap()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn absent_or_false_state_does_not_clear_without_prior_unread() {
        let (store, path) = store("not-observed");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        assert!(!apply_external_unread(&store, "s1", None).unwrap());
        assert!(!apply_external_unread(&store, "s1", Some(false)).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn true_then_absent_clears_after_viewing() {
        let (store, path) = store("removed");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        assert!(!apply_external_unread(&store, "s1", Some(true)).unwrap());
        assert!(apply_external_unread(&store, "s1", None).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn true_then_false_clears_only_the_matching_session() {
        let (store, path) = store("transition");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        store.apply(&stop("s2", "t2", 200)).unwrap();
        assert!(!apply_external_unread(&store, "s1", Some(true)).unwrap());
        assert!(apply_external_unread(&store, "s1", Some(false)).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
        let remaining = store.list_sessions().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].session_id, "s2");
        let _ = std::fs::remove_file(path);
    }
}
