//! 화면이 부르는 Tauri 명령 — 로직은 `store`·`supervisor`에 두고 여기는 얇게 감싼다.

use std::path::PathBuf;
use std::sync::Arc;

use ieum_engine::{ValidationError, Workflow};
use ieum_local::UndoReport;
use ieum_local::ai::{KeyStore, detect_clis};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

use crate::account::AccountId;
use crate::active::Active;
use crate::credentials::{AccountCredentials, ApiKeyMeta, CredentialsView};
use crate::permissions;
use crate::plan_usage::{PlanUsage, PlanUsageClient, claude_account_email};
use crate::pricing;
use crate::store::{RunSummary, Settings, Store, StoredRun, StoredWorkflow};
use crate::sub_usage::{AntigravityUsageClient, CodexUsageClient};
use crate::supervisor::WorkflowEntry;
use crate::usage::{self, UsageSummary};
use crate::web::dashboard::{WebDashboardError, WebDashboardExecution, WebDashboardSummary};
use crate::web::executions::{Emit, Page, WebExecution, WebNodeLog};
use crate::web::workflows::{WebWorkflow, WebWorkflowSummary};
use crate::web::{self, BeClient, Watches, WebAccount, WebError};

/// API 키 저장소(운영 = 키체인).
pub struct Keys(pub Arc<dyn KeyStore>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub id: String,
    pub path: PathBuf,
    pub version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Providers {
    pub clis: Vec<CliInfo>,
    /// 키가 있는 공급원(`anthropic`·`openai`·`gemini`).
    pub api_keys: Vec<String>,
}

#[tauri::command]
pub async fn list_workflows(active: State<'_, Active>) -> Result<Vec<WorkflowEntry>, String> {
    active.sup().await?.list_workflows()
}

#[tauri::command]
pub async fn save_workflow(
    active: State<'_, Active>,
    wf: StoredWorkflow,
) -> Result<StoredWorkflow, String> {
    active.sup().await?.save_workflow(wf)
}

#[tauri::command]
pub async fn delete_workflow(active: State<'_, Active>, id: String) -> Result<(), String> {
    active.sup().await?.delete_workflow(&id)
}

#[tauri::command]
pub async fn validate_workflow(
    active: State<'_, Active>,
    wf: Workflow,
) -> Result<Vec<ValidationError>, String> {
    Ok(active.sup().await?.validate(&wf))
}

#[tauri::command]
pub async fn run_now(
    active: State<'_, Active>,
    id: String,
    file_path: String,
) -> Result<String, String> {
    active
        .sup()
        .await?
        .run_now(&id, &PathBuf::from(file_path))
        .await
}

#[tauri::command]
pub async fn list_runs(
    active: State<'_, Active>,
    workflow_id: Option<String>,
) -> Result<Vec<RunSummary>, String> {
    active
        .sup()
        .await?
        .store()
        .list_runs(workflow_id.as_deref())
}

/// 활성 계정의 AI 사용량 요약. `range` = `today`·`week`·`month`(로컬 시각대).
#[tauri::command]
pub async fn usage_summary(
    active: State<'_, Active>,
    range: String,
) -> Result<UsageSummary, String> {
    let sup = active.sup().await?;
    let now = chrono::Local::now();
    let from = usage::range_start(&range, now)?;
    let runs = sup.store().all_runs()?;
    Ok(usage::summarize(&runs, from, now, pricing::table()))
}

#[tauri::command]
pub async fn get_run(active: State<'_, Active>, run_id: String) -> Result<StoredRun, String> {
    active.sup().await?.store().get_run(&run_id)
}

#[tauri::command]
pub async fn undo_run(active: State<'_, Active>, run_id: String) -> Result<UndoReport, String> {
    active.sup().await?.undo_run(&run_id)
}

/// 구독 CLI는 claude만(codex·gemini 보류).
#[tauri::command]
pub async fn detect_providers(active: State<'_, Active>) -> Result<Providers, String> {
    with_credentials(&active, |c| {
        let view = c.list();
        if let Some(e) = &view.keychain_error {
            eprintln!("[ieum] 키를 확인하지 못했어요: {e}");
        }
        Ok(Providers {
            clis: detect_clis()
                .into_iter()
                .filter(|c| c.id == "cli:claude")
                .map(|c| CliInfo {
                    id: c.id.to_string(),
                    path: c.path,
                    version: c.version,
                })
                .collect(),
            api_keys: view
                .api_keys
                .into_iter()
                .filter(|(_, list)| !list.is_empty())
                .map(|(p, _)| p)
                .collect(),
        })
    })
    .await
}

// 키체인을 만지는 명령은 async + spawn_blocking — 동기 명령은 메인 스레드에서 돌아
// 키체인 허용 창을 기다리는 동안 화면이 굳는다(tests/commands_off_main.rs).
async fn with_credentials<T: Send + 'static>(
    active: &Active,
    f: impl FnOnce(AccountCredentials) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let c = active.credentials().await?;
    tauri::async_runtime::spawn_blocking(move || f(c))
        .await
        .map_err(|e| e.to_string())?
}

