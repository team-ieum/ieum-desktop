//! 화면이 부르는 Tauri 명령 — 로직은 `store`·`supervisor`에 두고 여기는 얇게 감싼다.

use std::path::PathBuf;
use std::sync::Arc;

use ieum_engine::{ValidationError, Workflow};
use ieum_local::UndoReport;
use ieum_local::ai::{ApiKind, KeyStore, detect_clis};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::store::{RunSummary, Settings, StoredRun, StoredWorkflow};
use crate::supervisor::{Supervisor, WorkflowEntry};

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
pub fn list_workflows(sup: State<'_, Supervisor>) -> Result<Vec<WorkflowEntry>, String> {
    sup.list_workflows()
}

#[tauri::command]
pub fn save_workflow(
    sup: State<'_, Supervisor>,
    wf: StoredWorkflow,
) -> Result<StoredWorkflow, String> {
    sup.save_workflow(wf)
}

#[tauri::command]
pub fn delete_workflow(sup: State<'_, Supervisor>, id: String) -> Result<(), String> {
    sup.delete_workflow(&id)
}

#[tauri::command]
pub fn validate_workflow(sup: State<'_, Supervisor>, wf: Workflow) -> Vec<ValidationError> {
    sup.validate(&wf)
}

#[tauri::command]
pub async fn run_now(
    sup: State<'_, Supervisor>,
    id: String,
    file_path: String,
) -> Result<String, String> {
    sup.run_now(&id, &PathBuf::from(file_path)).await
}

#[tauri::command]
pub fn list_runs(
    sup: State<'_, Supervisor>,
    workflow_id: Option<String>,
) -> Result<Vec<RunSummary>, String> {
    sup.store().list_runs(workflow_id.as_deref())
}

#[tauri::command]
pub fn get_run(sup: State<'_, Supervisor>, run_id: String) -> Result<StoredRun, String> {
    sup.store().get_run(&run_id)
}

#[tauri::command]
pub fn undo_run(sup: State<'_, Supervisor>, run_id: String) -> Result<UndoReport, String> {
    sup.undo_run(&run_id)
}

/// 구독 CLI는 claude만(codex·gemini 보류).
#[tauri::command]
pub async fn detect_providers(keys: State<'_, Keys>) -> Result<Providers, String> {
    let keys = keys.0.clone();
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

#[tauri::command]
pub fn set_api_key(keys: State<'_, Keys>, provider: String, key: String) -> Result<(), String> {
    keys.0.set(&provider, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_api_key(keys: State<'_, Keys>, provider: String) -> Result<(), String> {
    keys.0.delete(&provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(sup: State<'_, Supervisor>) -> Result<Settings, String> {
    sup.store().get_settings()
}

/// 자동 시작이 바뀌면 로그인 항목도 켜고 끈다.
#[tauri::command]
pub fn set_settings(app: AppHandle, sup: State<'_, Supervisor>, s: Settings) -> Result<(), String> {
    if s.autostart != sup.store().get_settings()?.autostart {
        let launch = app.autolaunch();
        let r = if s.autostart {
            launch.enable()
        } else {
            launch.disable()
        };
        r.map_err(|e| format!("자동 시작을 바꾸지 못했어요: {e}"))?;
    }
    sup.store().set_settings(&s)
}
