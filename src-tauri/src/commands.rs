//! 화면이 부르는 Tauri 명령 — 로직은 `store`·`supervisor`에 두고 여기는 얇게 감싼다.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use ieum_engine::{ValidationError, Workflow};
use ieum_local::UndoReport;
use ieum_local::ai::keys::ScopedKeys;
use ieum_local::ai::{ApiKind, KeyStore, detect_clis};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

use crate::active::{Active, NOT_SIGNED_IN};
use crate::store::{RunSummary, Settings, Store, StoredRun, StoredWorkflow};
use crate::supervisor::WorkflowEntry;
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

#[tauri::command]
pub async fn get_run(active: State<'_, Active>, run_id: String) -> Result<StoredRun, String> {
    active.sup().await?.store().get_run(&run_id)
}

#[tauri::command]
pub async fn undo_run(active: State<'_, Active>, run_id: String) -> Result<UndoReport, String> {
    active.sup().await?.undo_run(&run_id)
}

/// 활성 계정 범위의 키 저장소. 로그인 전이면 `NOT_SIGNED_IN`.
async fn account_keys(active: &Active, keys: &Keys) -> Result<Arc<dyn KeyStore>, String> {
    let key16 = active
        .account_key()
        .await
        .ok_or_else(|| NOT_SIGNED_IN.to_string())?;
    Ok(Arc::new(ScopedKeys::new(keys.0.clone(), &key16)))
}

/// 구독 CLI는 claude만(codex·gemini 보류).
#[tauri::command]
pub async fn detect_providers(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
) -> Result<Providers, String> {
    let keys = account_keys(&active, &keys).await?;
    tauri::async_runtime::spawn_blocking(move || Providers {
        clis: detect_clis()
            .into_iter()
            .filter(|c| c.id == "cli:claude")
            .map(|c| CliInfo {
                id: c.id.to_string(),
                path: c.path,
                version: c.version,
            })
            .collect(),
        api_keys: [ApiKind::Anthropic, ApiKind::OpenAi, ApiKind::Gemini]
            .into_iter()
            .map(ApiKind::account)
            .filter(|a| match keys.get(a) {
                Ok(k) => k.is_some(),
                Err(e) => {
                    eprintln!("[ieum] 키를 확인하지 못했어요: {a}: {e}");
                    false
                }
            })
            .map(String::from)
            .collect(),
    })
    .await
    .map_err(|e| e.to_string())
}

/// API 키 계정만(같은 키체인 서비스의 `ieum-session`은 화면이 건드리지 못하게).
fn api_account(provider: &str) -> Result<&'static str, String> {
    [ApiKind::Anthropic, ApiKind::OpenAi, ApiKind::Gemini]
        .into_iter()
        .map(ApiKind::account)
        .find(|a| *a == provider)
        .ok_or_else(|| format!("알 수 없는 공급원이에요: {provider}"))
}

// 키체인을 만지는 명령은 async + spawn_blocking — 동기 명령은 메인 스레드에서 돌아
// 키체인 허용 창을 기다리는 동안 화면이 굳는다(tests/commands_off_main.rs).
#[tauri::command]
pub async fn set_api_key(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    provider: String,
    key: String,
) -> Result<(), String> {
    let account = api_account(&provider)?;
    let keys = account_keys(&active, &keys).await?;
    tauri::async_runtime::spawn_blocking(move || keys.set(account, &key))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_api_key(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    provider: String,
) -> Result<(), String> {
    let account = api_account(&provider)?;
    let keys = account_keys(&active, &keys).await?;
    tauri::async_runtime::spawn_blocking(move || keys.delete(account))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 키체인 계정의 저장 여부. 값은 담지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyState {
    Stored,
    Missing,
    /// 키체인 접근 오류
    Error,
}

/// API 키 3개와 이음 세션(`ieum-session`)의 저장 여부.
pub fn key_states(keys: &dyn KeyStore) -> BTreeMap<&'static str, KeyState> {
    [ApiKind::Anthropic, ApiKind::OpenAi, ApiKind::Gemini]
        .into_iter()
        .map(ApiKind::account)
        .chain([web::SESSION_ACCOUNT])
        .map(|a| {
            let state = match keys.get(a) {
                Ok(Some(_)) => KeyState::Stored,
                Ok(None) => KeyState::Missing,
                Err(e) => {
                    eprintln!("[ieum] 키체인을 확인하지 못했어요: {a}: {e}");
                    KeyState::Error
                }
            };
            (a, state)
        })
        .collect()
}

#[tauri::command]
pub async fn keychain_status(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
) -> Result<BTreeMap<&'static str, KeyState>, String> {
    let keys = account_keys(&active, &keys).await?;
    tauri::async_runtime::spawn_blocking(move || key_states(keys.as_ref()))
        .await
        .map_err(|e| e.to_string())
}

/// "웹에서 수정하기" 주소 — 허용된 서버 주소 + `/main`.
pub fn web_editor_url(server_url: &str) -> Result<String, String> {
    Ok(format!("{}/main", web::validate_server_url(server_url)?))
}

/// 기본 브라우저로 연다. 주소는 화면이 아니라 저장된 설정에서 만든다.
// ponytail: macOS `open`만(1단계는 macOS 전용) — 다른 OS를 내면 opener 플러그인으로.
#[tauri::command]
pub fn open_web_editor(active: State<'_, Active>) -> Result<(), String> {
    let url = web_editor_url(&active.settings().get_settings()?.server_url)?;
    let status = std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .status()
        .map_err(|e| format!("브라우저를 열지 못했어요: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("브라우저를 열지 못했어요: {status}"))
    }
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

/// 비밀번호는 BE로만 보내고 저장하지 않는다.
#[tauri::command]
pub async fn web_login(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    email: String,
    password: String,
) -> Result<WebAccount, WebError> {
    be_client(&active, &keys)?.login(&email, &password).await
}

#[tauri::command]
pub async fn web_logout(
    active: State<'_, Active>,
    keys: State<'_, Keys>,
    watches: State<'_, Watches>,
) -> Result<(), WebError> {
    watches.clear();
    be_client(&active, &keys)?.logout().await
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
