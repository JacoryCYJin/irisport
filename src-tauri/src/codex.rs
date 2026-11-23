use crate::paths;
use crate::sources;
use crate::store::{HookEvent, Store};
use crate::tray;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

const LATENCY_LIMIT: usize = 200;
const LATENCY_FILE_LIMIT: u64 = 64 * 1024;

pub fn start(app: AppHandle, store: Store) {
    thread::spawn(move || {
        let mut migration_pending = store.storage_version().unwrap_or(1) < 2;
        let mut legacy_offset = store
            .meta_get("spool_offset")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let mut ticks = 0u64;
        let mut known_results = store
            .list_sessions()
            .unwrap_or_default()
            .into_iter()
            .map(|session| ((session.source, session.session_id), session.result_seq))
            .collect::<HashMap<_, _>>();

        loop {
            let mut changed = false;
            let mut touched = false;

            if migration_pending {
                match drain_legacy_once(&store, &mut legacy_offset) {
                    Ok((applied, seen)) => {
                        changed |= applied;
                        touched |= seen;
                        let _ = store.meta_set("spool_offset", &legacy_offset.to_string());
                    }
                    Err(error) => eprintln!("irisport: 读取旧事件队列失败: {error}"),
                }
            }

            match process_inbox_once(&store) {
                Ok((applied, seen)) => {
                    changed |= applied;
                    touched |= seen;
                }
                Err(error) => eprintln!("irisport: 读取事件收件箱失败: {error}"),
            }

            match store.finalize_model_failures(now_ms()) {
                Ok(count) => changed |= count > 0,
                Err(error) => eprintln!("irisport: 结束失败任务时写入失败: {error}"),
            }

            if changed {
                if let Ok(count) = store.count_unread() {
                    tray::set_count(&app, count);
                }
                let sessions = store.list_sessions().unwrap_or_default();
                let current_results = sessions
                    .iter()
                    .map(|session| {
                        (
                            (session.source.clone(), session.session_id.clone()),
                            session.result_seq,
                        )
                    })
                    .collect::<HashMap<_, _>>();
                for session in sessions {
                    let key = (session.source.clone(), session.session_id.clone());
                    if session.result_seq > known_results.get(&key).copied().unwrap_or(0) {
                        let _ = app.emit("task-completed", &session);
                    }
                }
                known_results = current_results;
                let _ = app.emit("sessions-updated", ());
            }
            if touched {
                let _ = app.emit("source-status-updated", ());
            }

            ticks += 1;
            if ticks % 1200 == 0 {
                if let Ok(version) = store.storage_version() {
                    migration_pending = version < 2;
                }
            }

            thread::sleep(Duration::from_millis(50));
        }
    });
}

pub fn process_inbox_once(store: &Store) -> Result<(bool, bool), String> {
    let dir = paths::inbox_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok((false, false)),
    };

    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    files.sort();

    let mut changed = false;
    let mut touched = false;

    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            let _ = fs::remove_file(&path);
            continue;
        };
        let event: HookEvent = match serde_json::from_str(&text) {
            Ok(event) => event,
            Err(_) => {
                let _ = fs::remove_file(&path);
                continue;
            }
        };
        touched = true;
        changed |= handle_event(store, &event);
        let _ = fs::remove_file(&path);
    }

    Ok((changed, touched))
}

pub fn drain_legacy_once(store: &Store, offset: &mut u64) -> Result<(bool, bool), String> {
    let path = paths::legacy_queue_path();
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok((false, false)),
    };

    if metadata.len() < *offset {
        *offset = 0;
    }
    if metadata.len() == *offset {
        return Ok((false, false));
    }

    let mut file = OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(*offset))
        .map_err(|error| error.to_string())?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| error.to_string())?;

    let Some(last_newline) = text.rfind('\n') else {
        return Ok((false, false));
    };
    let complete = &text[..=last_newline];

    let mut changed = false;
    let mut touched = false;
    for line in complete.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<HookEvent>(line) {
            touched = true;
            changed |= handle_event(store, &event);
        }
    }

    // Advance only after the complete lines have been handled. If the app is
    // interrupted mid-batch, restarting re-reads the batch; the events table and
    // handled_turns make the replay idempotent.
    *offset += complete.as_bytes().len() as u64;

    Ok((changed, touched))
}

fn handle_event(store: &Store, event: &HookEvent) -> bool {
    if !sources::accepts_events(&event.source) {
        let _ = store.increment_rejected(&event.source);
        return false;
    }
    let processed_at = now_ms();
    match store.apply(event) {
        Ok(true) => {
            append_latency(event, processed_at);
            true
        }
        Ok(false) => false,
        Err(error) => {
            eprintln!("irisport: 事件写入失败: {error}");
            false
        }
    }
}

fn append_latency(event: &HookEvent, processed_at: i64) {
    let path = paths::latency_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = existing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() >= LATENCY_LIMIT {
        lines = lines.split_off(lines.len() - (LATENCY_LIMIT - 1));
    }

    let record = serde_json::json!({
        "t": event.t,
        "processed_at": processed_at,
        "delta_ms": processed_at - event.t,
        "event": event.event,
    });
    let serialized = record.to_string();
    lines.push(&serialized);

    let _ = write_atomic(&path, &format!("{}\n", lines.join("\n")));
}

pub fn compact_latency() {
    let path = paths::latency_path();
    let Ok(existing) = fs::read_to_string(&path) else {
        return;
    };
    let lines: Vec<&str> = existing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() <= LATENCY_LIMIT {
        return;
    }
    let keep = &lines[lines.len() - LATENCY_LIMIT..];
    let _ = write_atomic(&path, &format!("{}\n", keep.join("\n")));
}

pub fn trim_latency_for_size() {
    if let Ok(metadata) = fs::metadata(paths::latency_path()) {
        if metadata.len() > LATENCY_FILE_LIMIT {
            compact_latency();
        }
    }
}

fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let temp = path.with_file_name(format!(
        ".{}-{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("latency"),
        std::process::id()
    ));
    fs::write(&temp, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(temp, path)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
