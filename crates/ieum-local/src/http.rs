//! `http.request` — BE `HttpNodeExecutor`를 옮겼다. 내부망 차단·멱등성 키는 없다(사용자 결정).
//! 실패 문구·`code`에 요청 헤더 값·요청 본문·연결 주소를 싣지 않는다.

use std::error::Error as _;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use ieum_engine::{NodeCtx, NodeError, NodeRunner};
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, COOKIE, HeaderMap, HeaderName, HeaderValue, LOCATION,
    PROXY_AUTHORIZATION,
};
use reqwest::{Client, Method, Response, Url};
use serde_json::{Map, Value, json};

/// BE `MAX_RESPONSE_BYTES` — 오류 응답 본문에도 같다.
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_REDIRECTS: usize = 3;
/// 실패 문구에 싣는 응답 본문(UTF-16 단위). 실행 기록에 1MB가 그대로 남지 않게(BE와 다름, GAPS.md).
const MAX_ERROR_BODY_UNITS: usize = 2000;

/// reqwest가 다시 내보내지 않는 `url::ParseError`.
type ParseError = <Url as FromStr>::Err;

fn fail_code(code: impl Into<String>, message: impl Into<String>) -> NodeError {
    NodeError {
        message: message.into(),
        code: Some(code.into()),
    }
}

fn unreachable(message: impl Into<String>) -> NodeError {
    fail_code("HTTP_UNREACHABLE", message)
}

pub struct HttpRunner {
    client: Client,
}

impl HttpRunner {
    pub fn new() -> Self {
        // 리다이렉트는 직접 따라간다(출처가 바뀌면 자격 헤더를 떼야 한다).
        // `.timeout()`은 요청 전체 마감이라 쓰지 않는다 — BE는 연결 10초·읽기 30초.
        // gzip 등 reqwest feature를 켜지 않아 압축 응답을 풀지 않는다(작은 응답이 크게 풀리지 않게).
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP 클라이언트 생성");
        HttpRunner { client }
    }
}

impl Default for HttpRunner {
    fn default() -> Self {
        Self::new()
    }
}

struct Request {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
}

fn request(cfg: &Map<String, Value>) -> Result<Request, NodeError> {
    let credential = match cfg.get("webhookCredentialId") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(_) => true,
    };
    if credential {
        return Err(fail_code(
            "FIELD_INVALID webhookCredentialId",
            "저장된 웹훅 주소는 데스크톱에서 쓸 수 없어요. URL 칸에 주소를 직접 넣어 주세요.",
        ));
    }
    let method = match cfg.get("method") {
        None | Some(Value::Null) => "GET".to_string(),
        Some(Value::String(s)) => s.to_uppercase(),
        Some(v) => v.to_string().to_uppercase(),
    };
    let method = match method.as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "DELETE" => Method::DELETE,
        _ => {
            return Err(fail_code(
                "FIELD_INVALID method",
                format!("지원하지 않는 HTTP 메서드: {method}"),
            ));
        }
    };
    let url = match cfg.get("url") {
        Some(Value::String(s)) if !s.trim().is_empty() => s,
        _ => return Err(fail_code("FIELD_EMPTY url", "'URL' 칸이 비어 있어요")),
    };
    let url = check_url(Url::parse(url), url).map_err(|m| fail_code("FIELD_INVALID url", m))?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    // 값은 비밀일 수 있어 이름만 싣는다.
    let bad_header = |name: &str| {
        fail_code(
            "FIELD_INVALID headers",
            format!("헤더 형식이 올바르지 않아요: {name}"),
        )
    };
    match cfg.get("headers") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            for (k, v) in m {
                let v = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| bad_header(k))?;
                let value = HeaderValue::from_str(&v).map_err(|_| bad_header(k))?;
                headers.insert(name, value);
            }
        }
        Some(_) => {
            return Err(fail_code(
                "FIELD_INVALID headers",
                "헤더는 이름과 값의 묶음이어야 해요.",
            ));
        }
    }
    // 문자열도 JSON 문자열(`"abc"`)로 나간다 — BE와 같은 본문. 빈 문자열도 `""`로 나간다.
    let body = match cfg.get("body") {
        None | Some(Value::Null) => None,
        Some(v) => Some(serde_json::to_vec(v).expect("JSON 값 직렬화")),
    };
    Ok(Request {
        method,
        url,
        headers,
        body,
    })
}

