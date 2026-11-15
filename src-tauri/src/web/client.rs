//! BE 클라이언트. 리다이렉트 끔·응답 1MB·15초 상한. GET 외 메서드는 로그인·갱신·로그아웃과
//! 가입(인증번호 발송·확인·가입, `post_public`)뿐.
//! 에러에는 토큰·비밀번호는 물론 서버가 보낸 메시지도 넣지 않는다(받은 값을 되돌려 줄 수 있어서).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use ieum_local::ai::KeyStore;
use reqwest::{Client, Method, Response, StatusCode, redirect};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::{
    Session, WebAccount, clear_session, end_session, load_session, session_generation,
    store_session_if, validate_server_url,
};

pub const TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_BODY: usize = 1024 * 1024;
/// SSE 연결 상한. 유휴·이벤트 크기·전체 시간은 `sse::SseLimits`.
pub const STREAM_CONNECT: Duration = Duration::from_secs(10);

/// 401이어도 이음 세션 만료가 아니라 외부 연동(OAuth) 만료인 BE `ErrorCode` — 세션을 지우지 않는다.
const EXTERNAL_401: &[&str] = &["CREDENTIAL_EXPIRED", "AUTHENTICATION_REQUIRED"];

/// 인증번호가 틀렸거나 쓸 수 없는 BE `ErrorCode`(400) → `CodeInvalid`.
/// 같은 계열의 `VERIFICATION_RESEND_TOO_SOON`·`VERIFICATION_BLOCKED`는 429라 `AuthLocked`.
const CODE_ERRORS: &[&str] = &[
    "VERIFICATION_CODE_INVALID",
    "VERIFICATION_CODE_EXPIRED",
    "VERIFICATION_ATTEMPTS_EXCEEDED",
    "EMAIL_NOT_VERIFIED",
];

/// 동시에 여러 요청이 401을 받아도 갱신은 한 번만(갱신 토큰이 회전돼도 서로 지우지 않게).
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 화면용 에러 코드 `AUTH_FAILED`·`AUTH_LOCKED`·`AUTH_EXPIRED`·`EXTERNAL_EXPIRED`·`NETWORK`·`SERVER`,
/// 가입용 `EMAIL_TAKEN`·`CODE_INVALID`·`BAD_INPUT`.
/// 직렬화 = `{code, message?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WebError {
    AuthFailed,
    AuthLocked,
    AuthExpired,
    /// 외부 서비스 연동(OAuth) 만료 — 이음 로그인은 그대로.
    ExternalExpired,
    Network(String),
    Server(String),
    EmailTaken,
    /// 인증번호가 틀렸거나 만료·횟수 초과, 또는 인증 전 가입.
    CodeInvalid,
    /// 그 밖의 입력 거절(4xx).
    BadInput,
}

impl WebError {
    pub fn code(&self) -> &'static str {
        match self {
            WebError::AuthFailed => "AUTH_FAILED",
            WebError::AuthLocked => "AUTH_LOCKED",
            WebError::AuthExpired => "AUTH_EXPIRED",
            WebError::ExternalExpired => "EXTERNAL_EXPIRED",
            WebError::Network(_) => "NETWORK",
            WebError::Server(_) => "SERVER",
            WebError::EmailTaken => "EMAIL_TAKEN",
            WebError::CodeInvalid => "CODE_INVALID",
            WebError::BadInput => "BAD_INPUT",
        }
    }
}

impl fmt::Display for WebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebError::AuthFailed => f.write_str("이메일 또는 비밀번호가 맞지 않아요."),
            WebError::AuthLocked => {
                f.write_str("로그인 시도가 많아 잠시 잠겼어요. 나중에 다시 시도해 주세요.")
            }
            WebError::AuthExpired => f.write_str("로그인이 만료됐어요. 다시 로그인해 주세요."),
            WebError::ExternalExpired => {
                f.write_str("연결된 외부 서비스 인증이 만료됐어요. 웹 이음에서 다시 연동해 주세요.")
            }
            WebError::Network(m) => write!(f, "서버에 연결하지 못했어요: {m}"),
            WebError::Server(m) => write!(f, "서버 오류: {m}"),
            WebError::EmailTaken => f.write_str("이미 가입된 이메일이에요."),
            WebError::CodeInvalid => f.write_str("인증번호가 맞지 않거나 만료됐어요."),
            WebError::BadInput => f.write_str("입력한 내용을 다시 확인해 주세요."),
        }
    }
}

impl std::error::Error for WebError {}

/// BE `TokenResponse`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    access_token: String,
    refresh_token: String,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Tokens(***)")
    }
}

/// BE 공통 래퍼 `{success, data, message, code}` — `message`는 쓰지 않는다.
#[derive(Deserialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
}

pub(crate) fn net(e: reqwest::Error) -> WebError {
    if e.is_timeout() {
        WebError::Network("시간 초과".into())
    } else {
        WebError::Network(e.without_url().to_string())
    }
}

