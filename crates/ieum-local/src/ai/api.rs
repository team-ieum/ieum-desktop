//! API 키 공급원(Anthropic·OpenAI·Gemini). SSE 스트리밍, 구조화 출력은 각 API 기능으로.
//! 키는 헤더로만 보내고(URL 쿼리 금지 — reqwest 에러가 URL을 담는다) 에러·`Debug`에 드러내지 않는다.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderValue;
use serde_json::{Map, Value, json};

use super::keys::{ApiKey, KeyStore, Keychain};
use super::{AiError, AiProvider, AiRequest, ChunkFn};

pub const API_TIMEOUT: Duration = Duration::from_secs(300);
/// 응답 본문 전체 상한.
const MAX_BODY: usize = 10 * 1024 * 1024;
/// 오류 응답에서 읽는 상한.
const MAX_ERROR_BODY: usize = 64 * 1024;
/// 에러 메시지에 싣는 공급자 오류 설명(글자).
const DETAIL_CHARS: usize = 300;
const ANTHROPIC_MAX_TOKENS: u32 = 8192;
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// 구조화 출력용 도구·스키마 이름.
const OUTPUT_NAME: &str = "output";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKind {
    Anthropic,
    OpenAi,
    Gemini,
}

impl ApiKind {
    /// `local.ai` provider 값.
    pub fn id(self) -> &'static str {
        match self {
            ApiKind::Anthropic => "api:anthropic",
            ApiKind::OpenAi => "api:openai",
            ApiKind::Gemini => "api:gemini",
        }
    }

    /// 키체인 계정.
    pub fn account(self) -> &'static str {
        match self {
            ApiKind::Anthropic => "anthropic",
            ApiKind::OpenAi => "openai",
            ApiKind::Gemini => "gemini",
        }
    }

    fn name(self) -> &'static str {
        match self {
            ApiKind::Anthropic => "Anthropic",
            ApiKind::OpenAi => "OpenAI",
            ApiKind::Gemini => "Gemini",
        }
    }

    fn official_url(self) -> &'static str {
        match self {
            ApiKind::Anthropic => "https://api.anthropic.com",
            ApiKind::OpenAi => "https://api.openai.com",
            ApiKind::Gemini => "https://generativelanguage.googleapis.com",
        }
    }

    /// OpenAI는 기본값 없음 — 모델을 골라야 한다.
    fn default_model(self) -> Option<&'static str> {
        match self {
            ApiKind::Anthropic => Some("claude-haiku-5-5"),
            ApiKind::OpenAi => None,
            ApiKind::Gemini => Some("gemini-3.5-flash"),
        }
    }

    fn auth_header(self) -> &'static str {
        match self {
            ApiKind::Anthropic => "x-api-key",
            ApiKind::OpenAi => "authorization",
            ApiKind::Gemini => "x-goog-api-key",
        }
    }

    /// 요청 URL(키 없음)과 본문.
    fn request(self, base: &str, model: &str, req: &AiRequest) -> (String, Value) {
        let base = base.trim_end_matches('/');
        match self {
            ApiKind::Anthropic => {
                let mut body = json!({
                    "model": model,
                    "max_tokens": ANTHROPIC_MAX_TOKENS,
                    "stream": true,
                    "messages": [{"role": "user", "content": req.prompt}],
                });
                if let Some(s) = &req.schema {
                    body["tools"] = json!([{
                        "name": OUTPUT_NAME,
                        "description": "출력 칸에 맞춰 답한다.",
                        "input_schema": s,
                    }]);
                    body["tool_choice"] = json!({"type": "tool", "name": OUTPUT_NAME});
                }
                (format!("{base}/v1/messages"), body)
            }
            ApiKind::OpenAi => {
                let mut body = json!({
                    "model": model,
                    "stream": true,
                    "messages": [{"role": "user", "content": req.prompt}],
                });
                if let Some(s) = &req.schema {
                    // strict는 additionalProperties: false와 전부 required를 요구한다(required는 이미 전부).
                    let mut s = s.clone();
                    s["additionalProperties"] = Value::Bool(false);
                    body["response_format"] = json!({
                        "type": "json_schema",
                        "json_schema": {"name": OUTPUT_NAME, "strict": true, "schema": s},
                    });
                }
                (format!("{base}/v1/chat/completions"), body)
            }
            ApiKind::Gemini => {
                let mut body = json!({
                    "contents": [{"role": "user", "parts": [{"text": req.prompt}]}],
                });
                if let Some(s) = &req.schema {
                    body["generationConfig"] = json!({
                        "responseMimeType": "application/json",
                        "responseSchema": gemini_schema(s),
                    });
                }
                (
                    format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse"),
                    body,
                )
            }
        }
    }
}

