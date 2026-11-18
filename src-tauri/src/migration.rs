use crate::codex;
use crate::hooks_config;
use crate::paths;
use crate::store::Store;
use std::fs;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUIET_WINDOW: Duration = Duration::from_secs(2);
const MAX_WAIT: Duration = Duration::from_secs(15);

pub fn run(store: &Store) -> Result<(), String> {
    let version = store.storage_version()?;

    if version >= 2 {
        if paths::legacy_queue_path().exists() {
            let mut offset = store
                .meta_get("spool_offset")?
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            let _ = codex::drain_legacy_once(store, &mut offset);
            let _ = fs::remove_file(paths::legacy_queue_path());
            let _ = store.meta_remove("spool_offset");
        }
        let _ = fs::remove_file(paths::legacy_rejected_path());
        let _ = store.prune_seen();
        let _ = store.prune_handled();
        codex::compact_latency();
        codex::trim_latency_for_size();
        return Ok(());
    }

    store.meta_set("migration_state", "draining")?;

    let hooks_configured = hooks_config::configured()?.0;
    let had_legacy = paths::legacy_queue_path().exists()
        || store.meta_get("spool_offset")?.is_some();

    let mut offset = store
        .meta_get("spool_offset")?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    if hooks_configured || had_legacy {
        let deadline = Instant::now() + MAX_WAIT;
        let mut last_signature = queue_signature();
        let mut quiet_since = Instant::now();

        loop {
            let _ = codex::drain_legacy_once(store, &mut offset);
            let _ = codex::process_inbox_once(store);
            let _ = store.meta_set("spool_offset", &offset.to_string());

            let signature = queue_signature();
            let length = queue_length();
            if signature != last_signature || offset < length {
                last_signature = signature;
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() >= QUIET_WINDOW {
                break;
            }

            if Instant::now() >= deadline {
                store.meta_set("migration_state", "legacy_busy")?;
                return Ok(());
            }

            thread::sleep(Duration::from_millis(150));
        }
    }

    store.meta_set("legacy_quiet_at", &now_ms().to_string())?;
    store.migrate_seen_events()?;
    store.prune_seen()?;
    store.prune_handled()?;

    let _ = fs::remove_file(paths::legacy_queue_path());
    let _ = fs::remove_file(paths::legacy_rejected_path());
    let _ = store.meta_remove("spool_offset");

    if paths::probe_dir().exists() {
        let _ = fs::remove_dir_all(paths::probe_dir());
    }

    codex::compact_latency();
    codex::trim_latency_for_size();

    store.set_storage_version(2)?;
    store.meta_set("migration_state", "done")?;
    let _ = store.compact_database();

    Ok(())
}

fn queue_length() -> u64 {
    fs::metadata(paths::legacy_queue_path())
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn queue_signature() -> (u64, Option<SystemTime>) {
    match fs::metadata(paths::legacy_queue_path()) {
        Ok(metadata) => (metadata.len(), metadata.modified().ok()),
        Err(_) => (0, None),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