/// 1MB 넘는 응답은 끝까지 읽지 않는다.
async fn read_body(mut r: Response) -> Result<Vec<u8>, WebError> {
    let too_big = || WebError::Server("응답이 너무 커요.".into());
    if r.content_length().is_some_and(|n| n > MAX_BODY as u64) {
        return Err(too_big());
    }
    let mut buf = Vec::new();
    while let Some(c) = r.chunk().await.map_err(net)? {
        if buf.len() + c.len() > MAX_BODY {
            return Err(too_big());
        }
        buf.extend_from_slice(&c);
    }
    Ok(buf)
}

/// 성공 응답의 `data`. 그 밖은 상태 코드만 담은 `Server`.
async fn data<T: DeserializeOwned>(r: Response) -> Result<T, WebError> {
    let status = r.status();
    let body = read_body(r).await?;
    if !status.is_success() {
        return Err(WebError::Server(format!("HTTP {}", status.as_u16())));
    }
    // serde 오류 메시지는 받은 값을 인용하므로 쓰지 않는다.
    let resp: ApiResponse<T> = serde_json::from_slice(&body)
        .map_err(|_| WebError::Server("응답 형식이 예상과 달라요.".into()))?;
    if !resp.success {
        return Err(WebError::Server("요청이 거절됐어요.".into()));
    }
    resp.data
        .ok_or_else(|| WebError::Server("응답에 data가 없어요.".into()))
}

/// 실패 응답 BE 래퍼의 `code`(`message`는 보지 않는다).
async fn be_code(r: Response) -> Option<String> {
    #[derive(Deserialize)]
    struct Code {
        code: Option<String>,
    }
    let body = read_body(r).await.ok()?;
    serde_json::from_slice::<Code>(&body).ok()?.code
}

/// 401 응답이 외부 연동 만료(`EXTERNAL_401`)인가.
async fn external_401(r: Response) -> bool {
    be_code(r)
        .await
        .is_some_and(|c| EXTERNAL_401.contains(&c.as_str()))
}

pub struct BeClient {
    base: String,
    http: Client,
    /// SSE용 — 전체 시간 상한 없음(연결 10초만).
    stream_http: Client,
    keys: Arc<dyn KeyStore>,
}

impl BeClient {
    /// `server_url`은 `validate_server_url`을 통과해야 한다.
    pub fn new(server_url: &str, keys: Arc<dyn KeyStore>) -> Result<Self, WebError> {
        let base = validate_server_url(server_url).map_err(WebError::Server)?;
        let http = Client::builder()
            .redirect(redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .map_err(net)?;
        let stream_http = Client::builder()
            .redirect(redirect::Policy::none())
            .connect_timeout(STREAM_CONNECT)
            .build()
            .map_err(net)?;
        Ok(Self {
            base,
            http,
            stream_http,
            keys,
        })
    }

    pub fn account(&self) -> Option<WebAccount> {
        match load_session(self.keys.as_ref(), &self.base) {
            Ok(s) => s.map(|s| WebAccount {
                email: s.email,
                server_url: self.base.clone(),
            }),
            Err(e) => {
                eprintln!("[ieum] 세션을 읽지 못했어요: {e}");
                None
            }
        }
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<WebAccount, WebError> {
        let generation = session_generation(&self.base);
        let body = json!({"email": email, "password": password});
        let r = self
            .send(
                &self.http,
                Method::POST,
                "/api/v1/auth/login",
                None,
                Some(body),
            )
            .await?;
        if r.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err(WebError::AuthLocked);
        }
        if r.status().is_client_error() {
            return Err(WebError::AuthFailed);
        }
        let t: Tokens = data(r).await?;
        let s = Session {
            email: email.to_string(),
            server_url: self.base.clone(),
            access_token: t.access_token,
            refresh_token: t.refresh_token,
        };
        // 그사이 로그아웃·서버 변경이 있었으면 저장하지 않는다.
        if !store_session_if(self.keys.as_ref(), &s, generation).map_err(WebError::Server)? {
            return Err(WebError::AuthExpired);
        }
        Ok(WebAccount {
            email: s.email,
            server_url: s.server_url,
        })
    }

    /// BE에 알리고(실패해도 무시) 로컬 세션은 항상 지운다.
    pub async fn logout(&self) -> Result<(), WebError> {
        if let Ok(Some(s)) = load_session(self.keys.as_ref(), &self.base) {
            let body = json!({"refreshToken": s.refresh_token});
            let _ = self
                .send(
                    &self.http,
                    Method::POST,
                    "/api/v1/auth/logout",
                    None,
                    Some(body),
                )
                .await;
        }
        end_session(self.keys.as_ref(), &self.base).map_err(WebError::Server)
    }

    /// 인증 헤더 없는 POST(가입용). 2xx면 본문을 보지 않는다.
    /// 429 → `AuthLocked`, 5xx 등 → `Server`, 4xx는 BE `code`로 `EmailTaken`·`CodeInvalid`·`BadInput`.
    pub(crate) async fn post_public(&self, path: &str, body: Value) -> Result<(), WebError> {
        let r = self
            .send(&self.http, Method::POST, path, None, Some(body))
            .await?;
        let status = r.status();
        if status.is_success() {
            return Ok(());
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(WebError::AuthLocked);
        }
        if !status.is_client_error() {
            return Err(WebError::Server(format!("HTTP {}", status.as_u16())));
        }
        Err(match be_code(r).await.as_deref() {
            Some("EMAIL_ALREADY_EXISTS") => WebError::EmailTaken,
            Some(c) if CODE_ERRORS.contains(&c) => WebError::CodeInvalid,
            _ => WebError::BadInput,
        })
    }

    /// `path`는 `/`로 시작(`/api/v1/...`). 401 규칙은 `authed`.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, WebError> {
        data(self.authed(&self.http, path, None).await?).await
    }

