//! 세션(키체인 계정 `ieum-session`에 JSON 한 값)·서버 주소 검증·설정 저장.
//! 토큰은 키체인에만 둔다 — 설정 파일·실행 기록·로그·이벤트에 넣지 않는다. 비밀번호는 저장하지 않는다.

use std::fmt;

use ieum_local::ai::KeyStore;
pub use ieum_local::ai::keys::SESSION_ACCOUNT;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::store::{Settings, Store};

/// 화면에 보이는 계정 정보(토큰 없음).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAccount {
    pub email: String,
    pub server_url: String,
}

/// 키체인에 저장하는 세션. `Debug`는 토큰을 가린다.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub email: String,
    /// 이 토큰을 발급한 서버. 다른 서버로는 보내지 않는다.
    pub server_url: String,
    pub access_token: String,
    pub refresh_token: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("email", &self.email)
            .field("server_url", &self.server_url)
            .field("access_token", &"***")
            .field("refresh_token", &"***")
            .finish()
    }
}

/// `server_url`의 세션. 없거나 깨졌거나 다른 서버 것이면 `None`(깨진·다른 서버 세션은 지운다).
pub fn load_session(keys: &dyn KeyStore, server_url: &str) -> Result<Option<Session>, String> {
    let Some(raw) = keys.get(SESSION_ACCOUNT).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    match serde_json::from_str::<Session>(&raw) {
        Ok(s) if s.server_url == server_url => Ok(Some(s)),
        // 파싱 오류 메시지는 토큰 조각을 담을 수 있어 쓰지 않는다.
        _ => clear_session(keys).map(|()| None),
    }
}

pub fn store_session(keys: &dyn KeyStore, s: &Session) -> Result<(), String> {
    let raw = serde_json::to_string(s).map_err(|_| "세션을 저장하지 못했어요.".to_string())?;
    keys.set(SESSION_ACCOUNT, &raw).map_err(|e| e.to_string())
}

pub fn clear_session(keys: &dyn KeyStore) -> Result<(), String> {
    keys.delete(SESSION_ACCOUNT).map_err(|e| e.to_string())
}

/// `https://호스트[:포트]`만(개발용 `http://localhost`·`http://127.0.0.1` 예외).
/// 사용자 정보·경로·쿼리·조각은 거부. 정규형(끝 `/` 없음)을 돌려준다.
pub fn validate_server_url(raw: &str) -> Result<String, String> {
    let bad = || format!("서버 주소가 올바르지 않아요: {raw:?} (https://호스트 형식)");
    let url = Url::parse(raw.trim()).map_err(|_| bad())?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1"));
    let scheme_ok = url.scheme() == "https" || (url.scheme() == "http" && local);
    if !scheme_ok
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    Ok(url.origin().ascii_serialization())
}

/// 설정 저장. 서버 주소를 검증하고, 바뀌면 세션을 먼저 지운다(토큰이 다른 서버로 가지 않게).
pub fn save_settings(
    store: &Store,
    keys: &dyn KeyStore,
    mut s: Settings,
) -> Result<Settings, String> {
    s.server_url = validate_server_url(&s.server_url)?;
    if store.get_settings()?.server_url != s.server_url {
        clear_session(keys)?;
    }
    store.set_settings(&s)?;
    Ok(s)
}
