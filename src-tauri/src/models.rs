//! 모델 드롭다운의 목록. 공급원마다 출처가 다르다:
//! API 키 3사·Claude 구독 = 앱 가격표(금액을 셀 수 있는 모델), Codex = `codex debug models --bundled`(네트워크 없음),
//! Antigravity = `agy models`(모델 목록 조회 — 생성 호출 아님).
//! 모델마다 고를 수 있는 추론 수준(`efforts`)도 같이 준다 — 비면 그 모델은 수준을 못 고른다.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ieum_local::ai::{agy_split, shell_output};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    /// 고를 수 있는 추론 수준, 낮은 것부터. 비면 못 고른다.
    pub efforts: Vec<String>,
}

/// 드롭다운 목록 + "기본 모델"일 때 고를 수 있는 수준.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    pub models: Vec<ModelChoice>,
    pub default_efforts: Vec<String>,
}

const LIST_TIMEOUT: Duration = Duration::from_secs(20);

fn owned(levels: &[&str]) -> Vec<String> {
    levels.iter().map(|s| s.to_string()).collect()
}

/// claude CLI `--effort`(도움말: low, medium, high, xhigh, max)와 Anthropic `output_config.effort`
/// (가격표의 Claude 모델 모두 다섯 수준) — 모델과 무관하게 같다.
const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
/// OpenAI Chat Completions `reasoning_effort`. `none`·`minimal`은 GPT-6 Astra·6.1 Sol이 받지 않아 뺀다
/// (OpenAI reasoning 안내). 나머지는 codex 내장 목록의 같은 모델 수준과 같다.
const OPENAI_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Gemini `thinkingLevel` — Gemini 생각 안내의 모델별 표. 표에 없는 모델(기본 `gemini-3.5-flash` 포함)은 고르지 않는다.
fn gemini_efforts(id: &str) -> Vec<String> {
    owned(match id {
        "gemini-4-carbon" | "gemini-4-argon" | "gemini-3.8-flash" => &["low", "medium", "high"],
        "gemini-3.6-flash" | "gemini-3.5-flash-lite" => &["minimal", "low", "medium", "high"],
        _ => &[],
    })
}

/// 가격표에서 이름이 `prefix`로 시작하는 모델, 표의 순서대로. 화면 이름은 표의 `name`, 없으면 `readable(id)`.
pub fn priced(prefix: &str) -> Vec<ModelChoice> {
    let v: Value = serde_json::from_str(include_str!("../pricing.json")).unwrap_or_default();
    v["models"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| id.starts_with(prefix))
        .map(|(id, p)| ModelChoice {
            id: id.clone(),
            label: p["name"]
                .as_str()
                .map_or_else(|| readable(id), String::from),
            efforts: match prefix {
                "claude-" => owned(&CLAUDE_EFFORTS),
                "gpt-" => owned(&OPENAI_EFFORTS),
                "gemini-" => gemini_efforts(id),
                _ => Vec::new(),
            },
        })
        .collect()
}

/// id → 읽기 쉬운 이름: `-`로 나눠 낱말은 첫 글자를 크게, 이어진 숫자 조각은 `.`로 잇는다.
/// `gpt` 뒤 판 번호는 `GPT-6.1`처럼 붙인다. (`claude-haiku-5-5` → `Claude Haiku 5.5`)
pub fn readable(id: &str) -> String {
    let num = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');
    let mut words: Vec<String> = Vec::new();
    let mut prev_num = false;
    for part in id.split('-').filter(|p| !p.is_empty()) {
        let is_num = num(part);
        match words.last_mut() {
            Some(w) if is_num && prev_num => *w += &format!(".{part}"),
            Some(w) if is_num && w == "GPT" => *w += &format!("-{part}"),
            _ if part == "gpt" => words.push("GPT".into()),
            _ => {
                let mut c = part.chars();
                let first = c.next().map(|f| f.to_uppercase().collect::<String>());
                words.push(first.unwrap_or_default() + c.as_str());
            }
        }
        prev_num = is_num;
    }
    words.join(" ")
}

/// codex 수준 중 목록에서 빼는 것. `ultra`는 "자동 작업 위임"(하위 에이전트)을 켠다 —
/// AI 노드는 도구를 모두 막고 돌리므로(`lock_codex_catalog`) 위임이 막히거나 차단과 어긋난다.
const CODEX_HIDDEN_EFFORTS: [&str; 1] = ["ultra"];

