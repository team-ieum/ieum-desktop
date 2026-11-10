//! `local.ai` — AI 노드. 출력 칸 규칙·검증은 `ieum-agent/core/output_fields.py`를 옮겼다(메시지 그대로).

mod cli;
mod content;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::{ExecEvent, NodeCtx, NodeError, NodeRunner};
use serde_json::{Map, Value, json};

pub use cli::{CliKind, CliProvider, DetectedCli, detect_clis, parse_stream};
pub use content::{MAX_CHARS, MAX_PDF_BYTES, Skip, file_text, file_text_bounded, read_file_text};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct AiError(pub String);

#[derive(Debug, Clone)]
pub struct AiRequest {
    pub prompt: String,
    pub model: Option<String>,
    /// 출력 칸에서 만든 JSON Schema. 없으면 텍스트로 받는다.
    pub schema: Option<Value>,
}

/// 텍스트 조각 콜백. 별칭으로 둔다 — `async_trait`이 `&str`의 생략 수명을 고정 수명으로 바꿔 버린다.
pub type ChunkFn<'a> = dyn Fn(&str) + Send + Sync + 'a;

#[async_trait]
pub trait AiProvider: Send + Sync {
    /// 텍스트 조각은 `chunk`로 흘리고 최종 텍스트를 돌려준다.
    async fn ask(&self, req: AiRequest, chunk: &ChunkFn<'_>) -> Result<String, AiError>;
}

/// 모델 응답 모양이 틀렸을 때 다시 묻는 횟수를 포함한 총 시도 수(스펙 §4.1: 1회 재시도).
const ATTEMPTS: usize = 2;

/// `local.ai` 러너. provider 문자열(`cli:claude` 등) → 공급원.
pub struct AiRunner {
    providers: HashMap<String, Arc<dyn AiProvider>>,
}

impl AiRunner {
    /// 공급원 없이 — 테스트·조립용.
    pub fn empty() -> Self {
        AiRunner {
            providers: HashMap::new(),
        }
    }

    /// 구독 CLI 공급원을 단다. 지금은 claude만 — `cli:codex`는 보류(`CliKind::Codex` 주석).
    pub fn new() -> Self {
        let k = CliKind::Claude;
        Self::empty().with_provider(k.id(), Arc::new(CliProvider::new(k)))
    }

    pub fn with_provider(mut self, id: &str, provider: Arc<dyn AiProvider>) -> Self {
        self.providers.insert(id.to_string(), provider);
        self
    }
}

impl Default for AiRunner {
    fn default() -> Self {
        Self::new()
    }
}

fn fail(message: impl Into<String>) -> NodeError {
    NodeError {
        message: message.into(),
    }
}

fn opt_str<'a>(cfg: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    cfg.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

/// 프롬프트 = 지시 + (파일이 있으면) 파일 이름·내용. 내용을 못 읽으면 `notice`로 한 줄 알린다.
async fn build_prompt(prompt: &str, file: Option<&str>, notice: &ChunkFn<'_>) -> String {
    let Some(file) = file else {
        return prompt.to_string();
    };
    let path = Path::new(file);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let text = file_text_bounded(path).await.unwrap_or_else(|skip| {
        notice(skip.notice());
        String::new()
    });
    format!("{prompt}\n\n[파일 이름] {name}\n[파일 내용]\n{text}")
}

#[async_trait]
impl NodeRunner for AiRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let cfg = &ctx.config;
        let fields = validate_output_fields(cfg.get("outputFields")).map_err(fail)?;
        let provider_id = cfg.get("provider").and_then(Value::as_str).unwrap_or("");
        let provider = self
            .providers
            .get(provider_id)
            .ok_or_else(|| fail(format!("지원하지 않는 공급원이에요: {provider_id}")))?;
        let prompt = opt_str(cfg, "prompt").ok_or_else(|| fail("'prompt' 칸이 비었어요."))?;
        let model = opt_str(cfg, "model");
        let node_id = &ctx.node.id;
        let events = ctx.events;
        let chunk = |text: &str| {
            let _ = events.send(ExecEvent::NodeChunk {
                node_id: node_id.clone(),
                text: text.to_string(),
            });
        };

