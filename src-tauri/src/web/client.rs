//! BE 클라이언트. 리다이렉트 끔·응답 1MB·15초 상한. GET 외 메서드는 로그인·갱신·로그아웃뿐.
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

/// 동시에 여러 요청이 401을 받아도 갱신은 한 번만(갱신 토큰이 회전돼도 서로 지우지 않게).
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 화면용 에러 코드 `AUTH_FAILED`·`AUTH_LOCKED`·`AUTH_EXPIRED`·`NETWORK`·`SERVER`. 직렬화 = `{code, message?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WebError {
    AuthFailed,
    AuthLocked,
    AuthExpired,
    Network(String),
    Server(String),
}

impl WebError {
    pub fn code(&self) -> &'static str {
        match self {
            WebError::AuthFailed => "AUTH_FAILED",
            WebError::AuthLocked => "AUTH_LOCKED",
            WebError::AuthExpired => "AUTH_EXPIRED",
            WebError::Network(_) => "NETWORK",
            WebError::Server(_) => "SERVER",
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
            WebError::Network(m) => write!(f, "서버에 연결하지 못했어요: {m}"),
            WebError::Server(m) => write!(f, "서버 오류: {m}"),
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

fn net(e: reqwest::Error) -> WebError {
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

pub struct BeClient {
    base: String,
    http: Client,
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
        Ok(Self { base, http, keys })
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
            .send(Method::POST, "/api/v1/auth/login", None, Some(body))
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
                .send(Method::POST, "/api/v1/auth/logout", None, Some(body))
                .await;
        }
        end_session(self.keys.as_ref(), &self.base).map_err(WebError::Server)
    }

    /// `path`는 `/`로 시작(`/api/v1/...`). 401이면 갱신 1회 후 재시도, 또 401이면 세션 삭제 + `AuthExpired`.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, WebError> {
        let s = self.session()?;
        let r = self
            .send(Method::GET, path, Some(&s.access_token), None)
            .await?;
        if r.status() != StatusCode::UNAUTHORIZED {
            return data(r).await;
        }
        let s = self.refresh(&s.access_token).await?;
        let r = self
            .send(Method::GET, path, Some(&s.access_token), None)
            .await?;
        if r.status() == StatusCode::UNAUTHORIZED {
            self.expire();
            return Err(WebError::AuthExpired);
        }
        data(r).await
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
            .send(Method::POST, "/api/v1/auth/refresh", None, Some(body))
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
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> Result<Response, WebError> {
        // `/`로 시작해야 호스트가 바뀌지 않는다(`@evil.com` 같은 값 차단).
        if !path.starts_with('/') {
            return Err(WebError::Server("잘못된 요청 경로예요.".into()));
        }
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        req.send().await.map_err(net)
    }
}
