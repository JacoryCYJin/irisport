use crate::commands;
use crate::panel;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

pub const TRAY_ID: &str = "main";

pub fn create(app: &AppHandle, count: i64) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Irisport", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &quit])?;

    let icon = icon_for_count(count)?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Irisport")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => panel::toggle(app, None),
            "settings" => {
                panel::hide(app);
                let _ = commands::show_settings(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                panel::toggle(tray.app_handle(), Some(rect));
            }
        })
        .build(app)?;

    set_count_labels(&tray, count);
    Ok(())
}

fn icon_for_count(count: i64) -> tauri::Result<Image<'static>> {
    let bytes: &[u8] = match count {
        ..=0 => include_bytes!("../icons/tray.png"),
        1 => include_bytes!("../icons/tray-lit.png"),
        2 => include_bytes!("../icons/tray-lit-medium.png"),
        _ => include_bytes!("../icons/tray-lit-high.png"),
    };
    Image::from_bytes(bytes)
}

pub fn set_count(app: &AppHandle, count: i64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(icon) = icon_for_count(count) {
        let _ = tray.set_icon_with_as_template(Some(icon), true);
    }
    set_count_labels(&tray, count);
}

fn set_count_labels(tray: &TrayIcon, count: i64) {
    // Clear any title persisted by an older build; the menu bar now uses only
    // the bulb brightness. The exact count stays available in the tooltip.
    let _ = tray.set_title(Some(""));
    let tooltip = if count > 0 {
        format!("Irisport · {count} 个待查看结果")
    } else {
        "Irisport · 暂无待查看结果".to_string()
    };
    let _ = tray.set_tooltip(Some(tooltip.as_str()));
}