/// `codex debug models --bundled` 출력 → 목록에 보이는 모델(`visibility: "list"`)만, 출력 순서대로.
/// 기본 모델의 수준 = `priority`가 가장 낮은(가장 앞) 보이는 모델의 수준 — codex가 기본으로 고르는 모델.
pub fn parse_codex(json: &str) -> Result<ModelList, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let models = v["models"].as_array().ok_or("models가 없어요")?;
    let listed: Vec<&Value> = models
        .iter()
        .filter(|m| m["visibility"] == "list")
        .collect();
    let efforts = |m: &Value| -> Vec<String> {
        m["supported_reasoning_levels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l["effort"].as_str())
            .filter(|e| !CODEX_HIDDEN_EFFORTS.contains(e))
            .map(String::from)
            .collect()
    };
    let default_efforts = listed
        .iter()
        .min_by_key(|m| m["priority"].as_i64().unwrap_or(i64::MAX))
        .map(|m| efforts(m))
        .unwrap_or_default();
    Ok(ModelList {
        models: listed
            .iter()
            .filter_map(|m| {
                let id = m["slug"].as_str()?.to_string();
                let label = m["display_name"].as_str().unwrap_or(&id).to_string();
                Some(ModelChoice {
                    id,
                    label,
                    efforts: efforts(m),
                })
            })
            .collect(),
        default_efforts,
    })
}

/// `agy models` 출력(`<id>\t<이름>` 줄들, 앞의 안내 줄은 무시) → 목록.
/// `-low`·`-medium`·`-high`·`-xhigh`로 끝나는 id는 접미사를 떼어 한 모델로 묶고(처음 나온 자리), 수준을 모은다.
/// 이름의 끝 ` (Medium)` 같은 괄호도 뗀다. 접미사 없는 모델은 수준이 없다.
pub fn parse_agy(text: &str) -> Vec<ModelChoice> {
    let mut out: Vec<ModelChoice> = Vec::new();
    for l in text.lines() {
        let Some((id, label)) = l.split_once('\t') else {
            continue;
        };
        let (id, label) = (id.trim(), label.trim());
        if id.is_empty() {
            continue;
        }
        let (base, effort) = agy_split(id);
        let name = match (effort, label.rfind(" (")) {
            _ if label.is_empty() => base,
            (Some(_), Some(i)) if label.ends_with(')') => &label[..i],
            _ => label,
        };
        match out.iter_mut().find(|m| m.id == base) {
            Some(m) => m.efforts.extend(effort.map(String::from)),
            None => out.push(ModelChoice {
                id: base.into(),
                label: name.into(),
                efforts: effort.map(String::from).into_iter().collect(),
            }),
        }
    }
    // 수준은 낮은 것부터.
    let rank = |e: &String| ieum_local::ai::AGY_EFFORTS.iter().position(|x| x == e);
    for m in &mut out {
        m.efforts.sort_by_key(rank);
        m.efforts.dedup();
    }
    out
}

fn codex_models() -> Result<ModelList, String> {
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
/// 기본 모델의 수준: claude CLI는 모델과 무관, Anthropic API는 기본 모델(Haiku 5.5)의 수준, codex는 카탈로그 기본 모델의 것.
/// OpenAI는 기본 모델이 없고, Gemini 기본 모델·Antigravity("기본"은 수준이 id에 묶인다)는 고르지 않는다.
pub fn list(provider: &str) -> Result<ModelList, String> {
    let (models, default_efforts) = match provider {
        "api:anthropic" | "cli:claude" => (priced("claude-"), owned(&CLAUDE_EFFORTS)),
        "api:openai" => (priced("gpt-"), Vec::new()),
        "api:gemini" => (priced("gemini-"), Vec::new()),
        "cli:codex" => {
            let l = codex_models()?;
            (l.models, l.default_efforts)
        }
        "cli:agy" => (agy_models()?, Vec::new()),
        _ => return Err(format!("모르는 공급원이에요: {provider}")),
    };
    if models.is_empty() {
        return Err("모델 목록이 비어 있어요".into());
    }
    Ok(ModelList {
        models,
        default_efforts,
    })
}

/// 성공한 목록만 앱이 켜져 있는 동안 기억한다(CLI 목록은 띄우는 데 몇 초 걸린다).
static CACHE: Mutex<Option<HashMap<String, ModelList>>> = Mutex::new(None);

#[tauri::command]
pub async fn list_models(provider: String) -> Result<ModelList, String> {
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
