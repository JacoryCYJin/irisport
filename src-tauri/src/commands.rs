use crate::hooks_config;
use crate::kimi_code;
use crate::panel;
use crate::paths;
use crate::sources::{self, ReadSignal, SourceKind};
use crate::store::{SessionView, Store};
use crate::tray;
use crate::workbuddy;
use serde::Serialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize)]
pub struct SourceStatusView {
    source: String,
    label: String,
    description: String,
    installed: bool,
    configured: bool,
    update_available: bool,
    access_verified: bool,
    verified_current: bool,
    runtime_ready: bool,
    issue: Option<String>,
    checked_at: i64,
    supports_completion: bool,
    supports_open: bool,
    supports_auto_read: bool,
    last_event_at: Option<i64>,
    rejected_count: usize,
    config_path: String,
}

#[derive(Serialize)]
pub struct AppStatusView {
    sources: Vec<SourceStatusView>,
    data_path: String,
    version: String,
}

#[derive(Serialize)]
pub struct PreferencesView {
    show_unread_count: bool,
    notifications_enabled: bool,
    notification_sound: bool,
    show_excerpt_in_notification: bool,
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
pub fn open_session(
    store: State<Store>,
    source: String,
    session_id: String,
) -> Result<OpenResultView, String> {
    let Some(identity) = store.session_identity(&source, &session_id)? else {
        return Ok(OpenResultView {
            ok: false,
            message: Some("会话不存在".to_string()),
        });
    };
    if !sources::supports_open(&identity.source) {
        return Ok(OpenResultView {
            ok: false,
            message: Some(format!(
                "{} 暂不支持回到原会话",
                sources::label(&identity.source)
            )),
        });
    }

    let result = match sources::kind(&identity.source) {
        SourceKind::Codex => {
            let url = sources::open_url(&identity.source, &identity.session_id)
                .ok_or_else(|| "缺少 Codex 会话地址".to_string())?;
            run_open(Command::new("/usr/bin/open").arg(&url))
        }
        _ => OpenResultView {
            ok: false,
            message: Some("该来源暂不支持打开结果".to_string()),
        },
    };

    Ok(result)
}

#[tauri::command]
pub fn install_source(
    app: AppHandle,
    store: State<Store>,
    source: String,
) -> Result<AppStatusView, String> {
    let source = sources::canonical(&source);
    ensure_hook_binary(&app)?;
    let command = format!(
        "/bin/sh \"{}\" --source {source}",
        paths::hook_wrapper().display()
    );
    match sources::kind(source) {
        SourceKind::Codex => hooks_config::install(&command)?,
        SourceKind::KimiCode => kimi_code::install(&command)?,
        SourceKind::WorkBuddy => workbuddy::install(&command)?,
        _ => return Err("该来源不支持自动连接".to_string()),
    }
    store.meta_set(
        &format!("hooks_installed_at:{source}"),
        &now_ms().to_string(),
    )?;
    let _ = app.emit("source-status-updated", ());
    app_status(store)
}

#[tauri::command]
pub fn uninstall_source(
    app: AppHandle,
    store: State<Store>,
    source: String,
) -> Result<AppStatusView, String> {
    match sources::kind(sources::canonical(&source)) {
        SourceKind::Codex => {
            hooks_config::uninstall()?;
        }
        SourceKind::KimiCode => {
            kimi_code::uninstall()?;
        }
        SourceKind::WorkBuddy => {
            workbuddy::uninstall()?;
        }
        _ => return Err("该来源不支持断开连接".to_string()),
    }
    let _ = app.emit("source-status-updated", ());
    app_status(store)
}

#[tauri::command]
pub fn app_status(store: State<Store>) -> Result<AppStatusView, String> {
    build_status(&store)
}

#[tauri::command]
pub fn preferences(store: State<Store>) -> PreferencesView {
    PreferencesView {
        show_unread_count: store.preference_bool("show_unread_count", false),
        notifications_enabled: store.preference_bool("notifications_enabled", false),
        notification_sound: store.preference_bool("notification_sound", false),
        show_excerpt_in_notification: store.preference_bool("show_excerpt_in_notification", true),
    }
}

#[tauri::command]
pub fn set_preference(
    app: AppHandle,
    store: State<Store>,
    key: String,
    value: bool,
) -> Result<(), String> {
    const KEYS: [&str; 4] = [
        "show_unread_count",
        "notifications_enabled",
        "notification_sound",
        "show_excerpt_in_notification",
    ];
    if !KEYS.contains(&key.as_str()) {
        return Err("未知偏好设置".to_string());
    }
    store.meta_set(&key, if value { "true" } else { "false" })?;
    if key == "show_unread_count" {
        tray::refresh(&app);
    }
    let _ = app.emit("preferences-updated", ());
    Ok(())
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    panel::hide(&app);
}

#[tauri::command]
pub fn show_panel(app: AppHandle) {
    panel::show(&app);
}

#[tauri::command]
pub fn set_panel_height(app: AppHandle, height: f64) -> Result<(), String> {
    let Some(window) = app.get_webview_window("panel") else {
        return Ok(());
    };
    let height = height.clamp(120.0, 520.0);
    window
        .set_size(tauri::LogicalSize::new(380.0, height))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_settings(app: AppHandle, section: Option<String>) -> Result<(), String> {
    show_settings(&app, section)
}

#[tauri::command]
pub fn open_source(source: String) -> Result<(), String> {
    let result = match sources::kind(sources::canonical(&source)) {
        SourceKind::Codex => Command::new("/usr/bin/open")
            .arg("codex://settings")
            .status(),
        SourceKind::KimiCode => Command::new("/usr/bin/open")
            .args(["-a", "Kimi Code"])
            .status(),
        SourceKind::WorkBuddy => Command::new("/usr/bin/open")
            .args(["-a", "WorkBuddy"])
            .status(),
        _ => return Err("该来源没有可打开的应用".to_string()),
    };
    result
        .map(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("系统未接受打开请求".to_string())
            }
        })
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn reveal_path(target: String) -> Result<(), String> {
    let path = match target.as_str() {
        "app-data" => paths::support_dir(),
        "codex-hooks" => hooks_config::hooks_path(),
        "kimi-config" => kimi_code::config_path(),
        _ => return Err("未知路径".to_string()),
    };
    let mut command = Command::new("/usr/bin/open");
    if path.is_dir() {
        command.arg(&path);
    } else {
        command.arg("-R").arg(&path);
    }
    command
        .status()
        .map_err(|error| error.to_string())?
        .success()
        .then_some(())
        .ok_or_else(|| "系统未接受打开请求".to_string())
}

pub fn refresh_hook_binary(app: &AppHandle) -> Result<(), String> {
    let codex_entries = hooks_config::configured()?.1;
    let kimi_entries = kimi_code::configured()?.1;
    let workbuddy_entries = workbuddy::configured()?.1;
    if codex_entries == 0 && kimi_entries == 0 && workbuddy_entries == 0 {
        return Ok(());
    }
    copy_hook_binary(app)
}

pub fn show_settings(app: &AppHandle, section: Option<String>) -> Result<(), String> {
    let section = match section.as_deref() {
        Some("general" | "notifications" | "sources" | "data") => section,
        _ => None,
    };
    if let Some(window) = app.get_webview_window("settings") {
        if let Some(section) = section {
            let _ = app.emit("settings-navigate", section);
        }
        #[cfg(target_os = "macos")]
        let _ = app.show();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let query = section
        .map(|section| format!("index.html?view=settings&section={section}"))
        .unwrap_or_else(|| "index.html?view=settings".to_string());
    let window =
        tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App(query.into()))
            .title("设置")
            .inner_size(680.0, 560.0)
            .min_inner_size(600.0, 480.0)
            .resizable(true)
            .center()
            .build()
            .map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    let _ = app.show();
    let _ = window.set_focus();
    Ok(())
}

fn ensure_hook_binary(app: &AppHandle) -> Result<(), String> {
    copy_hook_binary(app)?;
    let wrapper = paths::hook_wrapper();
    let script = format!(
        "#!/bin/sh\nBIN=\"{}\"\n[ -x \"$BIN\" ] || exit 0\n\"$BIN\" \"$@\" 2>/dev/null || exit 0\nexit 0\n",
        paths::hook_binary().display()
    );
    fs::write(&wrapper, script).map_err(|error| format!("写入 hook wrapper 失败: {error}"))?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn copy_hook_binary(app: &AppHandle) -> Result<(), String> {
    let source = hook_source(app)?;
    fs::create_dir_all(paths::hook_bin_dir()).map_err(|error| error.to_string())?;
    let binary = paths::hook_binary();
    let temp = paths::hook_bin_dir().join(format!(".irisport-hook-{}.tmp", std::process::id()));
    fs::copy(&source, &temp).map_err(|error| format!("安装 hook 二进制失败: {error}"))?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    fs::rename(&temp, &binary).map_err(|error| error.to_string())?;
    Ok(())
}

fn build_status(store: &Store) -> Result<AppStatusView, String> {
    let mut source_statuses = Vec::new();
    let runtime_ready = hook_runtime_ready();
    let checked_at = now_ms();
    for source in sources::ALL {
        let kind = sources::kind(source);
        let mut issue = None;
        let (configured, entries) = match match kind {
            SourceKind::Codex => hooks_config::configured(),
            SourceKind::KimiCode => kimi_code::configured(),
            SourceKind::WorkBuddy => workbuddy::configured(),
            _ => Ok((false, 0)),
        } {
            Ok(result) => result,
            Err(error) => {
                issue = Some(format!("配置无法读取：{error}"));
                (false, 0)
            }
        };
        if configured && !runtime_ready {
            issue = Some("Hook 执行文件缺失或不可执行".to_string());
        }
        if configured && issue.is_none() {
            match match kind {
                SourceKind::Codex => hooks_config::uses_current_wrapper(),
                SourceKind::KimiCode => kimi_code::uses_current_wrapper(),
                SourceKind::WorkBuddy => workbuddy::uses_current_wrapper(),
                _ => Ok(true),
            } {
                Ok(true) => {}
                Ok(false) => issue = Some("Hook 配置指向了旧路径".to_string()),
                Err(error) => issue = Some(format!("Hook 路径检查失败：{error}")),
            }
        }
        let expected_entries = match kind {
            SourceKind::Codex => hooks_config::expected_entries(),
            SourceKind::KimiCode => kimi_code::expected_entries(),
            SourceKind::WorkBuddy => workbuddy::expected_entries(),
            _ => 0,
        };
        let installed = match kind {
            SourceKind::Codex => Path::new("/Applications/ChatGPT.app").exists(),
            SourceKind::KimiCode => Path::new("/Applications/Kimi Code.app").exists(),
            SourceKind::WorkBuddy => Path::new("/Applications/WorkBuddy.app").exists(),
            _ => false,
        };
        let config_path = match kind {
            SourceKind::Codex => hooks_config::hooks_path(),
            SourceKind::KimiCode => kimi_code::config_path(),
            SourceKind::WorkBuddy => workbuddy::config_path(),
            _ => PathBuf::new(),
        };
        let last_event_at = store
            .source_meta_get(source, "last_event_at")?
            .and_then(|value| value.parse::<i64>().ok());
        let hooks_installed_at = store
            .source_meta_get(source, "hooks_installed_at")?
            .and_then(|value| value.parse::<i64>().ok());
        let access_verified = store
            .source_meta_get(source, "access_verified_at")?
            .is_some();
        let verified_current =
            current_config_verified(access_verified, last_event_at, hooks_installed_at);
        let rejected_count = store
            .source_meta_get(source, "rejected_count")?
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        source_statuses.push(SourceStatusView {
            source: source.to_string(),
            label: sources::label(source).to_string(),
            description: sources::description(source).to_string(),
            installed,
            configured,
            update_available: configured && entries < expected_entries,
            access_verified,
            verified_current,
            runtime_ready,
            issue,
            checked_at,
            supports_completion: sources::accepts_events(source),
            supports_open: sources::supports_open(source),
            supports_auto_read: sources::read_signal(source) == ReadSignal::Trusted,
            last_event_at,
            rejected_count,
            config_path: config_path.display().to_string(),
        });
    }

    Ok(AppStatusView {
        sources: source_statuses,
        data_path: paths::support_dir().display().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

fn hook_runtime_ready() -> bool {
    [&paths::hook_binary(), &paths::hook_wrapper()]
        .into_iter()
        .all(|path| executable_file(path))
}

fn executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn current_config_verified(
    access_verified: bool,
    last_event_at: Option<i64>,
    hooks_installed_at: Option<i64>,
) -> bool {
    access_verified
        && hooks_installed_at
            .map(|installed_at| {
                last_event_at
                    .map(|last_event| last_event >= installed_at)
                    .unwrap_or(false)
            })
            .unwrap_or(true)
}

fn run_open(command: &mut Command) -> OpenResultView {
    match command.output() {
        Ok(output) if output.status.success() => OpenResultView {
            ok: true,
            message: None,
        },
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            OpenResultView {
                ok: false,
                message: Some(if detail.is_empty() {
                    "系统未接受打开请求，提醒已保留。".to_string()
                } else {
                    format!("无法打开结果：{detail}。提醒已保留。")
                }),
            }
        }
        Err(error) => OpenResultView {
            ok: false,
            message: Some(format!("无法发起打开请求：{error}。提醒已保留。")),
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updated_hooks_require_a_new_real_event() {
        assert!(current_config_verified(true, Some(200), Some(100)));
        assert!(!current_config_verified(true, Some(100), Some(200)));
        assert!(!current_config_verified(true, None, Some(200)));
        assert!(current_config_verified(true, Some(100), None));
        assert!(!current_config_verified(false, Some(200), None));
    }

    #[test]
    fn executable_check_requires_a_file_and_execute_permission() {
        let path =
            std::env::temp_dir().join(format!("irisport-health-executable-{}", std::process::id()));
        fs::write(&path, "test").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!executable_file(&path));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(executable_file(&path));
        let _ = fs::remove_file(&path);
        assert!(!executable_file(&path));
    }
}