/// Gemini `responseSchema`의 type은 OpenAPI 대문자 enum(`OBJECT`·`STRING` …).
fn gemini_schema(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let v = match v.as_str() {
                        Some(t) if k == "type" => json!(t.to_uppercase()),
                        _ => gemini_schema(v),
                    };
                    (k.clone(), v)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(gemini_schema).collect()),
        _ => v.clone(),
    }
}

/// 모델 이름은 URL 경로(Gemini)에 들어간다 — 경로·쿼리를 깨는 문자를 막는다.
fn valid_model(m: &str) -> bool {
    !m.contains("..")
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':'))
}

pub struct ApiProvider {
    kind: ApiKind,
    base_url: String,
    keys: Arc<dyn KeyStore>,
    client: reqwest::Client,
}

impl fmt::Debug for ApiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiProvider")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl ApiProvider {
    /// 운영: 공식 URL + macOS 키체인.
    pub fn new(kind: ApiKind) -> Self {
        Self::with(kind, kind.official_url(), Arc::new(Keychain))
    }

    /// 기본 URL·키 저장소 주입(테스트용).
    pub fn with(kind: ApiKind, base_url: &str, keys: Arc<dyn KeyStore>) -> Self {
        // 리다이렉트를 따라가면 키 헤더가 다른 곳으로 갈 수 있다.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("HTTP 클라이언트 생성");
        ApiProvider {
            kind,
            base_url: base_url.to_string(),
            keys,
            client,
        }
    }

    async fn call(
        &self,
        req: &AiRequest,
        model: &str,
        key: &ApiKey,
        chunk: &ChunkFn<'_>,
    ) -> Result<String, AiError> {
        let name = self.kind.name();
        let (url, body) = self.kind.request(&self.base_url, model, req);
        let value = match self.kind {
            ApiKind::OpenAi => format!("Bearer {}", key.expose()),
            _ => key.expose().to_string(),
        };
        // 에러 메시지에 값을 싣지 않는다.
        let mut auth = HeaderValue::from_str(&value)
            .map_err(|_| AiError("API 키에 쓸 수 없는 문자가 있어요.".into()))?;
        auth.set_sensitive(true);
        let mut rb = self
            .client
            .post(url)
            .header(self.kind.auth_header(), auth)
            .json(&body);
        if self.kind == ApiKind::Anthropic {
            rb = rb.header("anthropic-version", ANTHROPIC_VERSION);
        }
        let mut resp = rb
            .send()
            .await
            .map_err(|e| AiError(format!("{name} API에 연결하지 못했어요: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let mut b = Vec::new();
            while let Ok(Some(c)) = resp.chunk().await {
                b.extend_from_slice(&c);
                if b.len() >= MAX_ERROR_BODY {
                    break;
                }
            }
            let v: Value = serde_json::from_slice(&b).unwrap_or_default();
            return Err(AiError(http_error(name, status.as_u16(), &v, key)));
        }

        let mut st = Stream::new(self.kind);
        let (mut buf, mut total) = (Vec::new(), 0usize);
        while let Some(c) = resp
            .chunk()
            .await
            .map_err(|e| AiError(format!("{name} 응답을 읽다가 끊겼어요: {e}")))?
        {
            total += c.len();
            if total > MAX_BODY {
                return Err(AiError(format!(
                    "{name} 응답이 {}MB를 넘어 멈췄어요.",
                    MAX_BODY / 1024 / 1024
                )));
            }
            buf.extend_from_slice(&c);
            while let Some(i) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=i).collect();
                st.line(String::from_utf8_lossy(&line).trim_end(), key, chunk)?;
            }
        }
        st.line(String::from_utf8_lossy(&buf).trim_end(), key, chunk)?;
        st.finish()
    }
}

