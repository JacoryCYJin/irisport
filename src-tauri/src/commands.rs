use crate::hooks_config;
use crate::panel;
use crate::paths;
use crate::store::{SessionView, Store};
use crate::tray;
use serde::Serialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize)]
pub struct HookStatusView {
    configured: bool,
    access_verified: bool,
    last_event_at: Option<i64>,
    rejected_count: usize,
    hooks_path: String,
    hook_installed: bool,
}

#[derive(Serialize)]
pub struct OpenResultView {
    ok: bool,
    cleared: bool,
    kept_due_to_new_result: bool,
    message: Option<String>,
}

#[tauri::command]
pub fn list_sessions(store: State<Store>) -> Result<Vec<SessionView>, String> {
    store.list_sessions()
}

#[tauri::command]
pub fn open_session(
    app: AppHandle,
    store: State<Store>,
    session_id: String,
    expected_result_seq: i64,
) -> Result<OpenResultView, String> {
    let Some((unread, current_seq)) = store.session_state(&session_id)? else {
        return Ok(OpenResultView {
            ok: false,
            cleared: false,
            kept_due_to_new_result: false,
            message: Some("会话不存在".to_string()),
        });
    };

    if !unread {
        return Ok(OpenResultView {
            ok: true,
            cleared: false,
            kept_due_to_new_result: false,
            message: None,
        });
    }

    if current_seq != expected_result_seq {
        return Ok(OpenResultView {
            ok: true,
            cleared: false,
            kept_due_to_new_result: true,
            message: Some("已有新结果，本次提醒保留".to_string()),
        });
    }

    let url = format!("codex://threads/{}", encode_component(&session_id));
    let result = Command::new("/usr/bin/open").arg(&url).output();
    let accepted = match &result {
        Ok(output) => output.status.success(),
        Err(_) => false,
    };

    if !accepted {
        let detail = match result {
            Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_string(),
            Err(error) => error.to_string(),
        };
        return Ok(OpenResultView {
            ok: false,
            cleared: false,
            kept_due_to_new_result: false,
            message: Some(if detail.is_empty() {
                "系统未接受打开请求，提醒已保留".to_string()
            } else {
                format!("系统未接受打开请求：{detail}")
            }),
        });
    }

    let cleared = store.clear_if_version(&session_id, expected_result_seq)?;
    if cleared {
        if let Ok(count) = store.count_unread() {
            tray::set_count(&app, count);
        }
        let _ = app.emit("sessions-updated", ());
    }

    Ok(OpenResultView {
        ok: true,
        cleared,
        kept_due_to_new_result: false,
        message: None,
    })
}

#[tauri::command]
pub fn install_codex_hooks(app: AppHandle, store: State<Store>) -> Result<HookStatusView, String> {
    install_inner(&app, &store)?;
    hook_status(app, store)
}

#[tauri::command]
pub fn uninstall_codex_hooks(
    app: AppHandle,
    store: State<Store>,
) -> Result<HookStatusView, String> {
    hooks_config::uninstall()?;
    let _ = app.emit("hook-status-updated", ());
    hook_status(app, store)
}

#[tauri::command]
pub fn hook_status(app: AppHandle, store: State<Store>) -> Result<HookStatusView, String> {
    build_status(&app, &store)
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    panel::hide(&app);
}

#[tauri::command]
pub fn open_codex_settings() -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg("codex://settings")
        .status()
        .map(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("系统未接受打开设置请求".to_string())
            }
        })
        .map_err(|error| error.to_string())?
}

pub fn install_from_tray(app: &AppHandle) -> Result<(), String> {
    let store = app.state::<Store>();
    install_inner(app, &store)?;
    let _ = app.emit("hook-status-updated", ());
    Ok(())
}

pub fn uninstall_from_tray(app: &AppHandle) -> Result<(), String> {
    hooks_config::uninstall()?;
    let _ = app.emit("hook-status-updated", ());
    Ok(())
}

fn install_inner(app: &AppHandle, store: &Store) -> Result<(), String> {
    let source = hook_source(app)?;
    fs::create_dir_all(paths::hook_bin_dir()).map_err(|error| error.to_string())?;

    let binary = paths::hook_binary();
    fs::copy(&source, &binary).map_err(|error| format!("安装 hook 二进制失败: {error}"))?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;

    let wrapper = paths::hook_wrapper();
    let script = format!(
        "#!/bin/sh\nBIN=\"{}\"\n[ -x \"$BIN\" ] || exit 0\n\"$BIN\" \"$@\" 2>/dev/null || exit 0\nexit 0\n",
        binary.display()
    );
    fs::write(&wrapper, script).map_err(|error| format!("写入 hook wrapper 失败: {error}"))?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;

    let command = format!("/bin/sh \"{}\"", wrapper.display());
    hooks_config::install(&command)?;
    store.meta_set("hooks_installed_at", &now_ms().to_string())?;
    Ok(())
}

fn build_status(_app: &AppHandle, store: &Store) -> Result<HookStatusView, String> {
    let (configured, _entries) = hooks_config::configured()?;
    let access_verified = store.meta_get("access_verified_at")?.is_some();
    let last_event_at = store
        .meta_get("last_event_at")?
        .and_then(|value| value.parse::<i64>().ok());

    Ok(HookStatusView {
        configured,
        access_verified,
        last_event_at,
        rejected_count: count_lines(&paths::rejected_path()),
        hooks_path: hooks_config::hooks_path().display().to_string(),
        hook_installed: paths::hook_binary().is_file(),
    })
}

fn hook_source(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("bin/irisport-hook");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let candidate = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin/irisport-hook");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err("找不到 irisport-hook 二进制，请先运行 scripts/prepare-hook.sh".to_string())
}

fn count_lines(path: &std::path::Path) -> usize {
    fs::read_to_string(path)
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0)
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let allowed = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if allowed {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
