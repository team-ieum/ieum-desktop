//! 화면이 부르는 Tauri 명령 — 로직은 `store`·`supervisor`에 두고 여기는 얇게 감싼다.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ieum_engine::{ValidationError, Workflow};
use ieum_local::UndoReport;
use ieum_local::ai::{KeyStore, detect_clis};
use ieum_local::builder::{ChatFile, PendingFinish, Reply};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::account::AccountId;
use crate::active::{Active, NOT_SIGNED_IN};
use crate::builder_host::{PickFolder, builder_of};
use crate::channel::Channel;
use crate::credentials::{AccountCredentials, ApiKeyMeta, CliAccountBegin, CredentialsView};
use crate::permissions;
use crate::plan_usage::PlanUsage;
use crate::pricing;
use crate::store::{RunSummary, Settings, Store, StoredRun, StoredWorkflow};
use crate::sub_usage::{AntigravityUsageClient, SubscriptionUsage};
use crate::supervisor::{Supervisor, WorkflowEntry};
use crate::terminal::Terms;
use crate::usage::{self, UsageReport, UsageSummary};
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
    save_from_canvas(&active.sup().await?, wf)
}

/// 캔버스 저장. 화면이 읽은 `wf.updated_at`이 저장본과 다르면 `STALE` — 그사이 빌더가 고친 것을 덮지 않는다.
/// id가 빈 새 워크플로우만 검사 없이 저장한다.
pub fn save_from_canvas(sup: &Supervisor, wf: StoredWorkflow) -> Result<StoredWorkflow, String> {
    if wf.id.is_empty() {
        return sup.save_workflow(wf);
    }
    let expected = wf.updated_at.clone();
    sup.save_workflow_expecting(wf, &expected)
}

#[tauri::command]
pub async fn delete_workflow(
    app: AppHandle,
    active: State<'_, Active>,
    channel: State<'_, Channel>,
    terms: State<'_, Terms>,
    id: String,
) -> Result<(), String> {
    let key = active.account_key().await.ok_or(NOT_SIGNED_IN)?;
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let work = crate::terminal::work_root(&cache, &key);
    delete_workflow_and_sessions(&active, &channel, &terms, &work, &id).await
}

/// 워크플로우를 지우며 그 세션도 끝낸다: 토큰을 먼저 버려 지우는 사이에 온 명령이 닿지 못하게 하고,
/// 지운 뒤 터미널을 끝내고, 그 계정에 살아 있는 세션이 없으면 작업 폴더를 비운다(세션 없이 남은 것 포함).
pub async fn delete_workflow_and_sessions(
    active: &Active,
    channel: &Channel,
    terms: &Terms,
    work_root: &Path,
    id: &str,
) -> Result<(), String> {
    channel.revoke_workflow(id);
    // 저장소가 id(UUID 정규형)를 검사한다 — 통과한 id만 폴더 이름이 된다.
    active.sup().await?.delete_workflow(id)?;
    terms.kill(id);
    terms.clear_work_if_idle(work_root);
    Ok(())
}

/// 로그아웃: 터미널을 전부 끝내고 토큰·소켓을 버린다.
pub fn end_all_sessions(channel: &Channel, terms: &Terms) {
    terms.kill_all();
    channel.close();
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
    let chats = sup.store().all_chat_usage()?;
    Ok(usage::summarize(&runs, &chats, from, now, pricing::table()))
}

/// 활성 계정의 통계 및 사용량 화면 — 합계·최근 30일 개요·공급자별 분석(로컬 시각대).
#[tauri::command]
pub async fn usage_report(active: State<'_, Active>) -> Result<UsageReport, String> {
    let sup = active.sup().await?;
    let store = sup.store();
    let runs = store.all_runs()?;
    let chats = store.all_chat_usage()?;
    let names = store
        .list_workflows()?
        .into_iter()
        .map(|w| (w.id, w.name))
        .collect();
    Ok(usage::report(
        &runs,
        &chats,
        &names,
        chrono::Local::now(),
        pricing::table(),
    ))
}

#[tauri::command]
pub async fn get_run(active: State<'_, Active>, run_id: String) -> Result<StoredRun, String> {
    active.sup().await?.store().get_run(&run_id)
}

#[tauri::command]
pub async fn undo_run(active: State<'_, Active>, run_id: String) -> Result<UndoReport, String> {
    active.sup().await?.undo_run(&run_id)
}

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

