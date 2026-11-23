//! GitHub REST 호출 한 번. 오류 문구·`code`에 응답 본문·토큰·헤더 값을 싣지 않는다.

use std::time::Duration;

use ieum_engine::NodeError;
use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, Method, Url};
use serde_json::Value;

const BASE: &str = "https://api.github.com";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BYTES: usize = 32 * 1024 * 1024;

fn fail(code: impl Into<String>, message: impl Into<String>) -> NodeError {
    NodeError {
        message: message.into(),
        code: Some(code.into()),
    }
}

fn unreachable() -> NodeError {
    fail(
        "GITHUB_UNREACHABLE",
        "GitHub에 닿지 못했어요. 잠시 뒤 다시 실행해 주세요.",
    )
}

/// 호출 전에 입력을 거절할 때. 문구는 빌더 AI에게 그대로 가므로 사용자 값을 넣지 않는다(builder `test.rs`).
pub(crate) fn input_error(message: &str) -> NodeError {
    fail("GITHUB_INPUT", message)
}

pub struct GithubApi {
    base: String,
    max_bytes: usize,
    timeout: Duration,
}

pub struct Resp {
    pub status: u16,
    /// 본문이 비었거나 JSON이 아니면 `Null`.
    pub body: Value,
    /// `x-ratelimit-remaining: 0` 또는 `retry-after`가 있었다.
    pub rate_limited: bool,
    /// 요청이 GET(조회)이었다 — 403 문구가 읽기·쓰기로 갈린다.
    pub read: bool,
    pub etag: Option<String>,
    /// 한도가 풀리는 유닉스 초 — `retry-after`가 있으면 지금+그 초, 없으면 `x-ratelimit-reset`.
    pub reset_at: Option<i64>,
    pub remaining: Option<u64>,
    pub limit: Option<u64>,
}

fn num<T: std::str::FromStr>(h: &reqwest::header::HeaderMap, name: &str) -> Option<T> {
    h.get(name)?.to_str().ok()?.trim().parse().ok()
}

impl Default for GithubApi {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubApi {
    pub fn new() -> Self {
        Self::with_base(BASE)
    }

    pub fn with_base(base: &str) -> Self {
        GithubApi {
            base: base.trim_end_matches('/').to_string(),
            max_bytes: MAX_BYTES,
            timeout: TIMEOUT,
        }
    }

    pub fn with_limits(mut self, max_bytes: usize, timeout: Duration) -> Self {
        self.max_bytes = max_bytes;
        self.timeout = timeout;
        self
    }

    /// 연결 실패·시간 초과·크기 초과만 `Err`. 그 밖 상태는 `Ok`.
    pub async fn call(
        &self,
        token: &str,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Resp, NodeError> {
        self.send(token, method, path, query, body, None).await
    }

    /// 조회(GET). `etag`가 있으면 `If-None-Match`로 보내고, 바뀐 게 없으면 304(`body`는 `Null`).
    pub async fn get_if(
        &self,
        token: &str,
        path: &str,
        query: &[(&str, String)],
        etag: Option<&str>,
    ) -> Result<Resp, NodeError> {
        self.send(token, Method::GET, path, query, None, etag).await
    }

    async fn send(
        &self,
        token: &str,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        etag: Option<&str>,
    ) -> Result<Resp, NodeError> {
        let read = method == Method::GET;
        let mut url = Url::parse(&format!("{}{path}", self.base)).map_err(|_| unreachable())?;
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        }
        // 리다이렉트는 따라가지 않는다(자격 헤더가 다른 곳으로 가지 않게).
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.timeout)
            .build()
            .map_err(|_| unreachable())?;
        let mut req = client
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(USER_AGENT, "ieum-desktop");
        if let Some(e) = etag {
            req = req.header("If-None-Match", e);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let mut res = req.send().await.map_err(|_| unreachable())?;
        let status = res.status().as_u16();
        let h = res.headers();
        let rate_limited = h.get("retry-after").is_some()
            || h.get("x-ratelimit-remaining").is_some_and(|v| v == "0");
        let etag = h
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let (remaining, limit) = (num(h, "x-ratelimit-remaining"), num(h, "x-ratelimit-limit"));
        let reset_at = match num::<i64>(h, "retry-after") {
            Some(secs) => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64 + secs),
            None => num(h, "x-ratelimit-reset"),
        };
        let mut buf = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(|_| unreachable())? {
            if buf.len() + chunk.len() > self.max_bytes {
                return Err(fail("GITHUB_TOO_LARGE", "GitHub 응답이 너무 커요."));
            }
            buf.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice(&buf).unwrap_or(Value::Null);
        Ok(Resp {
            status,
            body,
            rate_limited,
            read,
            etag,
            reset_at,
            remaining,
            limit,
        })
    }
}

impl Resp {
    /// 상태가 `ok`면 본문, 아니면 상태별 정해 둔 문구(응답 본문은 싣지 않는다).
    pub fn expect(self, ok: u16) -> Result<Value, NodeError> {
        let s = self.status;
        if s == ok {
            return Ok(self.body);
        }
        Err(match s {
            401 => fail(
                "GITHUB_AUTH",
                "GitHub 로그인이 만료됐어요. 설정 > 연동에서 다시 연결해 주세요.",
            ),
            403 | 429 if self.rate_limited => fail(
                "GITHUB_RATE_LIMIT",
                "GitHub 요청 한도에 닿았어요. 잠시 뒤 다시 실행해 주세요.",
            ),
            403 if self.read => fail("GITHUB_FORBIDDEN", "이 저장소·조직을 읽을 권한이 없어요."),
            403 => fail("GITHUB_FORBIDDEN", "이 저장소에 쓸 권한이 없어요."),
            404 => fail(
                "GITHUB_NOT_FOUND",
                "저장소나 이슈를 찾지 못했어요. 이름과 권한을 확인해 주세요.",
            ),
            301 | 302 | 307 | 308 => fail(
                "GITHUB_MOVED",
                "저장소 주소가 바뀌었어요. 소유자·저장소 이름을 확인해 주세요.",
            ),
            410 => fail("GITHUB_GONE", "이 저장소는 이슈를 쓰지 않아요."),
            422 => fail(
                "GITHUB_INVALID",
                "GitHub가 입력값을 받지 않았어요. 칸 값을 확인해 주세요.",
            ),
            500..=599 => unreachable(),
            _ => fail(
                format!("GITHUB_STATUS {s}"),
                format!("GitHub가 요청을 거절했어요 (HTTP {s})."),
            ),
        })
    }
}
