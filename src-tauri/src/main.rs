#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod codex;
mod codex_state;
mod commands;
mod hooks_config;
mod migration;
mod panel;
mod paths;
mod sources;
mod store;
mod tray;

use store::Store;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            panel::toggle(app, None);
        }))
        .setup(|app| {
            setup(app).map_err(|error| {
                Box::new(std::io::Error::other(error)) as Box<dyn std::error::Error>
            })
        })
        .on_window_event(|window, event| {
            if window.label() == "panel" {
                if let tauri::WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::open_session,
            commands::install_codex_hooks,
            commands::uninstall_codex_hooks,
            commands::hook_status,
            commands::hide_panel,
            commands::set_panel_height,
            commands::open_settings,
            commands::open_codex_settings
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 Irisport");
}

fn setup(app: &mut tauri::App) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }

    paths::ensure_dirs().map_err(|error| error.to_string())?;
    let store = Store::open(paths::db_path())?;

    if let Err(error) = commands::refresh_hook_binary(app.handle()) {
        eprintln!("irisport: 更新 hook 二进制失败: {error}");
    }
    if let Err(error) = migration::run(&store) {
        eprintln!("irisport: 数据迁移未完成: {error}");
    }

    let count = store.count_unread().unwrap_or(0);

    if let Some(window) = app.get_webview_window("panel") {
        #[cfg(target_os = "macos")]
        {
            use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
            let _ = apply_vibrancy(
                &window,
                NSVisualEffectMaterial::Popover,
                None,
                Some(14.0),
            );
        }
    }

    app.manage(store.clone());
    tray::create(app.handle(), count).map_err(|error| error.to_string())?;
    codex::start(app.handle().clone(), store.clone());
    codex_state::start(app.handle().clone(), store);

    Ok(())
}