/// 구독 계정 추가 시작: 설정 폴더를 만들고 터미널에서 돌릴 로그인 명령을 준다. `kind` = `claude|codex`.
#[tauri::command]
pub async fn begin_cli_account(
    active: State<'_, Active>,
    kind: String,
    label: String,
) -> Result<CliAccountBegin, String> {
    with_credentials(&active, move |c| c.begin_cli_account(&kind, &label)).await
}

/// 로그인했는지 그 계정 폴더로 확인(키체인 허용 창이 여기서 뜰 수 있음). 이메일이 보이면 추가 완료.
#[tauri::command]
pub async fn confirm_cli_account(
    active: State<'_, Active>,
    usage: State<'_, SubscriptionUsage>,
    kind: String,
    id: String,
) -> Result<PlanUsage, String> {
    let c = Arc::new(active.credentials().await?);
    crate::credentials::confirm_cli_account(c, &usage, &kind, &id).await
}

#[tauri::command]
pub async fn cancel_cli_account(
    active: State<'_, Active>,
    kind: String,
    id: String,
) -> Result<(), String> {
    with_credentials(&active, move |c| c.cancel_cli_account(&kind, &id)).await
}

/// 이 Mac의 CLI 로그인을 IEUM에서 숨기기·되살리기(CLI 로그아웃 아님). `kind` = `claude|codex`.
#[tauri::command]
pub async fn set_system_hidden(
    active: State<'_, Active>,
    kind: String,
    hidden: bool,
) -> Result<(), String> {
    with_credentials(&active, move |c| c.set_system_hidden(&kind, hidden)).await
}

/// 구독 계정 하나의 연결 상태·사용량. 키체인·파일 읽기는 안에서 `spawn_blocking`.
#[tauri::command]
pub async fn subscription_usage(
    active: State<'_, Active>,
    usage: State<'_, SubscriptionUsage>,
    kind: String,
    id: String,
    fresh: Option<bool>,
) -> Result<PlanUsage, String> {
    let k = kind.clone();
    let dir = with_credentials(&active, move |c| c.cli_dir(&k, &id)).await?;
    if fresh == Some(true) {
        usage.refresh(&kind, dir).await
    } else {
        usage.get(&kind, dir).await
    }
}

/// 대기 중인 계정 권하기를 전부 꺼낸다(꺼내면 비운다). 다른 이음 계정 때 쌓인 것은 버린다.
#[tauri::command]
pub async fn take_account_suggestion(
    active: State<'_, Active>,
    pending: State<'_, crate::limit_watch::PendingSuggest>,
) -> Result<Vec<crate::limit_watch::Suggestion>, String> {
    Ok(pending.take(active.account_key().await.as_deref()))
}

/// Antigravity 구독 연결 상태. 키체인 읽기·`agy` 실행은 안에서 `spawn_blocking`.
#[tauri::command]
pub async fn antigravity_plan_usage(
    client: State<'_, AntigravityUsageClient>,
    fresh: Option<bool>,
) -> Result<PlanUsage, String> {
    Ok(if fresh == Some(true) {
        client.refresh().await
    } else {
        client.get().await
    })
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
pub async fn open_web_editor(active: State<'_, Active>) -> Result<(), String> {
    open_url(&web_editor_url(
        &active.settings().get_settings()?.server_url,
    )?)
}

#[tauri::command]
pub async fn open_web_workflow(active: State<'_, Active>, id: String) -> Result<(), String> {
    open_url(&web_workflow_url(
        &active.settings().get_settings()?.server_url,
        &id,
    )?)
}

#[tauri::command]
pub fn open_help() -> Result<(), String> {
    open_url(HELP_URL)
}

/// 공급자 키 발급 화면 주소 — 정해진 세 곳만. 화면이 준 주소는 받지 않는다.
pub fn provider_console_url(provider: &str) -> Result<&'static str, String> {
    match provider {
        "anthropic" => Ok("https://console.anthropic.com/settings/keys"),
        "openai" => Ok("https://platform.openai.com/api-keys"),
        "gemini" => Ok("https://aistudio.google.com/apikey"),
        _ => Err("모르는 공급자예요.".into()),
    }
}

#[tauri::command]
pub fn open_provider_console(provider: String) -> Result<(), String> {
    open_url(provider_console_url(&provider)?)
}

