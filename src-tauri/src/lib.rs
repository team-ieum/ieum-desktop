pub mod commands;
pub mod store;
pub mod supervisor;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use ieum_local::WatchTiming;
use ieum_local::ai::{AiRunner, Keychain};
use serde_json::json;
use tauri::{App, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use crate::commands::Keys;
use crate::store::Store;
use crate::supervisor::{AppEvent, Deps, Emit, Supervisor};

/// 로그인 자동 시작 때 붙는 인자 — 창 없이 메뉴 막대로만 뜬다.
const HIDDEN_ARG: &str = "--hidden";

/// 첫 실행이면 자동 시작을 켜고 `first_run_done`을 남긴다.
fn first_run(app: &App, store: &Store) {
    let mut s = match store.get_settings() {
        Ok(s) if !s.first_run_done => s,
        Ok(_) => return,
        Err(e) => return eprintln!("[ieum] 설정을 읽지 못했어요: {e}"),
    };
    // 개발 빌드 바이너리를 로그인 항목에 넣지 않는다.
    if cfg!(debug_assertions) {
        return eprintln!("[ieum] 개발 빌드라 첫 실행 자동 시작 등록을 건너뜀");
    }
    match app.autolaunch().enable() {
        Ok(()) => s.autostart = true,
        Err(e) => eprintln!("[ieum] 자동 시작을 켜지 못했어요: {e}"),
    }
    s.first_run_done = true;
    if let Err(e) = store.set_settings(&s) {
        eprintln!("[ieum] 설정을 저장하지 못했어요: {e}");
    }
}

fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::new(app.path().app_data_dir()?);
    let handle = app.handle().clone();
    let emit: Emit = Arc::new(move |e| {
        let sent = match e {
            AppEvent::Exec { workflow_id, event } => handle.emit(
                "exec-event",
                json!({"workflowId": workflow_id, "event": event}),
            ),
            AppEvent::WatchStatus {
                workflow_id,
                ok,
                message,
            } => handle.emit(
                "watch-status",
                json!({"workflowId": workflow_id, "ok": ok, "message": message}),
            ),
        };
        if let Err(e) = sent {
            eprintln!("[ieum] 이벤트를 보내지 못했어요: {e}");
        }
    });
    let deps = Deps {
        store: store.clone(),
        trash_dir: app.path().home_dir()?.join(".Trash"),
        ai: Arc::new(AiRunner::new()),
        emit,
        timing: WatchTiming::default(),
        recheck: Duration::from_secs(30),
    };
    let sup = tauri::async_runtime::block_on(async { Supervisor::start(deps) })?;
    app.manage(sup);
    app.manage(Keys(Arc::new(Keychain)));
    tray::create(app)?;
    first_run(app, &store);
    if !std::env::args().any(|a| a == HIDDEN_ARG) {
        tray::show_main(app.handle());
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(setup)
        // 창 닫기 = 숨기기(감시는 계속).
        .on_window_event(|w, e| {
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_workflows,
            commands::save_workflow,
            commands::delete_workflow,
            commands::validate_workflow,
            commands::run_now,
            commands::list_runs,
            commands::get_run,
            commands::undo_run,
            commands::detect_providers,
            commands::set_api_key,
            commands::delete_api_key,
            commands::get_settings,
            commands::set_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
