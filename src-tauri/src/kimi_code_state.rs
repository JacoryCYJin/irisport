use crate::paths;
use crate::sources;
use crate::store::{Store, StoreResult};
use crate::tray;
use chromium_storage_localstorage_core::{read_dir, LocalStorageRecord};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const UNREAD_KEY: &str = "kimi-web.unread";

#[derive(Deserialize)]
struct SessionIndexEntry {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "sessionDir")]
    session_dir: Option<PathBuf>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Deserialize)]
struct SessionState {
    id: String,
    #[serde(rename = "lastTurnReason")]
    last_turn_reason: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: i64,
}

struct UnreadSnapshot {
    current: HashMap<String, bool>,
    latest_seq: u64,
    unread_sequences: HashMap<String, u64>,
}

#[derive(Serialize, Deserialize)]
struct UnreadBaseline {
    started_at: i64,
    sequence: u64,
}

pub fn start(app: AppHandle, store: Store) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));

        let mut changed = reconcile_cancelled_sessions(&store).unwrap_or(0) > 0;

        let sessions = store.kimi_sync_sessions().unwrap_or_default();
        if !sessions.is_empty() {
            let path = paths::kimi_code_app_home().join("Local Storage/leveldb");
            if let Ok(Some(snapshot)) = read_unread_snapshot(&path) {
                let _ = sync_unread_baselines(&store, &snapshot);
                for session_id in sessions {
                    changed |=
                        apply_unread_snapshot(&store, &session_id, &snapshot).unwrap_or(false);
                }
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

fn reconcile_cancelled_sessions(store: &Store) -> StoreResult<usize> {
    reconcile_cancelled_sessions_at(store, &paths::kimi_code_home().join("session_index.jsonl"))
}

fn reconcile_cancelled_sessions_at(store: &Store, index_path: &Path) -> StoreResult<usize> {
    let running = store
        .list_sessions()?
        .into_iter()
        .filter(|session| session.source == sources::KIMI_CODE && session.pending)
        .collect::<Vec<_>>();
    if running.is_empty() || !index_path.is_file() {
        return Ok(0);
    }

    let index = fs::read_to_string(index_path).map_err(|error| error.to_string())?;
    let mut session_dirs = HashMap::new();
    for line in index.lines() {
        let Ok(entry) = serde_json::from_str::<SessionIndexEntry>(line) else {
            continue;
        };
        if entry.deleted {
            session_dirs.remove(&entry.session_id);
        } else if let Some(path) = entry.session_dir {
            session_dirs.insert(entry.session_id, path);
        }
    }

    let mut changed = 0;
    for session in running {
        let Some(state_path) = session_dirs
            .get(&session.session_id)
            .map(|directory| directory.join("state.json"))
        else {
            continue;
        };
        let Ok(raw) = fs::read_to_string(state_path) else {
            continue;
        };
        let Ok(state) = serde_json::from_str::<SessionState>(&raw) else {
            continue;
        };
        let started_at = session.last_prompt_at.unwrap_or(session.updated_at);
        if state.id == session.session_id
            && state.last_turn_reason.as_deref() == Some("cancelled")
            && state.updated_at >= started_at
            && store.mark_interrupted_session(
                sources::KIMI_CODE,
                &session.session_id,
                state.updated_at,
            )?
        {
            changed += 1;
        }
    }
    Ok(changed)
}

fn read_unread_snapshot(path: &Path) -> Result<Option<UnreadSnapshot>, String> {
    if !path.is_dir() {
        return Ok(None);
    }
    let records = read_dir(path).map_err(|error| error.to_string())?;
    unread_snapshot(records)
}

fn unread_snapshot(records: Vec<LocalStorageRecord>) -> Result<Option<UnreadSnapshot>, String> {
    let mut latest: Option<(u64, Result<HashMap<String, bool>, String>)> = None;
    let mut unread_sequences: HashMap<String, u64> = HashMap::new();

    for record in records {
        let LocalStorageRecord::Data {
            script_key,
            value,
            seq,
            deleted,
            ..
        } = record
        else {
            continue;
        };
        if script_key.text != UNREAD_KEY || script_key.lossy {
            continue;
        }

        let parsed: Result<HashMap<String, bool>, String> = if deleted {
            Ok(HashMap::new())
        } else if value.lossy {
            Err("Kimi Code 未读状态无法完整解码".to_string())
        } else {
            serde_json::from_str(&value.text)
                .map_err(|error| format!("Kimi Code 未读状态不是合法 JSON: {error}"))
        };
        if let Ok(map) = &parsed {
            for (session_id, unread) in map {
                if *unread {
                    unread_sequences
                        .entry(session_id.clone())
                        .and_modify(|current| *current = (*current).max(seq))
                        .or_insert(seq);
                }
            }
        }
        if latest
            .as_ref()
            .map(|(current, _)| seq > *current)
            .unwrap_or(true)
        {
            latest = Some((seq, parsed));
        }
    }

    let Some((latest_seq, current)) = latest else {
        return Ok(None);
    };
    Ok(Some(UnreadSnapshot {
        current: current?,
        latest_seq,
        unread_sequences,
    }))
}

fn unread_baseline_key(session_id: &str) -> String {
    format!("kimi_unread_baseline:{session_id}")
}

fn sync_unread_baselines(store: &Store, snapshot: &UnreadSnapshot) -> StoreResult<()> {
    let running = store
        .list_sessions()?
        .into_iter()
        .filter(|session| session.source == sources::KIMI_CODE && session.pending);
    for session in running {
        let started_at = session.last_prompt_at.unwrap_or(session.updated_at);
        let key = unread_baseline_key(&session.session_id);
        let existing = store
            .meta_get(&key)?
            .and_then(|value| serde_json::from_str::<UnreadBaseline>(&value).ok());
        if existing
            .as_ref()
            .map(|baseline| baseline.started_at == started_at)
            .unwrap_or(false)
        {
            continue;
        }
        let baseline = UnreadBaseline {
            started_at,
            sequence: snapshot.latest_seq,
        };
        store.meta_set(
            &key,
            &serde_json::to_string(&baseline).map_err(|error| error.to_string())?,
        )?;
    }
    Ok(())
}

fn apply_unread_snapshot(
    store: &Store,
    session_id: &str,
    snapshot: &UnreadSnapshot,
) -> StoreResult<bool> {
    let current = snapshot.current.get(session_id).copied();
    let baseline = store
        .meta_get(&unread_baseline_key(session_id))?
        .and_then(|value| serde_json::from_str::<UnreadBaseline>(&value).ok());
    if let Some(baseline) = baseline {
        let historical_unread = snapshot
            .unread_sequences
            .get(session_id)
            .map(|sequence| *sequence > baseline.sequence)
            .unwrap_or(false);
        if historical_unread && current != Some(true) {
            store.mark_external_unread(sources::KIMI_CODE, session_id)?;
            return store.mark_observed_session_turn(
                sources::KIMI_CODE,
                session_id,
                baseline.started_at,
            );
        }
    }
    apply_external_unread(store, session_id, current)
}

fn apply_external_unread(
    store: &Store,
    session_id: &str,
    external_unread: Option<bool>,
) -> StoreResult<bool> {
    match external_unread {
        Some(true) => store.mark_external_unread(sources::KIMI_CODE, session_id),
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

    fn start(session_id: &str, turn_id: &str, t: i64) -> HookEvent {
        HookEvent {
            event: "TurnStarted".to_string(),
            prompt: Some("测试任务".to_string()),
            ..stop(session_id, turn_id, t)
        }
    }

    fn write_session_state(
        index_path: &Path,
        session_id: &str,
        last_turn_reason: Option<&str>,
        updated_at: i64,
    ) {
        let root = index_path.parent().unwrap();
        let session_dir = root.join(session_id);
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("state.json"),
            serde_json::json!({
                "id": session_id,
                "lastTurnReason": last_turn_reason,
                "updatedAt": updated_at
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            index_path,
            serde_json::json!({
                "sessionId": session_id,
                "sessionDir": session_dir
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn latest_record_wins_and_tombstone_means_no_unread() {
        let records = vec![
            record(8, r#"{"s1":true}"#, false),
            record(10, r#"{"s1":true}"#, false),
            record(9, "{}", false),
        ];
        let snapshot = unread_snapshot(records).unwrap().unwrap();
        assert_eq!(snapshot.current.get("s1"), Some(&true));
        assert_eq!(snapshot.latest_seq, 10);
        assert_eq!(snapshot.unread_sequences.get("s1"), Some(&10));
        assert!(unread_snapshot(vec![record(11, "", true)])
            .unwrap()
            .unwrap()
            .current
            .is_empty());
    }

    #[test]
    fn absent_or_false_state_does_not_clear_without_prior_unread() {
        let (store, path) = store("not-observed");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        assert!(!apply_external_unread(&store, "s1", None).unwrap());
        assert!(!apply_external_unread(&store, "s1", Some(false)).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        assert!(store.list_sessions().unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn true_then_absent_clears_after_viewing() {
        let (store, path) = store("removed");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        assert!(apply_external_unread(&store, "s1", Some(true)).unwrap());
        assert!(apply_external_unread(&store, "s1", None).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn true_then_false_clears_only_the_matching_session() {
        let (store, path) = store("transition");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        store.apply(&stop("s2", "t2", 200)).unwrap();
        assert!(apply_external_unread(&store, "s1", Some(true)).unwrap());
        assert!(apply_external_unread(&store, "s2", Some(true)).unwrap());
        assert!(apply_external_unread(&store, "s1", Some(false)).unwrap());
        assert_eq!(store.count_unread().unwrap(), 1);
        let remaining = store.list_sessions().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].session_id, "s2");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn transient_unread_between_polls_still_proves_the_result_was_read() {
        let (store, path) = store("transient");
        store.apply(&start("s1", "t1", 100)).unwrap();
        let baseline = unread_snapshot(vec![record(9, "{}", false)])
            .unwrap()
            .unwrap();
        sync_unread_baselines(&store, &baseline).unwrap();
        store.apply(&stop("s1", "t1", 200)).unwrap();
        let snapshot = unread_snapshot(vec![
            record(9, "{}", false),
            record(10, r#"{"s1":true}"#, false),
            record(11, "{}", false),
        ])
        .unwrap()
        .unwrap();
        assert!(apply_unread_snapshot(&store, "s1", &snapshot).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn previous_turn_unread_history_does_not_clear_a_new_result() {
        let (store, path) = store("previous-history");
        store.apply(&start("s1", "t1", 100)).unwrap();
        let snapshot = unread_snapshot(vec![
            record(9, r#"{"s1":true}"#, false),
            record(10, "{}", false),
        ])
        .unwrap()
        .unwrap();
        sync_unread_baselines(&store, &snapshot).unwrap();
        store.apply(&stop("s1", "t1", 200)).unwrap();
        assert!(!apply_unread_snapshot(&store, "s1", &snapshot).unwrap());
        assert_eq!(store.count_unread().unwrap(), 0);
        assert!(store.list_sessions().unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_unobserved_reminder_is_hidden_without_becoming_seen() {
        let (store, path) = store("legacy-repair");
        store.apply(&stop("s1", "t1", 100)).unwrap();
        store
            .mark_external_unread(sources::KIMI_CODE, "s1")
            .unwrap();
        store
            .meta_set("external_unread_observed:kimi-code:s1", "0")
            .unwrap();
        assert_eq!(store.repair_unobserved_external_unread().unwrap(), 1);
        assert_eq!(store.count_unread().unwrap(), 0);
        assert!(store.list_sessions().unwrap().is_empty());
        assert_eq!(store.kimi_sync_sessions().unwrap().len(), 1);
        assert_eq!(store.prune_seen().unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cancelled_state_interrupts_the_running_turn() {
        let (store, path) = store("cancelled");
        store.apply(&start("s1", "t1", 100)).unwrap();
        let root =
            std::env::temp_dir().join(format!("irisport-kimi-cancelled-{}", std::process::id()));
        let index = root.join("session_index.jsonl");
        write_session_state(&index, "s1", Some("cancelled"), 500);
        assert_eq!(reconcile_cancelled_sessions_at(&store, &index).unwrap(), 1);
        assert!(store.list_sessions().unwrap().is_empty());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn active_state_without_a_cancelled_reason_keeps_running() {
        let (store, path) = store("active");
        store.apply(&start("s1", "t1", 100)).unwrap();
        let root =
            std::env::temp_dir().join(format!("irisport-kimi-active-{}", std::process::id()));
        let index = root.join("session_index.jsonl");
        write_session_state(&index, "s1", None, 500);
        assert_eq!(reconcile_cancelled_sessions_at(&store, &index).unwrap(), 0);
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn previous_cancellation_does_not_interrupt_a_new_turn() {
        let (store, path) = store("previous-cancelled");
        store.apply(&start("s1", "t1", 200)).unwrap();
        let root = std::env::temp_dir().join(format!(
            "irisport-kimi-previous-cancelled-{}",
            std::process::id()
        ));
        let index = root.join("session_index.jsonl");
        write_session_state(&index, "s1", Some("cancelled"), 100);
        assert_eq!(reconcile_cancelled_sessions_at(&store, &index).unwrap(), 0);
        assert_eq!(store.list_sessions().unwrap()[0].state, "running");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(root);
    }
}
