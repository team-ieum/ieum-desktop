//! `http.request` — BE `HttpNodeExecutor`를 옮겼다. 내부망 차단·멱등성 키는 없다(사용자 결정).
//! 실패 문구·`code`에 요청 헤더 값·요청 본문·연결 주소를 싣지 않는다.

use std::error::Error as _;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use ieum_engine::{NodeCtx, NodeError, NodeRunner, java_strip};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, LOCATION};
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

/// BE와 같은 연결 10초·읽기 30초.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// 요청 하나(리다이렉트·본문 읽기 포함) 전체 상한 — 조금씩 흘리는 서버가 실행 줄을 막지 않게(BE에는 없다, GAPS.md).
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);

/// 실행마다 TLS 루트·연결 풀을 새로 만들지 않게 프로세스에서 같이 쓴다.
static SHARED: LazyLock<Client> = LazyLock::new(|| client(CONNECT_TIMEOUT, READ_TIMEOUT));

fn client(connect: Duration, read: Duration) -> Client {
    // 리다이렉트는 직접 따라간다(출처가 바뀌면 자격 헤더를 떼야 한다).
    // `.timeout()`은 요청 전체 마감이라 쓰지 않는다 — 읽기 제한은 읽기마다 다시 건다.
    // gzip 등 reqwest feature를 켜지 않아 압축 응답을 풀지 않는다(작은 응답이 크게 풀리지 않게).
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect)
        .read_timeout(read)
        .build()
        .expect("HTTP 클라이언트 생성")
}

pub struct HttpRunner {
    client: Client,
    total: Duration,
}

impl HttpRunner {
    pub fn new() -> Self {
        HttpRunner {
            client: SHARED.clone(),
            total: TOTAL_TIMEOUT,
        }
    }

    pub fn with_timeouts(connect: Duration, read: Duration, total: Duration) -> Self {
        HttpRunner {
            client: client(connect, read),
            total,
        }
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
        Some(Value::String(s)) => !java_strip(s).is_empty(),
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
        Some(Value::String(s)) if !java_strip(s).is_empty() => s,
        _ => return Err(fail_code("FIELD_EMPTY url", "'URL' 칸이 비어 있어요")),
    };
    let url =
        check_url(url, None, "유효하지 않은 URL").map_err(|m| fail_code("FIELD_INVALID url", m))?;

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
                // BE는 `Map<String,String>`으로 읽어 글이 아닌 값에서 실패한다.
                let Value::String(v) = v else {
                    return Err(bad_header(k));
                };
                let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| bad_header(k))?;
                let value = HeaderValue::from_str(v).map_err(|_| bad_header(k))?;
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

/// BE `validateUrl`에서 내부망 검사를 뺀 것. `base`가 있으면 리다이렉트 Location으로 읽고,
/// `syntax`는 읽지 못했을 때의 문구. 실패 문구에는 가린 원문을 싣는다.
fn check_url(text: &str, base: Option<&Url>, syntax: &str) -> Result<Url, String> {
    let fail = |what: &str| Err(format!("{what}: {}", mask(text)));
    // `Url`은 이런 글자를 퍼센트 인코딩하거나 지워 조용히 따라간다 — BE `java.net.URI`는 거절.
    if uri_rejects(text) {
        return fail(syntax);
    }
    let url = match base.map_or_else(|| Url::parse(text), |b| b.join(text)) {
        Ok(u) => u,
        Err(ParseError::RelativeUrlWithoutBase) => return fail("http·https URL만 허용됩니다"),
        Err(ParseError::EmptyHost) => return fail("URL에 호스트가 없습니다"),
        Err(_) => return fail(syntax),
    };
    if !matches!(url.scheme(), "http" | "https") {
        return fail("http·https URL만 허용됩니다");
    }
    // `Url`은 `http:/x`·`http:///x`를 `http://x/`로 고쳐 읽는다 — URI에는 호스트가 없다.
    let no_authority = after_scheme(text).is_some_and(|rest| {
        rest.strip_prefix("//")
            .is_none_or(|a| a.is_empty() || a.starts_with(['/', '?', '#']))
    });
    if no_authority || url.host_str().is_none_or(str::is_empty) {
        return fail("URL에 호스트가 없습니다");
    }
    Ok(url)
}

