pub mod account;
pub mod active;
pub mod builder_host;
pub mod channel;
pub mod chat;
pub mod commands;
pub mod credentials;
pub mod limit_watch;
pub mod models;
pub mod notify;
pub mod permissions;
pub mod pick;
pub mod plan_usage;
pub mod pricing;
pub mod skill;
pub mod store;
pub mod sub_usage;
pub mod supervisor;
pub mod terminal;
mod tray;
pub mod usage;
pub mod web;

use std::sync::Arc;
use std::time::Duration;

use ieum_local::WatchTiming;
use ieum_local::ai::{AiRunner, KeyStore, Keychain};
use serde_json::json;
use tauri::webview::NewWindowResponse;
use tauri::{App, Emitter, Manager, Url, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::active::{Active, BaseDeps};
use crate::commands::Keys;
use crate::store::Store;
use crate::supervisor::{AppEvent, Emit};

/// 로그인 자동 시작 때 붙는 인자 — 창 없이 메뉴 막대로만 뜬다.
const HIDDEN_ARG: &str = "--hidden";

/// 새 버전이 있으면 받아 설치해 둔다(다음 실행부터 적용). 설치한 버전, 최신이면 None.
/// 공개키가 자리표시자면 서명 확인에서 실패해 설치되지 않는다.
pub(crate) async fn install_update(
    handle: &tauri::AppHandle,
) -> Result<Option<String>, tauri_plugin_updater::Error> {
    use tauri_plugin_updater::UpdaterExt;
    let Some(update) = handle.updater()?.check().await? else {
        return Ok(None);
    };
    update.download_and_install(|_, _| {}, || {}).await?;
    eprintln!(
        "[ieum] 새 버전 {}을 설치했어요(다음 실행부터 적용)",
        update.version
    );
    Ok(Some(update.version))
}

/// 시작할 때 자동 확인. 실패는 로그만 — 팝업 없음.
fn check_update(app: &App) {
    if cfg!(debug_assertions) {
        return;
    }
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = install_update(&handle).await {
            eprintln!("[ieum] 업데이트 확인 실패: {e}");
        }
    });
}

/// 앱 화면 주소인가 — 운영 `tauri://localhost`, 개발이면 `devUrl`과 같은 출처.
pub fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || dev_url.is_some_and(|d| d.origin() == url.origin())
}