/// API 키·구독 계정 목록. 키 값은 담지 않는다(끝 4자만).
#[tauri::command]
pub async fn list_credentials(active: State<'_, Active>) -> Result<CredentialsView, String> {
    with_credentials(&active, |c| Ok(c.list())).await
}

#[tauri::command]
pub async fn add_api_key(
    active: State<'_, Active>,
    provider: String,
    label: String,
    key: String,
) -> Result<ApiKeyMeta, String> {
    with_credentials(&active, move |c| c.add_api_key(&provider, &label, &key)).await
}

/// `kind` = `anthropic|openai|gemini|claude|codex`.
#[tauri::command]
pub async fn rename_credential(
    active: State<'_, Active>,
    kind: String,
    id: String,
    label: String,
) -> Result<(), String> {
    with_credentials(&active, move |c| c.rename(&kind, &id, &label)).await
}

#[tauri::command]
pub async fn set_default_credential(
    active: State<'_, Active>,
    kind: String,
    id: String,
) -> Result<(), String> {
    with_credentials(&active, move |c| c.set_default(&kind, &id)).await
}

#[tauri::command]
pub async fn delete_credential(
    active: State<'_, Active>,
    kind: String,
    id: String,
) -> Result<(), String> {
    with_credentials(&active, move |c| c.delete(&kind, &id)).await
}

/// Claude 구독 사용량 + 로그인 계정(`~/.claude.json`). 키체인 읽기는 `PlanUsageClient` 안에서 `spawn_blocking`.
#[tauri::command]
pub async fn claude_plan_usage(client: State<'_, PlanUsageClient>) -> Result<PlanUsage, String> {
    let mut u = client.get().await;
    u.account_email = tauri::async_runtime::spawn_blocking(|| {
        claude_account_email(&std::env::home_dir()?.join(".claude.json"))
    })
    .await
    .ok()
    .flatten();
    Ok(u)
}

/// Codex 구독 연결 상태(계정·사용량). 파일 읽기는 안에서 `spawn_blocking`.
#[tauri::command]
pub async fn codex_plan_usage(client: State<'_, CodexUsageClient>) -> Result<PlanUsage, String> {
    Ok(client.get().await)
}

/// Antigravity 구독 연결 상태. 키체인 읽기·`agy` 실행은 안에서 `spawn_blocking`.
#[tauri::command]
pub async fn antigravity_plan_usage(
    client: State<'_, AntigravityUsageClient>,
) -> Result<PlanUsage, String> {
    Ok(client.get().await)
}

/// "웹에서 수정하기" 주소 — 허용된 서버 주소 + `/main`.
pub fn web_editor_url(server_url: &str) -> Result<String, String> {
    Ok(format!("{}/main", web::validate_server_url(server_url)?))
}

/// 웹 워크플로우 화면 주소 — 허용된 서버 + `/workflow/{id}`. `id`는 UUID 정규형(소문자·하이픈)만.
pub fn web_workflow_url(server_url: &str, id: &str) -> Result<String, String> {
    let canonical = uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id);
    if !canonical {
        return Err("잘못된 워크플로우 id예요.".into());
    }
    Ok(format!(
        "{}/workflow/{id}",
        web::validate_server_url(server_url)?
    ))
}

const HELP_URL: &str = "https://github.com/team-ieum/ieum-desktop/blob/main/docs/INSTALL.md";

/// 기본 브라우저로 연다. 주소는 화면이 아니라 고정값·저장된 설정에서 만든다.
// ponytail: macOS `open`만(1단계는 macOS 전용) — 다른 OS를 내면 opener 플러그인으로.
fn open_url(url: &str) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|e| format!("브라우저를 열지 못했어요: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("브라우저를 열지 못했어요: {status}"))
    }
}