/// BE `validateUrl`에서 내부망 검사를 뺀 것. `shown`은 실패 문구에 싣는 원문.
fn check_url(parsed: Result<Url, ParseError>, shown: &str) -> Result<Url, String> {
    let url = match parsed {
        Ok(u) => u,
        Err(ParseError::EmptyHost) => return Err(format!("URL에 호스트가 없습니다: {shown}")),
        Err(_) => return Err(format!("유효하지 않은 URL: {shown}")),
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("http·https URL만 허용됩니다: {shown}"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(format!("URL에 호스트가 없습니다: {shown}"));
    }
    Ok(url)
}

/// scheme·host·port(기본 80/443). `url`이 scheme·host를 소문자로 맞춰 둔다.
fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// reqwest 오류 문구에는 주소가 들어 있어 쓰지 않는다.
fn send_error(e: reqwest::Error) -> NodeError {
    let reason = if e.is_timeout() {
        "시간 초과"
    } else if is_dns(&e) {
        "주소를 찾지 못함"
    } else {
        "연결 실패"
    };
    unreachable(format!("요청을 보내지 못했어요: {reason}"))
}

/// hyper-util 연결 오류가 이름 풀이 실패를 `dns error`로 알린다(따로 꺼낼 타입이 없다).
fn is_dns(e: &reqwest::Error) -> bool {
    let mut src = e.source();
    while let Some(s) = src {
        if s.to_string().contains("dns error") {
            return true;
        }
        src = s.source();
    }
    false
}

/// Content-Length를 믿지 않고 받은 만큼 세어 상한에서 끊는다(나머지는 버린다, 실패 아님).
async fn read_capped(mut resp: Response) -> Result<Vec<u8>, NodeError> {
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(send_error)? {
        let room = MAX_RESPONSE_BYTES - buf.len();
        buf.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if buf.len() == MAX_RESPONSE_BYTES {
            break;
        }
    }
    Ok(buf)
}

/// 앞 2,000자(UTF-16 단위)까지, 넘으면 줄임표. 대리쌍은 쪼개지 않는다.
fn clip(text: &str) -> String {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        units += c.len_utf16();
        if units > MAX_ERROR_BODY_UNITS {
            return format!("{}…", &text[..i]);
        }
    }
    text.to_string()
}

/// BE `parseBody` — 비거나 공백뿐이면 null, JSON 객체면 객체, 그 밖(배열 포함)은 문자열.
fn parse_body(text: String) -> Value {
    if text.trim().is_empty() {
        return Value::Null;
    }
    match serde_json::from_str::<Map<String, Value>>(&text) {
        Ok(m) => Value::Object(m),
        Err(_) => Value::String(text),
    }
}

impl HttpRunner {
    async fn execute(&self, req: Request) -> Result<Map<String, Value>, NodeError> {
        let Request {
            mut method,
            mut url,
            mut headers,
            mut body,
        } = req;
        let mut hops = 0;
        loop {
            let mut rb = self
                .client
                .request(method.clone(), url.clone())
                .headers(headers.clone());
            if let Some(b) = &body {
                rb = rb.body(b.clone());
            }
            let resp = rb.send().await.map_err(send_error)?;
            let status = resp.status().as_u16();
            if let (301 | 302 | 303 | 307 | 308, Some(loc)) = (status, resp.headers().get(LOCATION))
            {
                let shown = String::from_utf8_lossy(loc.as_bytes()).into_owned();
                if hops == MAX_REDIRECTS {
                    return Err(unreachable(format!(
                        "리다이렉트가 {MAX_REDIRECTS}회를 넘었습니다 — 마지막 Location: {shown}"
                    )));
                }
                let next = loc
                    .to_str()
                    .ok()
                    .and_then(|s| url.join(s).ok())
                    .ok_or_else(|| {
                        unreachable(format!(
                            "리다이렉트 Location 형식이 올바르지 않습니다: {shown}"
                        ))
                    })?;
                let shown = next.to_string();
                let next = check_url(Ok(next), &shown).map_err(unreachable)?;
                // 한 번 뗀 자격 헤더는 원래 출처로 돌아와도 되살리지 않는다.
                if !same_origin(&url, &next) {
                    for h in [AUTHORIZATION, PROXY_AUTHORIZATION, COOKIE] {
                        headers.remove(h);
                    }
                }
                if status <= 303 {
                    method = Method::GET;
                    body = None;
                }
                url = next;
                hops += 1;
                continue;
            }
            let text = String::from_utf8_lossy(&read_capped(resp).await?).into_owned();
            if status >= 400 {
                return Err(fail_code(
                    format!("HTTP_STATUS {status}"),
                    format!("HTTP {status}: {}", clip(&text)),
                ));
            }
            let mut out = Map::new();
            out.insert("statusCode".into(), json!(status));
            out.insert("body".into(), parse_body(text));
            return Ok(out);
        }
    }
}

#[async_trait]
impl NodeRunner for HttpRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let req = request(&ctx.config)?;
        // 취소는 홉·본문 읽기 어디서든 끊는다. 시간 제한은 클라이언트가 매 요청에 다시 건다.
        tokio::select! {
            _ = ctx.cancel.cancelled() => Err(NodeError {
                message: "취소됐어요.".into(),
                code: None,
            }),
            r = self.execute(req) => r,
        }
    }
}