/// 메인 창을 직접 만든다(`tauri.conf.json`은 `create: false`) — 앱 밖 주소로 이동·새 창 열기를 막는다.
/// 외부 링크는 러스트 명령이 기본 브라우저로 연다(Task 16).
fn create_main_window(app: &App) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let dev_url = app
        .config()
        .build
        .dev_url
        .clone()
        .filter(|_| tauri::is_dev());
    WebviewWindowBuilder::from_config(app.handle(), &config)?
        .on_navigation(move |url| {
            let ok = is_app_url(url, dev_url.as_ref());
            if !ok {
                eprintln!(
                    "[ieum] 앱 밖 주소로 이동을 막았어요: {}",
                    url.origin().ascii_serialization()
                );
            }
            ok
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .build()?;
    Ok(())
}

fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    create_main_window(app)?;
    let app_data = app.path().app_data_dir()?;
    let store = Store::new(app_data.clone());
    // 알림 클릭 처리·모음은 감독이 실행을 시작하기(activate) 전에.
    app.manage(notify::Pending::default());
    if let Err(e) = notify::listen_clicks(app.handle()) {
        eprintln!("[ieum] 알림 클릭 처리를 걸지 못했어요: {e}");
    }
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
            AppEvent::QueueDropped { workflow_id, path } => {
                eprintln!("[ieum] 실행 줄이 넘쳐 버렸어요: 워크플로우 {workflow_id}, 파일 {path}");
                handle.emit(
                    "queue-dropped",
                    json!({"workflowId": workflow_id, "path": path}),
                )
            }
            AppEvent::RunNotice(n) => {
                notify::on_run_notice(&handle, &n);
                Ok(())
            }
            AppEvent::WorkflowChanged {
                workflow_id,
                updated_at,
            } => handle.emit(
                "workflow-changed",
                json!({"id": workflow_id, "updatedAt": updated_at}),
            ),
            AppEvent::FinishPending {
                workflow_id,
                workflow_name,
            } => handle.emit(
                "finish-pending",
                json!({"id": workflow_id, "name": workflow_name}),
            ),
            // 앱 시작 때 실패: 화면이 아직 듣지 않는다(웹뷰 로드 전·로그인 관문) — 여기서 바로 알린다.
            // 그 편집 화면이 떠 있을 수 없으니 화면 표시 여부는 묻지 않는다. 문구에 경로는 없다.
            AppEvent::TestUndoFailed {
                workflow_id,
                at_start: Some(name),
                ..
            } => {
                notify::on_test_undo_failed(&handle, &workflow_id, &name, false);
                Ok(())
            }
            AppEvent::TestUndoFailed {
                workflow_id,
                run_id,
                at_start: None,
            } => handle.emit(
                "test-undo-failed",
                json!({"id": workflow_id, "runId": run_id}),
            ),
        };
        if let Err(e) = sent {
            eprintln!("[ieum] 이벤트를 보내지 못했어요: {e}");
        }
    });
    let home = app.path().home_dir()?;
    // 키체인 항목 하나로 옮길 때 찾을 옛 계정별 항목 — 계정 폴더 이름(파일만 읽는다).
    let scopes = std::fs::read_dir(app_data.join("accounts"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|k| crate::account::is_valid_key(k))
        .collect();
    ieum_local::ai::keys::set_legacy_scopes(scopes);
    let keys: Arc<dyn KeyStore> = Arc::new(Keychain);
    let active = Active::new(BaseDeps {
        app_data,
        settings: store.clone(),
        trash_dir: home.join(".Trash"),
        home,
        keys: keys.clone(),
        emit,
        timing: WatchTiming::default(),
        recheck: Duration::from_secs(30),
        ai_factory: Arc::new(|keys| Arc::new(AiRunner::with_keys(keys))),
    });
    // 네트워크와 무관하게 마지막 계정을 바로 활성화(오프라인 첫 화면). 실패해도 앱은 뜬다(관문).
    // block_on 안에서 키체인을 만지지 않는다 — activate는 파일만 다룬다.
    match store.get_settings() {
        Ok(s) => {
            if let Some(id) = s.current_account
                && let Err(e) = tauri::async_runtime::block_on(active.activate(id))
            {
                eprintln!("[ieum] 저장된 계정을 활성화하지 못했어요: {e}");
            }
        }
        Err(e) => eprintln!("[ieum] 설정을 읽지 못했어요: {e}"),
    }
    app.manage(active);
    app.manage(Keys(keys));
    app.manage(crate::web::Watches::default());
    app.manage(crate::sub_usage::SubscriptionUsage::new()?);
    app.manage(crate::sub_usage::AntigravityUsageClient::new(
        Arc::new(crate::sub_usage::agy_login),
        Arc::new(crate::sub_usage::agy_usage_cli),
        crate::plan_usage::CACHE_TTL,
    ));
    app.manage(crate::limit_watch::PendingSuggest::default());
    crate::limit_watch::spawn(app.handle().clone());
    app.manage(pick::system_pick(app.handle().clone()));
    app.manage(terminal::Terms::default());
    app.manage(channel::app_gate(app.handle()));
    app.manage(channel::app_channel(app.handle()));
    tray::create(app)?;
    check_update(app);
    if !std::env::args().any(|a| a == HIDDEN_ARG) {
        tray::show_main(app.handle());
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 맨 먼저 등록(공식 문서): 두 번째 실행은 바로 끝나고 떠 있는 앱의 창을 보인다.
        // 로그인 자동 시작(`--hidden`)으로 온 두 번째 실행은 창을 띄우지 않는다.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|a| a == HIDDEN_ARG) {
                tray::show_main(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
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
            commands::usage_summary,
            commands::usage_report,
            commands::undo_run,
            commands::detect_providers,
            commands::list_credentials,
            commands::add_api_key,
            commands::rename_credential,
            commands::set_default_credential,
            commands::delete_credential,
            commands::open_web_editor,
            commands::get_settings,
            commands::set_settings,
            commands::web_login,
            commands::web_logout,
            commands::web_account,
            commands::current_account,
            commands::web_list_workflows,
            commands::web_get_workflow,
            commands::web_list_executions,
            commands::web_execution_logs,
            commands::web_watch_execution,
            commands::web_unwatch_execution,
            commands::web_send_code,
            commands::web_verify_code,
            commands::web_register,
            commands::web_dashboard_summary,
            commands::web_dashboard_executions,
            commands::web_dashboard_errors,
            commands::open_help,
            commands::open_web_workflow,
            commands::begin_cli_account,
            commands::confirm_cli_account,
            commands::cancel_cli_account,
            commands::set_system_hidden,
            commands::subscription_usage,
            commands::antigravity_plan_usage,
            commands::take_account_suggestion,
            commands::open_system_settings,
            commands::open_provider_console,
            commands::send_test_notification,
            commands::open_data_folder,
            commands::check_update_now,
            // ── 캔버스 채팅: 토대 ──
            commands::get_chat,
            commands::pending_finish,
            commands::confirm_finish,
            // ── 캔버스 채팅 B(통로·동기화) — 여기 아래에 추가 ──
            commands::set_editing,
            commands::finish_notice,
            commands::undo_failed_notice,
            // ── 캔버스 채팅 C(터미널 term_*) — 여기 아래에 추가 ──
            terminal::term_spawn,
            terminal::term_write,
            terminal::term_resize,
            terminal::term_kill,
            terminal::term_tools,
            // ── 캔버스 채팅 D(채팅 패널 chat_*) — 여기 아래에 추가 ──
            chat::chat_send,
            chat::chat_pick_folder,
            models::list_models,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, e| {
            // 닫기 = 숨기기라 Dock 아이콘·Finder로 다시 열면 숨긴 창을 보인다.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = e {
                tray::show_main(app);
            }
            // 앱이 끝나면 터미널 세션도 전부 끝낸다.
            if let tauri::RunEvent::Exit = e {
                app.state::<terminal::Terms>().kill_all();
                app.state::<channel::Channel>().close();
            }
        });
}