#[tauri::command]
pub fn open_web_editor(active: State<'_, Active>) -> Result<(), String> {
    open_url(&web_editor_url(
        &active.settings().get_settings()?.server_url,
    )?)
}

#[tauri::command]
pub fn open_web_workflow(active: State<'_, Active>, id: String) -> Result<(), String> {
    open_url(&web_workflow_url(
        &active.settings().get_settings()?.server_url,
        &id,
    )?)
}

#[tauri::command]
pub fn open_help() -> Result<(), String> {
    open_url(HELP_URL)
}

/// 정해진 시스템 설정 창만 연다(`files`·`notifications`·`login_items`).
#[tauri::command]
pub fn open_system_settings(app: AppHandle, pane: String) -> Result<(), String> {
    let url = permissions::system_settings_url(&pane, &app.config().identifier)
        .ok_or("모르는 설정 창이에요.")?;
    open_url(&url)
}

/// 앞 여부·알림 토글과 무관하게 테스트 알림을 보낸다.
#[tauri::command]
pub fn send_test_notification(app: AppHandle) -> Result<(), String> {
    crate::notify::send_test(&app)
}

#[tauri::command]
pub fn get_settings(active: State<'_, Active>) -> Result<Settings, String> {
    active.settings().get_settings()
}

/// 자동 시작이 바뀌면 로그인 항목도 켜고 끈다. 서버 주소가 바뀌면 세션을 지우고 구독도 해제한다.
#[tauri::command]
pub async fn set_settings(
    app: AppHandle,
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    watches: State<'_, Watches>,
    s: Settings,
) -> Result<(), String> {
    web::validate_server_url(&s.server_url)?;
    let old = active.settings().get_settings()?;
    if s.autostart != old.autostart {
        let launch = app.autolaunch();
        let r = if s.autostart {
            launch.enable()
        } else {
            launch.disable()
        };
        r.map_err(|e| format!("자동 시작을 바꾸지 못했어요: {e}"))?;
    }
    let saved = save_screen_settings(active.settings(), keys.0.as_ref(), s)?;
    if saved.server_url != old.server_url {
        watches.clear();
    }
    Ok(())
}

/// 화면이 보낸 설정 저장. `current_account`는 로그인·로그아웃만 바꾼다 — 화면 값은 버리고 저장된 값을 둔다.
pub fn save_screen_settings(
    store: &Store,
    keys: &dyn KeyStore,
    mut s: Settings,
) -> Result<Settings, String> {
    s.current_account = store.get_settings()?.current_account;
    web::save_settings(store, keys, s)
}

fn be_client(active: &Active, keys: &Keys) -> Result<BeClient, WebError> {
    let s = active.settings().get_settings().map_err(WebError::Server)?;
    BeClient::new(&s.server_url, keys.0.clone())
}

/// 로그인하고 그 계정을 활성화한다. 활성화가 실패하면 로그인도 실패(문구에 경로·비밀 없음).
pub async fn login_and_activate(
    active: &Active,
    client: &BeClient,
    email: &str,
    password: &str,
) -> Result<WebAccount, WebError> {
    let acc = client.login(email, password).await?;
    let id = AccountId {
        email: acc.email.clone(),
        server_url: acc.server_url.clone(),
    };
    if let Err(e) = active.activate(id).await {
        eprintln!("[ieum] 계정을 열지 못했어요: {e}");
        // 반쯤 된 상태를 남기지 않는다 — 저장만 실패해 슬롯이 찼어도 비워야 같은 계정 재로그인이 다시 저장한다.
        if let Err(e) = active.deactivate().await {
            eprintln!("[ieum] 슬롯을 비우며 설정을 저장하지 못했어요: {e}");
        }
        if let Err(e) = client.logout().await {
            eprintln!("[ieum] 세션을 지우지 못했어요: {}", e.code());
        }
        return Err(WebError::Server("계정을 열지 못했어요".into()));
    }
    Ok(acc)
}

/// 명시적 로그아웃: 세션을 지우고(서버 실패여도) 감시를 멈춘다. 클라이언트를 못 만들었어도 멈춘다.
pub async fn logout_and_deactivate(
    active: &Active,
    client: Result<BeClient, WebError>,
) -> Result<(), WebError> {
    let r = match client {
        Ok(c) => c.logout().await,
        Err(e) => Err(e),
    };
    active.deactivate().await.map_err(WebError::Server)?;
    r
}