/// 공백(유니코드 공백 포함)·제어문자·`"<>\^`{|}`·두 자리 16진수가 뒤따르지 않는 `%` (JDK 21로 확인).
fn uri_rejects(text: &str) -> bool {
    let b = text.as_bytes();
    text.char_indices().any(|(i, c)| {
        c.is_whitespace()
            || c.is_control()
            || "\"<>\\^`{|}".contains(c)
            || (c == '%'
                && !b
                    .get(i + 1..i + 3)
                    .is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)))
    })
}

/// `scheme:` 뒤의 글. scheme이 없으면(상대 주소) None.
fn after_scheme(text: &str) -> Option<&str> {
    let (scheme, rest) = text.split_once(':')?;
    let mut cs = scheme.chars();
    (cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        && cs.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)))
    .then_some(rest)
}

/// 실패 문구에 싣는 주소 — 쿼리·fragment·계정 정보는 토큰일 수 있어 가린다(BE는 원문, GAPS.md).
/// 읽지 못한 글에도 쓰므로 파싱 없이 글자로만 자른다. 빌더 fields도 같은 규칙으로 가린다.
pub(crate) fn mask(text: &str) -> String {
    let (head, cut) = match text.find(['?', '#']) {
        Some(i) => (&text[..i], format!("{}…", &text[i..=i])),
        None => (text, String::new()),
    };
    // scheme 없는 `//계정@호스트`도 계정 정보가 붙는다.
    let start = if head.starts_with("//") {
        Some(2)
    } else {
        head.find("://").map(|i| i + 3)
    };
    if let Some(s) = start {
        let end = head[s..].find('/').map_or(head.len(), |i| s + i);
        if let Some(at) = head[s..end].rfind('@') {
            return format!("{}…{}{cut}", &head[..s], &head[s + at..]);
        }
    }
    format!("{head}{cut}")
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

/// BE `parseBody` — 비거나 공백뿐이면(Java `isBlank`) null, JSON 객체면 객체, 그 밖(배열 포함)은 문자열.
fn parse_body(text: String) -> Value {
    if java_strip(&text).is_empty() {
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
                        "리다이렉트가 {MAX_REDIRECTS}회를 넘었습니다 — 마지막 Location: {}",
                        mask(&shown)
                    )));
                }
                // 보이는 ASCII가 아닌 바이트도 형식 오류다(그 글은 lossy로 싣는다).
                let syntax = "리다이렉트 Location 형식이 올바르지 않습니다";
                let next = match loc.to_str() {
                    Ok(s) => check_url(s, Some(&url), syntax),
                    Err(_) => Err(format!("{syntax}: {}", mask(&shown))),
                }
                .map_err(unreachable)?;
                // 다른 출처로는 사용자 머리말(키가 든 X-API-Key 등)을 보내지 않는다 — Content-Type·Accept만
                // 남긴다(BE는 자격 머리말 셋만 뗀다, GAPS.md). 한 번 뗀 것은 원래 출처로 돌아와도 되살리지 않는다.
                if !same_origin(&url, &next) {
                    let mut kept = HeaderMap::new();
                    for h in [CONTENT_TYPE, ACCEPT] {
                        if let Some(v) = headers.remove(&h) {
                            kept.insert(h, v);
                        }
                    }
                    headers = kept;
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
        // 취소·전체 상한은 홉·본문 읽기 어디서든 끊는다. 연결·읽기 제한은 클라이언트가 매 요청에 다시 건다.
        tokio::select! {
            _ = ctx.cancel.cancelled() => Err(NodeError {
                message: "취소됐어요.".into(),
                code: None,
            }),
            r = tokio::time::timeout(self.total, self.execute(req)) => {
                r.unwrap_or_else(|_| Err(unreachable("요청을 보내지 못했어요: 시간 초과")))
            }
        }
    }
}
