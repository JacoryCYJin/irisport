use crate::paths;
use crate::store::{HookEvent, Store};
use crate::tray;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub fn start(app: AppHandle, store: Store) {
    thread::spawn(move || {
        let spool = paths::spool_path();
        let mut offset = store
            .meta_get("spool_offset")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);

        loop {
            match read_batch(&spool, &mut offset) {
                Ok(events) if !events.is_empty() => {
                    let mut changed = false;
                    for event in events {
                        if event.source != "desktop" {
                            continue;
                        }
                        let processed_at = now_ms();
                        match store.apply(&event) {
                            Ok(true) => {
                                changed = true;
                                append_latency(&event, processed_at);
                            }
                            Ok(false) => {}
                            Err(error) => eprintln!("irisport: 事件写入失败: {error}"),
                        }
                    }
                    let _ = store.meta_set("spool_offset", &offset.to_string());
                    if changed {
                        if let Ok(count) = store.count_unread() {
                            tray::set_count(&app, count);
                        }
                        let _ = app.emit("sessions-updated", ());
                        let _ = app.emit("hook-status-updated", ());
                        crate::panel::mark_seen_if_focused(&app);
                    }
                }
                Ok(_) => {}
                Err(error) => eprintln!("irisport: 读取事件队列失败: {error}"),
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
}

fn read_batch(path: &Path, offset: &mut u64) -> std::io::Result<Vec<HookEvent>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(Vec::new()),
    };

    if metadata.len() < *offset {
        *offset = 0;
    }
    if metadata.len() == *offset {
        return Ok(Vec::new());
    }

    let mut file = OpenOptions::new().read(true).open(path)?;
    file.seek(SeekFrom::Start(*offset))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;

    let Some(last_newline) = text.rfind('\n') else {
        return Ok(Vec::new());
    };

    let complete = &text[..=last_newline];
    *offset += complete.as_bytes().len() as u64;

    let mut events = Vec::new();
    for line in complete.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<HookEvent>(line) {
            events.push(event);
        }
    }
    Ok(events)
}

fn append_latency(event: &HookEvent, processed_at: i64) {
    let path = paths::latency_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let record = serde_json::json!({
        "t": event.t,
        "processed_at": processed_at,
        "delta_ms": processed_at - event.t,
        "event": event.event,
    });
    let _ = writeln!(file, "{record}");
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
