use crate::commands;
use crate::panel;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

pub const TRAY_ID: &str = "main";

pub fn create(app: &AppHandle, count: i64) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开面板", true, None::<&str>)?;
    let connect = MenuItem::with_id(app, "connect", "连接 Codex", true, None::<&str>)?;
    let disconnect = MenuItem::with_id(app, "disconnect", "断开 Codex", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Irisport", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &connect, &disconnect, &quit])?;

    let icon = Image::new_owned(icon_rgba(18), 18, 18);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Irisport")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => panel::toggle(app, None),
            "connect" => {
                let _ = commands::install_from_tray(app);
                panel::toggle(app, None);
            }
            "disconnect" => {
                let _ = commands::uninstall_from_tray(app);
                panel::toggle(app, None);
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

    set_count(app, count);
    Ok(())
}

fn icon_rgba(size: usize) -> Vec<u8> {
    let scale = 4;
    let high = size * scale;
    let center = high as f64 / 2.0;
    let ring_radius = high as f64 * 0.31;
    let ring_width = high as f64 * 0.105;
    let dot_radius = high as f64 * 0.105;

    let mut high_pixels = vec![0u8; high * high * 4];
    for y in 0..high {
        for x in 0..high {
            let dx = x as f64 + 0.5 - center;
            let dy = y as f64 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let inside = (distance - ring_radius).abs() <= ring_width / 2.0
                || distance <= dot_radius;
            if inside {
                let index = (y * high + x) * 4;
                high_pixels[index + 3] = 255;
            }
        }
    }

    let mut pixels = vec![0u8; size * size * 4];
    let count = (scale * scale) as u32;
    for y in 0..size {
        for x in 0..size {
            let mut alpha = 0u32;
            for dy in 0..scale {
                for dx in 0..scale {
                    let index = ((y * scale + dy) * high + (x * scale + dx)) * 4;
                    alpha += high_pixels[index + 3] as u32;
                }
            }
            let index = (y * size + x) * 4;
            pixels[index + 3] = (alpha / count) as u8;
        }
    }
    pixels
}

pub fn set_count(app: &AppHandle, count: i64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let title = if count > 0 {
        Some(count.to_string())
    } else {
        None
    };
    let _ = tray.set_title(title.as_deref());
    let tooltip = if count > 0 {
        format!("Irisport · {count} 个新结果")
    } else {
        "Irisport".to_string()
    };
    let _ = tray.set_tooltip(Some(tooltip.as_str()));
}