/// 관문 판단용 — 활성 슬롯, 없으면 저장된 계정. 네트워크·키체인 안 씀.
pub async fn account_of(active: &Active) -> Option<AccountId> {
    match active.current().await {
        Some(id) => Some(id),
        None => active.settings().get_settings().ok()?.current_account,
    }
}

/// 비밀번호는 BE로만 보내고 저장하지 않는다.
#[tauri::command]
pub async fn web_login(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    email: String,
    password: String,
) -> Result<WebAccount, WebError> {
    login_and_activate(&active, &be_client(&active, &keys)?, &email, &password).await
}

#[tauri::command]
pub async fn web_send_code(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    email: String,
) -> Result<(), WebError> {
    be_client(&active, &keys)?.send_signup_code(&email).await
}

#[tauri::command]
pub async fn web_verify_code(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    email: String,
    code: String,
) -> Result<(), WebError> {
    be_client(&active, &keys)?
        .verify_signup_code(&email, &code)
        .await
}

/// 가입만 — 로그인·활성화는 화면이 이어서 부르는 `web_login`이 한다(가입 성공·로그인 실패를 화면이 구분하게).
#[tauri::command]
pub async fn web_register(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    email: String,
    password: String,
    name: String,
) -> Result<(), WebError> {
    be_client(&active, &keys)?
        .register(&email, &password, &name)
        .await
}

#[tauri::command]
pub async fn web_logout(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    watches: State<'_, Watches>,
) -> Result<(), WebError> {
    watches.clear();
    logout_and_deactivate(&active, be_client(&active, &keys)).await
}

#[tauri::command]
pub async fn current_account(active: State<'_, Active>) -> Result<Option<AccountId>, String> {
    Ok(account_of(&active).await)
}

#[tauri::command]
pub async fn web_account(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
) -> Result<Option<WebAccount>, WebError> {
    let client = be_client(&active, &keys)?;
    tauri::async_runtime::spawn_blocking(move || client.account())
        .await
        .map_err(|e| WebError::Server(e.to_string()))
}

#[tauri::command]
pub async fn web_list_workflows(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
) -> Result<Vec<WebWorkflowSummary>, WebError> {
    web::workflows::list(&be_client(&active, &keys)?).await
}

#[tauri::command]
pub async fn web_get_workflow(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    id: String,
) -> Result<WebWorkflow, WebError> {
    web::workflows::get(&be_client(&active, &keys)?, &id).await
}

#[tauri::command]
pub async fn web_list_executions(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    workflow_id: String,
    cursor: Option<String>,
) -> Result<Page<WebExecution>, WebError> {
    web::executions::list(&be_client(&active, &keys)?, &workflow_id, cursor).await
}

#[tauri::command]
pub async fn web_execution_logs(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    workflow_id: String,
    execution_id: String,
) -> Result<Vec<WebNodeLog>, WebError> {
    web::executions::logs(&be_client(&active, &keys)?, &workflow_id, &execution_id).await
}

#[tauri::command]
pub async fn web_dashboard_summary(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
) -> Result<WebDashboardSummary, WebError> {
    web::dashboard::summary(&be_client(&active, &keys)?).await
}

#[tauri::command]
pub async fn web_dashboard_executions(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    cursor: Option<String>,
) -> Result<Page<WebDashboardExecution>, WebError> {
    web::dashboard::executions(&be_client(&active, &keys)?, cursor).await
}

#[tauri::command]
pub async fn web_dashboard_errors(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    cursor: Option<String>,
) -> Result<Page<WebDashboardError>, WebError> {
    web::dashboard::errors(&be_client(&active, &keys)?, cursor).await
}

/// 받은 이벤트를 `web-exec-event`로 넘긴다(async — 구독 작업을 tokio 런타임에 띄운다).
#[tauri::command]
pub async fn web_watch_execution(
    app: AppHandle,
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    watches: State<'_, Watches>,
    workflow_id: String,
    execution_id: String,
) -> Result<(), WebError> {
    let client = Arc::new(be_client(&active, &keys)?);
    let emit: Emit = Arc::new(move |payload| {
        if let Err(e) = app.emit("web-exec-event", payload) {
            eprintln!("[ieum] 이벤트를 보내지 못했어요: {e}");
        }
    });
    watches.watch(client, &workflow_id, &execution_id, emit)
}

#[tauri::command]
pub fn web_unwatch_execution(watches: State<'_, Watches>, execution_id: String) {
    watches.unwatch(&execution_id);
}
