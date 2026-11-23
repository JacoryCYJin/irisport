use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Position, Size, WebviewWindow};

pub fn toggle(app: &AppHandle, rect: Option<tauri::Rect>) {
    let Some(window) = app.get_webview_window("panel") else {
        return;
    };

    if window.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }

    if let Some(rect) = rect {
        position(&window, rect);
    }

    #[cfg(target_os = "macos")]
    {
        let _ = app.show();
    }

    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("panel-opened", ());
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("panel") {
        let _ = window.hide();
    }
}

pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window("panel") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.set_focus();
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app.show();
    }
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("panel-opened", ());
}

fn position(window: &WebviewWindow, rect: tauri::Rect) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let (x, y, width, height) = match (rect.position, rect.size) {
        (Position::Physical(position), Size::Physical(size)) => (
            position.x as f64,
            position.y as f64,
            size.width as f64,
            size.height as f64,
        ),
        (Position::Physical(position), Size::Logical(size)) => (
            position.x as f64,
            position.y as f64,
            size.width * scale,
            size.height * scale,
        ),
        (Position::Logical(position), Size::Physical(size)) => (
            position.x * scale,
            position.y * scale,
            size.width as f64,
            size.height as f64,
        ),
        (Position::Logical(position), Size::Logical(size)) => {
            (position.x, position.y, size.width, size.height)
        }
    };

    let window_size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(380, 520));
    let panel_width = window_size.width as f64;
    let panel_height = window_size.height as f64;

    let mut left = x + width / 2.0 - panel_width / 2.0;
    let mut top = y + height + 6.0;

    if let Ok(Some(monitor)) = window.monitor_from_point(x + width / 2.0, y + height / 2.0) {
        let area = monitor.work_area();
        let area_left = area.position.x as f64;
        let area_top = area.position.y as f64;
        let area_right = area_left + area.size.width as f64;
        let area_bottom = area_top + area.size.height as f64;
        left = left.clamp(
            area_left + 6.0,
            (area_right - panel_width - 6.0).max(area_left + 6.0),
        );
        top = top.clamp(
            area_top + 6.0,
            (area_bottom - panel_height - 6.0).max(area_top + 6.0),
        );
    }

    let _ = window.set_position(PhysicalPosition::new(
        left.round() as i32,
        top.round() as i32,
    ));
}