    /// SSE용 GET — 전역 15초·1MB 상한 대신 헤더 수신(401 처리 포함)까지 `wait`만
    /// (`STREAM_CONNECT`가 기본). 본문 상한은 `sse::read_events`가 건다.
    pub async fn stream(&self, path: &str, wait: Duration) -> Result<Response, WebError> {
        let r = self.authed(&self.stream_http, path, Some(wait)).await?;
        if !r.status().is_success() {
            return Err(WebError::Server(format!("HTTP {}", r.status().as_u16())));
        }
        Ok(r)
    }

    /// GET. 401이면 갱신 1회 후 재시도, 또 401이면 세션 삭제 + `AuthExpired`.
    /// 외부 연동 만료 401(`EXTERNAL_401`)은 갱신·삭제 없이 `ExternalExpired`.
    /// `wait`는 GET 한 번(헤더·401 본문)마다의 상한. 갱신은 끊으면 회전된 토큰을 잃으니 걸지 않는다(15초 상한이 있다).
    async fn authed(
        &self,
        http: &Client,
        path: &str,
        wait: Option<Duration>,
    ) -> Result<Response, WebError> {
        let s = self.session()?;
        match self.try_get(http, path, &s.access_token, wait).await? {
            Ok(r) => return Ok(r),
            Err(true) => return Err(WebError::ExternalExpired),
            Err(false) => {}
        }
        let s = self.refresh(&s.access_token).await?;
        match self.try_get(http, path, &s.access_token, wait).await? {
            Ok(r) => Ok(r),
            Err(true) => Err(WebError::ExternalExpired),
            Err(false) => {
                self.expire();
                Err(WebError::AuthExpired)
            }
        }
    }

    /// 401이 아니면 `Ok(응답)`, 401이면 `Err(외부 연동 만료인가)`.
    async fn try_get(
        &self,
        http: &Client,
        path: &str,
        token: &str,
        wait: Option<Duration>,
    ) -> Result<Result<Response, bool>, WebError> {
        let get = async {
            let r = self
                .send(http, Method::GET, path, Some(token), None)
                .await?;
            if r.status() != StatusCode::UNAUTHORIZED {
                return Ok(Ok(r));
            }
            Ok(Err(external_401(r).await))
        };
        match wait {
            Some(d) => tokio::time::timeout(d, get)
                .await
                .unwrap_or_else(|_| Err(WebError::Network("시간 초과".into()))),
            None => get.await,
        }
    }

    fn session(&self) -> Result<Session, WebError> {
        load_session(self.keys.as_ref(), &self.base)
            .map_err(WebError::Server)?
            .ok_or(WebError::AuthExpired)
    }

    fn expire(&self) {
        if let Err(e) = clear_session(self.keys.as_ref()) {
            eprintln!("[ieum] 세션을 지우지 못했어요: {e}");
        }
    }

    /// `failed`(거절된 접근 토큰)를 이미 다른 요청이 갱신했으면 그 세션을 쓴다.
    async fn refresh(&self, failed: &str) -> Result<Session, WebError> {
        let _one = REFRESH.lock().await;
        let generation = session_generation(&self.base);
        let mut s = self.session()?;
        if s.access_token != failed {
            return Ok(s);
        }
        let body = json!({"refreshToken": s.refresh_token});
        let r = self
            .send(
                &self.http,
                Method::POST,
                "/api/v1/auth/refresh",
                None,
                Some(body),
            )
            .await?;
        if r.status().is_client_error() {
            self.expire();
            return Err(WebError::AuthExpired);
        }
        let t: Tokens = data(r).await?;
        s.access_token = t.access_token;
        s.refresh_token = t.refresh_token;
        if !store_session_if(self.keys.as_ref(), &s, generation).map_err(WebError::Server)? {
            return Err(WebError::AuthExpired);
        }
        Ok(s)
    }

    async fn send(
        &self,
        http: &Client,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Result<Response, WebError> {
        // `/`로 시작해야 호스트가 바뀌지 않는다(`@evil.com` 같은 값 차단).
        if !path.starts_with('/') {
            return Err(WebError::Server("잘못된 요청 경로예요.".into()));
        }
        let mut req = http.request(method, format!("{}{path}", self.base));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        req.send().await.map_err(net)
    }
}