/// 정해진 시스템 설정 창만 연다(`files`·`notifications`·`login_items`).
#[tauri::command]
pub fn open_system_settings(app: AppHandle, pane: String) -> Result<(), String> {
    let url = permissions::system_settings_url(&pane, &app.config().identifier)
        .ok_or("모르는 설정 창이에요.")?;
    open_url(&url)
}

/// 데이터 폴더를 Finder로 여는 명령 — 화면이 준 경로는 받지 않는다.
pub fn data_folder_command(dir: &std::path::Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("/usr/bin/open");
    cmd.arg(dir);
    cmd
}

/// 설정 고급 [데이터 폴더 열기] — 앱 데이터 폴더(고정 경로)만.
#[tauri::command]
pub fn open_data_folder(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let status = data_folder_command(&dir)
        .status()
        .map_err(|e| format!("폴더를 열지 못했어요: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("폴더를 열지 못했어요: {status}"))
    }
}

/// 앞 여부·알림 토글과 무관하게 테스트 알림을 보낸다.
#[tauri::command]
pub fn send_test_notification(app: AppHandle) -> Result<(), String> {
    crate::notify::send_test(&app)
}

/// 설정 [업데이트 확인] — `dev`(개발 빌드라 안 함)·`latest`·`available`(+버전, 오른쪽 아래 알림에서 설치).
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum UpdateCheck {
    Dev,
    Latest,
    Available { version: String },
}

#[tauri::command]
pub async fn check_update_now(app: AppHandle) -> Result<UpdateCheck, String> {
    if cfg!(debug_assertions) {
        return Ok(UpdateCheck::Dev);
    }
    match crate::find_update(&app, true).await {
        Ok(Some(version)) => Ok(UpdateCheck::Available { version }),
        Ok(None) => Ok(UpdateCheck::Latest),
        Err(e) => {
            eprintln!("[ieum] 업데이트 확인 실패: {e}");
            Err("업데이트를 확인하지 못했어요. 네트워크를 확인해 주세요.".into())
        }
    }
}

/// 이미 찾아 둔 새 버전 — 이벤트를 놓친(나중에 뜬) 창이 알림을 띄우려고 읽는다.
#[tauri::command]
pub fn pending_update(pending: State<'_, crate::PendingUpdate>) -> Option<String> {
    pending.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 이미 받는 중이면 두 번째 [업데이트]를 거절한다 — 화면이 바뀌어 알림이 다시 그려져도 설치는 하나만.
static INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `update-progress` 이벤트. 받는 중이면 받은 바이트·전체(서버가 알려 주면), 다 받으면 `installing`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub installing: bool,
}

/// 알림 [업데이트] — 다시 확인해 받아 설치하고 앱을 다시 시작한다. 받는 동안 진행을 알린다.
/// 오류: `UP_TO_DATE`(그 사이 최신이 됐다 — 화면은 알림을 닫는다), `BUSY`, 그 밖은 사람이 읽는 글.
#[tauri::command]
pub async fn install_update_and_restart(app: AppHandle) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    use tauri_plugin_updater::UpdaterExt;
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("BUSY".into());
    }
    let fail = |e: tauri_plugin_updater::Error| {
        INSTALLING.store(false, Ordering::SeqCst);
        eprintln!("[ieum] 업데이트 설치 실패: {e}");
        "업데이트를 받지 못했어요. 네트워크를 확인해 주세요.".to_string()
    };
    let Some(update) = app.updater().map_err(fail)?.check().await.map_err(fail)? else {
        INSTALLING.store(false, Ordering::SeqCst);
        return Err("UP_TO_DATE".into());
    };
    let (mut downloaded, mut last_pct) = (0u64, u64::MAX);
    let (on_chunk, on_done) = (app.clone(), app.clone());
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                // 조각마다 보내면 이벤트가 수천 개 — 1%가 바뀔 때만(전체를 모르면 256KB마다).
                let pct = total.map_or(downloaded >> 18, |t| downloaded * 100 / t.max(1));
                if pct != last_pct {
                    last_pct = pct;
                    let _ = on_chunk.emit(
                        "update-progress",
                        UpdateProgress {
                            downloaded,
                            total,
                            installing: false,
                        },
                    );
                }
            },
            move || {
                let _ = on_done.emit(
                    "update-progress",
                    UpdateProgress {
                        downloaded: 0,
                        total: None,
                        installing: true,
                    },
                );
            },
        )
        .await
        .map_err(fail)?;
    app.restart()
}