#[async_trait]
impl AiProvider for ApiProvider {
    async fn ask(&self, req: AiRequest, chunk: &ChunkFn<'_>) -> Result<String, AiError> {
        let name = self.kind.name();
        let model = req
            .model
            .as_deref()
            .or(self.kind.default_model())
            .ok_or_else(|| AiError("모델을 골라 주세요.".into()))?;
        if !valid_model(model) {
            return Err(AiError(format!("모델 이름이 올바르지 않아요: {model:?}")));
        }
        let key = self
            .keys
            .get(self.kind.account())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .map(ApiKey::new)
            .ok_or_else(|| {
                AiError(format!(
                    "{name} API 키가 없어요. 설정에서 키를 넣어 주세요."
                ))
            })?;
        // 취소되면 이 future가 버려지며 요청 연결도 끊긴다.
        match tokio::time::timeout(API_TIMEOUT, self.call(&req, model, &key, chunk)).await {
            // 마지막 방어선: 어떤 경로로 왔든 키 문자열은 가린다.
            Ok(r) => r.map_err(|e| AiError(key.mask(&e.0))),
            Err(_) => Err(AiError(format!(
                "{name} API가 {}초 안에 답하지 않아 멈췄어요(시간 초과).",
                API_TIMEOUT.as_secs()
            ))),
        }
    }
}

/// 공급자 오류 객체의 type·message만. 키는 자르기 전에 가린다(잘린 키 앞부분이 새지 않게).
fn error_detail(v: &Value, key: &ApiKey) -> String {
    let e = &v["error"];
    let parts: Vec<&str> = [
        e["type"].as_str().or(e["status"].as_str()),
        e["message"].as_str(),
    ]
    .into_iter()
    .flatten()
    .collect();
    key.mask(&parts.join(": "))
        .chars()
        .take(DETAIL_CHARS)
        .collect()
}

fn http_error(name: &str, status: u16, v: &Value, key: &ApiKey) -> String {
    let head = match status {
        401 | 403 => "API 키를 확인해 주세요.",
        429 => "요청이 너무 많아요. 잠시 뒤 다시 시도해 주세요.",
        500..=599 => "AI 서비스에 문제가 있어요.",
        _ => "AI 요청이 실패했어요.",
    };
    let detail = error_detail(v, key);
    if detail.is_empty() {
        format!("{head} ({name} HTTP {status})")
    } else {
        format!("{head} ({name} HTTP {status}: {detail})")
    }
}

/// SSE `data:` 줄을 읽어 조각·최종 텍스트를 모은다.
struct Stream {
    kind: ApiKind,
    text: String,
    /// Anthropic 강제 도구의 입력 JSON.
    tool_json: Option<String>,
    /// 끝 표시(message_stop·[DONE]·finishReason)를 봤는지 — 없으면 중간에 끊긴 것.
    done: bool,
}

impl Stream {
    fn new(kind: ApiKind) -> Self {
        Stream {
            kind,
            text: String::new(),
            tool_json: None,
            done: false,
        }
    }

    fn push(&mut self, t: &str, chunk: &ChunkFn<'_>) {
        if !t.is_empty() {
            chunk(t);
            self.text.push_str(t);
        }
    }

    fn line(&mut self, line: &str, key: &ApiKey, chunk: &ChunkFn<'_>) -> Result<(), AiError> {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return Ok(());
        };
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        if !v["error"].is_null() {
            return Err(AiError(format!(
                "{} API 오류: {}",
                self.kind.name(),
                error_detail(&v, key)
            )));
        }
        match self.kind {
            ApiKind::Anthropic => match v["type"].as_str() {
                Some("content_block_delta") => {
                    let d = &v["delta"];
                    match d["type"].as_str() {
                        Some("text_delta") => self.push(d["text"].as_str().unwrap_or(""), chunk),
                        Some("input_json_delta") => self
                            .tool_json
                            .get_or_insert_default()
                            .push_str(d["partial_json"].as_str().unwrap_or("")),
                        _ => {}
                    }
                }
                Some("message_stop") => self.done = true,
                _ => {}
            },
            ApiKind::OpenAi => {
                for c in v["choices"].as_array().into_iter().flatten() {
                    self.push(c["delta"]["content"].as_str().unwrap_or(""), chunk);
                }
            }
            ApiKind::Gemini => {
                for c in v["candidates"].as_array().into_iter().flatten() {
                    for p in c["content"]["parts"].as_array().into_iter().flatten() {
                        self.push(p["text"].as_str().unwrap_or(""), chunk);
                    }
                    if c["finishReason"].is_string() {
                        self.done = true;
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<String, AiError> {
        if !self.done {
            return Err(AiError(format!(
                "{} 응답이 중간에 끊겼어요.",
                self.kind.name()
            )));
        }
        Ok(self.tool_json.unwrap_or(self.text))
    }
}