        let work = async {
            // 파일 읽기(PDF 추출)도 취소 select 안에서.
            let req = AiRequest {
                prompt: build_prompt(prompt, opt_str(cfg, "file"), &chunk).await,
                model: model.map(str::to_string),
                schema: fields.as_deref().map(build_output_schema),
            };
            let mut mismatch = String::new();
            for _ in 0..ATTEMPTS {
                let text = provider
                    .ask(req.clone(), &chunk)
                    .await
                    .map_err(|e| fail(e.0))?;
                let Some(fields) = &fields else {
                    let mut out = Map::new();
                    out.insert("output".into(), Value::String(text));
                    return Ok(out);
                };
                match parse_structured_output(&text, fields) {
                    Ok(out) => return Ok(out),
                    Err(e) => mismatch = e,
                }
            }
            Err(fail(format!(
                "AI 응답이 출력 칸 모양과 달라요({ATTEMPTS}번 시도): {mismatch}"
            )))
        };
        // 취소되면 work가 버려지며 CLI 자식 프로세스(그룹)가 kill된다.
        tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => Err(fail("취소됐어요.")),
            r = work => r,
        }
    }
}

// ---- 출력 칸(outputFields) — agent core/output_fields.py ----

const MAX_FIELDS: usize = 20;
const MAX_DESCRIPTION: usize = 500;
const ATTRS: [&str; 3] = ["key", "type", "description"];
const RESERVED_KEYS: [&str; 2] = ["success", "error"];
const SENSITIVE_PARTS: [&str; 6] = [
    "apikey",
    "api_key",
    "token",
    "secret",
    "password",
    "authorization",
];
const PREVIEW_CHARS: usize = 50;

#[derive(Debug, Clone)]
pub struct OutputField {
    pub key: String,
    /// `text`·`number`·`boolean`
    pub ftype: String,
    pub description: Option<String>,
}

fn schema_type(ftype: &str) -> Option<&'static str> {
    match ftype {
        "text" => Some("string"),
        "number" => Some("number"),
        "boolean" => Some("boolean"),
        _ => None,
    }
}

/// 파이썬 `{x!r}` 흉내 — 문자열은 작은따옴표, 없음은 None.
fn repr(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => format!("'{s}'"),
        None | Some(Value::Null) => "None".into(),
        Some(v) => v.to_string(),
    }
}

/// `[A-Za-z_][A-Za-z0-9_]{0,63}` 전체 일치.
fn key_ok(key: &str) -> bool {
    let mut cs = key.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && key.len() <= 64
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 정규화된 칸 목록. 없음·빈 목록이면 None — 스키마 없이 텍스트를 받는다.
/// 로컬 AI 노드엔 tools·agentType이 없어 그 두 검사는 옮기지 않았다.
pub fn validate_output_fields(value: Option<&Value>) -> Result<Option<Vec<OutputField>>, String> {
    let list = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) if a.is_empty() => return Ok(None),
        Some(Value::Array(a)) => a,
        Some(_) => return Err("출력 칸(outputFields)은 목록이어야 합니다.".into()),
    };
    if list.len() > MAX_FIELDS {
        return Err(format!(
            "출력 칸은 최대 {MAX_FIELDS}개입니다(받은 개수: {}).",
            list.len()
        ));
    }
    let mut fields: Vec<OutputField> = Vec::new();
    for (i, f) in list.iter().enumerate() {
        let i = i + 1;
        let Value::Object(f) = f else {
            return Err(format!("{i}번째 출력 칸은 객체여야 합니다."));
        };
        let mut unknown: Vec<&str> = f
            .keys()
            .map(String::as_str)
            .filter(|k| !ATTRS.contains(k))
            .collect();
        if !unknown.is_empty() {
            unknown.sort();
            let shown: Vec<String> = unknown.iter().map(|k| format!("'{k}'")).collect();
            return Err(format!(
                "{i}번째 출력 칸에 알 수 없는 속성이 있습니다: [{}]",
                shown.join(", ")
            ));
        }
        let key = match f.get("key") {
            Some(Value::String(k)) if key_ok(k) => k.clone(),
            k => {
                return Err(format!(
                    "출력 칸 이름 {}는 영문·숫자·밑줄만 쓰고 영문이나 밑줄로 시작해야 합니다(64자 이하).",
                    repr(k)
                ));
            }
        };
        if fields.iter().any(|x| x.key == key) {
            return Err(format!("출력 칸 이름 '{key}'가 중복됐습니다."));
        }
        if RESERVED_KEYS.contains(&key.as_str()) {
            return Err(format!("출력 칸 이름 '{key}'는 예약어라 쓸 수 없습니다."));
        }
        let lower = key.to_lowercase();
        if SENSITIVE_PARTS.iter().any(|p| lower.contains(p)) {
            return Err(format!(
                "출력 칸 이름 '{key}'는 민감정보 이름이라 저장 시 가려집니다. 다른 이름을 쓰세요."
            ));
        }
        let ftype = match f.get("type") {
            Some(Value::String(t)) if schema_type(t).is_some() => t.clone(),
            t => {
                return Err(format!(
                    "출력 칸 '{key}'의 타입 {}는 text·number·boolean 중 하나여야 합니다.",
                    repr(t)
                ));
            }
        };
        let description = match f.get("description") {
            None | Some(Value::Null) => None,
            Some(Value::String(d)) if d.chars().count() <= MAX_DESCRIPTION => {
                Some(d.clone()).filter(|d| !d.is_empty())
            }
            Some(_) => {
                return Err(format!(
                    "출력 칸 '{key}'의 설명은 {MAX_DESCRIPTION}자 이하 문자열이어야 합니다."
                ));
            }
        };
        fields.push(OutputField {
            key,
            ftype,
            description,
        });
    }
    Ok(Some(fields))
}