#[cfg(test)]
mod update_tests {
    use super::UpdateCheck;

    #[test]
    fn update_check_serialises_with_status_tag() {
        let v = serde_json::to_value(UpdateCheck::Available {
            version: "1.2.3".into(),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"status": "available", "version": "1.2.3"})
        );
        assert_eq!(
            serde_json::to_value(UpdateCheck::Latest).unwrap(),
            serde_json::json!({"status": "latest"})
        );
    }
}

#[tauri::command]
pub async fn get_settings(active: State<'_, Active>) -> Result<Settings, String> {
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
    channel: State<'_, Channel>,
    terms: State<'_, Terms>,
    email: String,
    password: String,
) -> Result<WebAccount, WebError> {
    let before = active.account_key().await;
    // 활성화 도중 실패해도 이전 계정은 이미 내려갔을 수 있다 — 결과와 무관하게 정리한다.
    let acc = login_and_activate(&active, &be_client(&active, &keys)?, &email, &password).await;
    end_sessions_if_switched(before, &active, &channel, &terms).await;
    acc
}

/// 만료 뒤 관문에서 다른 계정으로 바뀌었으면 옛 계정의 터미널·토큰을 끝낸다(남은 CLI 프로세스).
/// 같은 계정 재로그인은 만료일 뿐 로그아웃이 아니라 그대로 둔다.
pub async fn end_sessions_if_switched(
    before: Option<String>,
    active: &Active,
    channel: &Channel,
    terms: &Terms,
) {
    if before.is_some() && active.account_key().await != before {
        end_all_sessions(channel, terms);
    }
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
    channel: State<'_, Channel>,
    terms: State<'_, Terms>,
) -> Result<(), WebError> {
    end_all_sessions(&channel, &terms);
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

/// 채팅 상태 파일(대화·테스트 상태). 없으면 빈 값.
#[tauri::command]
pub async fn get_chat(active: State<'_, Active>, workflow_id: String) -> Result<ChatFile, String> {
    active.sup().await?.store().get_chat(&workflow_id)
}

/// 사람의 확인을 기다리는 완료 요청(확인 카드 내용). 없으면 None.
#[tauri::command]
pub async fn pending_finish(
    active: State<'_, Active>,
    pick: State<'_, PickFolder>,
    workflow_id: String,
) -> Result<Option<PendingFinish>, String> {
    Ok(builder_of(&active, &pick).await?.pending(&workflow_id))
}

/// 확인 카드의 [켜기]/[아직] — 워크플로우를 켜는 유일한 길(사람이 누른다).
#[tauri::command]
pub async fn confirm_finish(
    active: State<'_, Active>,
    pick: State<'_, PickFolder>,
    workflow_id: String,
    accept: bool,
) -> Result<Reply, String> {
    Ok(builder_of(&active, &pick)
        .await?
        .confirm_finish(&workflow_id, accept)
        .await)
}

/// 편집기의 미저장 편집 표시 — 켜져 있는 동안 빌더의 고치는 동작은 `USER_EDITING`으로 거절된다(사람의 편집 우선).
#[tauri::command]
pub async fn set_editing(
    active: State<'_, Active>,
    workflow_id: String,
    editing: bool,
) -> Result<(), String> {
    active.sup().await?.set_editing(&workflow_id, editing);
    Ok(())
}

/// 완료 확인 대기 알림 — 화면이 `finish-pending`을 받으면 부른다. 그 편집 화면이 떠 있고 창이 앞이면 보내지 않는다.
#[tauri::command]
pub async fn finish_notice(
    app: AppHandle,
    active: State<'_, Active>,
    workflow_id: String,
    on_screen: bool,
) -> Result<(), String> {
    let name = active.sup().await?.store().get_workflow(&workflow_id)?.name;
    crate::notify::on_finish_pending(&app, &workflow_id, &name, on_screen);
    Ok(())
}

/// 테스트 되돌리기 실패 알림 — 화면이 `test-undo-failed`를 받으면 부른다. 판정은 `finish_notice`와 같다.
#[tauri::command]
pub async fn undo_failed_notice(
    app: AppHandle,
    active: State<'_, Active>,
    workflow_id: String,
    on_screen: bool,
) -> Result<(), String> {
    let name = active.sup().await?.store().get_workflow(&workflow_id)?.name;
    crate::notify::on_test_undo_failed(&app, &workflow_id, &name, on_screen);
    Ok(())
}
