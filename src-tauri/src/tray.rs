//! 메뉴 막대 아이콘: "열기"·"종료".

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager};

// ponytail: 한국어 상수 — i18n은 Task 16에서 사전으로 옮긴다.
const OPEN: &str = "열기";
const QUIT: &str = "종료";

pub fn create(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", OPEN, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", QUIT, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("IEUM")
        .menu(&menu)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