/// 출력 칸 → JSON Schema. additionalProperties는 넣지 않는다(공급원이 필요하면 붙인다).
pub fn build_output_schema(fields: &[OutputField]) -> Value {
    let mut props = Map::new();
    for f in fields {
        let mut p = Map::new();
        p.insert("type".into(), json!(schema_type(&f.ftype)));
        if let Some(d) = &f.description {
            p.insert("description".into(), json!(d));
        }
        props.insert(f.key.clone(), Value::Object(p));
    }
    let required: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
    json!({"type": "object", "properties": props, "required": required})
}

const TWO_63: f64 = 9_223_372_036_854_775_808.0;

fn type_ok(ftype: &str, v: &Value) -> bool {
    match ftype {
        "text" => v.is_string(),
        "boolean" => v.is_boolean(),
        // 정수는 int64 범위만. 실수는 serde_json이 NaN·Infinity를 읽지 않아 늘 유한하다.
        _ => v.is_i64() || v.is_f64(),
    }
}

// ponytail: 미리보기에 agent의 토큰 마스킹(OutputValidator)은 옮기지 않았다 — 로컬 실행이라 BE 저장이 없다.
fn preview(v: &Value) -> String {
    v.to_string().chars().take(PREVIEW_CHARS).collect()
}

/// 모델 원문 → 선언한 칸만 담은 객체. 코드펜스 제거 같은 관용 처리는 하지 않는다.
pub fn parse_structured_output(
    text: &str,
    fields: &[OutputField],
) -> Result<Map<String, Value>, String> {
    let data: Value = serde_json::from_str(text).map_err(|_| "AI 응답이 JSON이 아닙니다.")?;
    let Value::Object(data) = data else {
        return Err("AI 응답이 JSON 객체가 아닙니다.".into());
    };
    let mut out = Map::new();
    for f in fields {
        let key = &f.key;
        let value = match data.get(key) {
            None | Some(Value::Null) => return Err(format!("출력 필드 '{key}' 누락")),
            Some(v) => v,
        };
        if !type_ok(&f.ftype, value) {
            return Err(format!(
                "출력 필드 '{key}'는 {}여야 합니다(받은 값: {})",
                f.ftype,
                preview(value)
            ));
        }
        // 4.0 → 4 (BE 치환이 "4.0"이 되면 CONDITION equals "4"가 깨진다). int64 밖은 실수로 둔다.
        let value = match value.as_f64() {
            Some(x) if value.is_f64() && x.fract() == 0.0 && (-TWO_63..TWO_63).contains(&x) => {
                json!(x as i64)
            }
            _ => value.clone(),
        };
        out.insert(key.clone(), value);
    }
    Ok(out)
}
