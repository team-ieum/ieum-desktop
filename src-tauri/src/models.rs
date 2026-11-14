//! 모델 드롭다운의 목록. 공급원마다 출처가 다르다:
//! API 키 3사·Claude 구독 = 앱 가격표(금액을 셀 수 있는 모델), Codex = `codex debug models --bundled`(네트워크 없음),
//! Antigravity = `agy models`(모델 목록 조회 — 생성 호출 아님).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ieum_local::ai::shell_output;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
}

const LIST_TIMEOUT: Duration = Duration::from_secs(20);

/// 가격표에서 이름이 `prefix`로 시작하는 모델, 표의 순서대로.
pub fn priced(prefix: &str) -> Vec<ModelChoice> {
    let v: Value = serde_json::from_str(include_str!("../pricing.json")).unwrap_or_default();
    v["models"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| id.starts_with(prefix))
        .map(|(id, _)| ModelChoice {
            id: id.clone(),
            label: id.clone(),
        })
        .collect()
}

/// `codex debug models --bundled` 출력 → 목록에 보이는 모델(`visibility: "list"`)만, 출력 순서대로.
pub fn parse_codex(json: &str) -> Result<Vec<ModelChoice>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let models = v["models"].as_array().ok_or("models가 없어요")?;
    Ok(models
        .iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| {
            let id = m["slug"].as_str()?.to_string();
            let label = m["display_name"].as_str().unwrap_or(&id).to_string();
            Some(ModelChoice { id, label })
        })
        .collect())
}

/// `agy models` 출력(`<id>\t<이름>` 줄들, 앞의 안내 줄은 무시) → 목록.
pub fn parse_agy(text: &str) -> Vec<ModelChoice> {
    text.lines()
        .filter_map(|l| {
            let (id, label) = l.split_once('\t')?;
            let (id, label) = (id.trim(), label.trim());
            (!id.is_empty()).then(|| ModelChoice {
                id: id.into(),
                label: if label.is_empty() {
                    id.into()
                } else {
                    label.into()
                },
            })
        })
        .collect()
}

fn codex_models() -> Result<Vec<ModelChoice>, String> {
    // 출력이 수백 KB라 파이프(`shell_output` 64KB 한도)가 아니라 파일로 받는다.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let file = std::env::temp_dir().join(format!(
        "ieum-codex-models-{}-{}.json",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let line = format!(
        "codex debug models --bundled > '{}'",
        file.to_string_lossy().replace('\'', r"'\''")
    );
    let ran = shell_output(&line, LIST_TIMEOUT);
    let raw = std::fs::read_to_string(&file);
    let _ = std::fs::remove_file(&file);
    match (ran, raw) {
        (Some(_), Ok(raw)) => parse_codex(&raw),
        _ => Err("codex 모델 목록을 읽지 못했어요".into()),
    }
}

fn agy_models() -> Result<Vec<ModelChoice>, String> {
    let out = shell_output("agy models", LIST_TIMEOUT).ok_or("agy 모델 목록을 읽지 못했어요")?;
    Ok(parse_agy(&out))
}

/// 공급원의 모델 목록. 모르는 공급원은 Err. 빈 목록도 Err — 화면이 "기본 모델"만 보이며 알린다.
pub fn list(provider: &str) -> Result<Vec<ModelChoice>, String> {
    let models = match provider {
        "api:anthropic" | "cli:claude" => priced("claude-"),
        "api:openai" => priced("gpt-"),
        "api:gemini" => priced("gemini-"),
        "cli:codex" => codex_models()?,
        "cli:agy" => agy_models()?,
        _ => return Err(format!("모르는 공급원이에요: {provider}")),
    };
    if models.is_empty() {
        return Err("모델 목록이 비어 있어요".into());
    }
    Ok(models)
}

/// 성공한 목록만 앱이 켜져 있는 동안 기억한다(CLI 목록은 띄우는 데 몇 초 걸린다).
static CACHE: Mutex<Option<HashMap<String, Vec<ModelChoice>>>> = Mutex::new(None);

#[tauri::command]
pub async fn list_models(provider: String) -> Result<Vec<ModelChoice>, String> {
    let hit = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .get(&provider)
        .cloned();
    if let Some(m) = hit {
        return Ok(m);
    }
    let p = provider.clone();
    let models = tauri::async_runtime::spawn_blocking(move || list(&p))
        .await
        .map_err(|e| e.to_string())??;
    CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .insert(provider, models.clone());
    Ok(models)
}
