//! API 키 공급원(Anthropic·OpenAI·Gemini). SSE 스트리밍, 구조화 출력은 각 API 기능으로.
//! 키는 헤더로만 보내고(URL 쿼리 금지 — reqwest 에러가 URL을 담는다) 에러·`Debug`에 드러내지 않는다.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderValue;
use serde_json::{Map, Value, json};

use super::keys::{ApiKey, KeyStore, Keychain};
use super::{AiAnswer, AiError, AiProvider, AiRequest, AiUsage, ChunkFn, SYSTEM_INSTRUCTION};

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

    pub(super) fn official_url(self) -> &'static str {
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
                    "system": SYSTEM_INSTRUCTION,
                    "messages": [{"role": "user", "content": req.prompt}],
                });
                // 강제 tool_choice는 Opus·Sonnet 5.5 등에서 400 — output_config로 받는다(답은 text 블록).
                // thinking·temperature는 모델마다 400이 나므로 보내지 않는다.
                if let Some(s) = &req.schema {
                    body["output_config"] =
                        json!({"format": {"type": "json_schema", "schema": strict_schema(s)}});
                }
                (format!("{base}/v1/messages"), body)
            }
            ApiKind::OpenAi => {
                let mut body = json!({
                    "model": model,
                    "stream": true,
                    // 마지막 조각(choices 빈 목록)에 usage가 온다.
                    "stream_options": {"include_usage": true},
                    "messages": [
                        {"role": "system", "content": SYSTEM_INSTRUCTION},
                        {"role": "user", "content": req.prompt},
                    ],
                });
                if let Some(s) = &req.schema {
                    body["response_format"] = json!({
                        "type": "json_schema",
                        "json_schema": {"name": OUTPUT_NAME, "strict": true, "schema": strict_schema(s)},
                    });
                }
                (format!("{base}/v1/chat/completions"), body)
            }
            ApiKind::Gemini => {
                let mut body = json!({
                    "systemInstruction": {"parts": [{"text": SYSTEM_INSTRUCTION}]},
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

/// OpenAI strict·Anthropic output_config는 additionalProperties: false와 전부 required를 요구한다
/// (required는 `build_output_schema`가 이미 전부, 속성은 원시 타입뿐).
fn strict_schema(s: &Value) -> Value {
    let mut s = s.clone();
    s["additionalProperties"] = Value::Bool(false);
    s
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
    ) -> Result<AiAnswer, AiError> {
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
        // scanned: 줄바꿈이 없음을 이미 확인한 길이 — 다시 훑지 않아 긴 줄도 선형 시간.
        let (mut buf, mut total, mut scanned) = (Vec::new(), 0usize, 0usize);
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
            let mut start = 0;
            while let Some(i) = buf[scanned..].iter().position(|&b| b == b'\n') {
                let end = scanned + i;
                st.line(
                    String::from_utf8_lossy(&buf[start..end]).trim_end(),
                    key,
                    chunk,
                )?;
                start = end + 1;
                scanned = start;
            }
            buf.drain(..start);
            scanned = buf.len();
        }
        st.line(String::from_utf8_lossy(&buf).trim_end(), key, chunk)?;
        st.finish(model)
    }
}

#[async_trait]
impl AiProvider for ApiProvider {
    async fn ask(&self, req: AiRequest, chunk: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
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
            .get(req.key_name.as_deref().unwrap_or(self.kind.account()))
            .map_err(|e| AiError(format!("키체인에서 {name} API 키를 읽지 못했어요: {e}")))?
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
    /// 끝 표시(message_stop·[DONE]·finishReason)를 봤는지 — 없으면 중간에 끊긴 것.
    done: bool,
    /// 지금까지 본 사용량 칸. 뒤에 온 값이 덮는다(Anthropic message_start → message_delta,
    /// Gemini는 조각마다 누적값, OpenAI는 마지막 조각 하나).
    usage: Map<String, Value>,
}

impl Stream {
    fn new(kind: ApiKind) -> Self {
        Stream {
            kind,
            text: String::new(),
            done: false,
            usage: Map::new(),
        }
    }

    fn take_usage(&mut self, u: &Value) {
        for (k, v) in u.as_object().into_iter().flatten() {
            if !v.is_null() {
                self.usage.insert(k.clone(), v.clone());
            }
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
                Some("content_block_delta") if v["delta"]["type"] == "text_delta" => {
                    self.push(v["delta"]["text"].as_str().unwrap_or(""), chunk)
                }
                Some("message_start") => self.take_usage(&v["message"]["usage"]),
                Some("message_delta") => {
                    self.take_usage(&v["usage"]);
                    match v["delta"]["stop_reason"].as_str() {
                        Some("max_tokens" | "model_context_window_exceeded") => {
                            return Err(self.cut());
                        }
                        Some("refusal") => return Err(self.refused()),
                        _ => {}
                    }
                }
                Some("message_stop") => self.done = true,
                _ => {}
            },
            ApiKind::OpenAi => {
                self.take_usage(&v["usage"]);
                for c in v["choices"].as_array().into_iter().flatten() {
                    if c["delta"]["refusal"].is_string() {
                        return Err(self.refused());
                    }
                    self.push(c["delta"]["content"].as_str().unwrap_or(""), chunk);
                    match c["finish_reason"].as_str() {
                        Some("length") => return Err(self.cut()),
                        Some("content_filter") => return Err(self.refused()),
                        _ => {}
                    }
                }
            }
            ApiKind::Gemini => {
                if v["promptFeedback"]["blockReason"].is_string() {
                    return Err(self.refused());
                }
                self.take_usage(&v["usageMetadata"]);
                for c in v["candidates"].as_array().into_iter().flatten() {
                    for p in c["content"]["parts"].as_array().into_iter().flatten() {
                        self.push(p["text"].as_str().unwrap_or(""), chunk);
                    }
                    match c["finishReason"].as_str() {
                        Some("MAX_TOKENS") => return Err(self.cut()),
                        // STOP 말고는 안전·인용·차단 계열(SAFETY·RECITATION·BLOCKLIST …) — 거절로 본다.
                        Some("STOP") => self.done = true,
                        Some(_) => return Err(self.refused()),
                        None => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn cut(&self) -> AiError {
        AiError(format!(
            "{}: 답이 잘렸어요(최대 길이에 닿음).",
            self.kind.name()
        ))
    }

    fn refused(&self) -> AiError {
        AiError(format!("{}: AI가 요청을 거절했어요.", self.kind.name()))
    }

    fn finish(self, model: &str) -> Result<AiAnswer, AiError> {
        if !self.done {
            return Err(AiError(format!(
                "{} 응답이 중간에 끊겼어요.",
                self.kind.name()
            )));
        }
        let usage = (!self.usage.is_empty()).then(|| self.parse_usage(model));
        Ok(AiAnswer {
            text: self.text,
            usage,
        })
    }

    /// 공급원별 사용량 칸 → `AiUsage`. 입력은 캐시 읽기를 뺀 값으로 맞춘다.
    fn parse_usage(&self, model: &str) -> AiUsage {
        let u = &self.usage;
        let n = |k: &str| u.get(k).and_then(Value::as_u64);
        let source = self.kind.account();
        let (prompt, cached, written, output) = match self.kind {
            ApiKind::Anthropic => {
                return AiUsage::from_anthropic(source, Some(model), &Value::Object(u.clone()));
            }
            // prompt_tokens는 캐시 읽기·쓰기를 모두 포함한다(GPT-5.6+ 캐시 쓰기는 1.25배 단가).
            ApiKind::OpenAi => {
                let d = u.get("prompt_tokens_details");
                (
                    n("prompt_tokens"),
                    d.and_then(|d| d["cached_tokens"].as_u64()),
                    d.and_then(|d| d["cache_write_tokens"].as_u64()),
                    n("completion_tokens").unwrap_or(0),
                )
            }
            // 생각 토큰은 출력 요금으로 매긴다.
            ApiKind::Gemini => (
                n("promptTokenCount"),
                n("cachedContentTokenCount"),
                None,
                n("candidatesTokenCount").unwrap_or(0) + n("thoughtsTokenCount").unwrap_or(0),
            ),
        };
        AiUsage {
            source: source.into(),
            model: Some(model.into()),
            input_tokens: prompt
                .unwrap_or(0)
                .saturating_sub(cached.unwrap_or(0) + written.unwrap_or(0)),
            output_tokens: output,
            cache_read_tokens: cached,
            cache_write_tokens: written,
            credential: None,
        }
    }
}
