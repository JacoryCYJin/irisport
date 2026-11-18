use crate::hooks_config;
use crate::panel;
use crate::paths;
use crate::sources;
use crate::store::{SessionView, Store};
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
    data_path: String,
    version: String,
}

#[derive(Serialize)]
pub struct OpenResultView {
    ok: bool,
    message: Option<String>,
}

#[tauri::command]
pub fn list_sessions(store: State<Store>) -> Result<Vec<SessionView>, String> {
    store.list_sessions()
}

#[tauri::command]
pub fn open_session(store: State<Store>, session_id: String) -> Result<OpenResultView, String> {
    let Some(source) = store.session_source(&session_id)? else {
        return Ok(OpenResultView {
            ok: false,
            message: Some("会话不存在".to_string()),
        });
    };
    let Some(url) = sources::open_url(&source, &session_id) else {
        return Ok(OpenResultView {
            ok: false,
            message: Some(format!(
                "{} 暂不支持回到原会话",
                sources::label(&source)
            )),
        });
    };
    let result = Command::new("/usr/bin/open").arg(&url).output();

    match result {
        Ok(output) if output.status.success() => Ok(OpenResultView {
            ok: true,
            message: None,
        }),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Ok(OpenResultView {
                ok: false,
                message: Some(if detail.is_empty() {
                    "系统未接受打开请求".to_string()
                } else {
                    format!("系统未接受打开请求：{detail}")
                }),
            })
        }
        Err(error) => Ok(OpenResultView {
            ok: false,
            message: Some(format!("无法发起打开请求：{error}")),
        }),
    }
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
pub fn set_panel_height(app: AppHandle, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window("panel") else {
        return Ok(());
    };
    let height = height.clamp(100.0, 480.0);
    window
        .set_size(tauri::LogicalSize::new(360.0, height))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

pub fn refresh_hook_binary(app: &AppHandle) -> Result<(), String> {
    let (configured, _) = hooks_config::configured()?;
    if !configured {
        return Ok(());
    }
    let source = hook_source(app)?;
    let destination = paths::hook_binary();
    fs::create_dir_all(paths::hook_bin_dir()).map_err(|error| error.to_string())?;
    let temp = paths::hook_bin_dir().join(format!(
        ".irisport-hook-{}.tmp",
        std::process::id()
    ));
    fs::copy(&source, &temp).map_err(|error| error.to_string())?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    fs::rename(&temp, &destination).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        #[cfg(target_os = "macos")]
        let _ = app.show();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let window = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("index.html?view=settings".into()),
    )
    .title("Irisport 设置")
    .inner_size(560.0, 540.0)
    .resizable(false)
    .center()
    .build()
    .map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    let _ = app.show();
    let _ = window.set_focus();
    Ok(())
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
        rejected_count: store
            .meta_get("rejected_count")?
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0),
        hooks_path: hooks_config::hooks_path().display().to_string(),
        hook_installed: paths::hook_binary().is_file(),
        data_path: paths::support_dir().display().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
