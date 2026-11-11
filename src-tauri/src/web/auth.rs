//! 세션(키체인 계정 `ieum-session`에 JSON 한 값)·서버 주소 검증·설정 저장.
//! 토큰은 키체인에만 둔다 — 설정 파일·실행 기록·로그·이벤트에 넣지 않는다. 비밀번호는 저장하지 않는다.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

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

/// 서버별 세션 세대. 로그아웃·서버 변경 때 올려, 그 전에 시작한 로그인·갱신이 세션을 되살리지 못하게 한다.
static GENERATIONS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

fn generations() -> MutexGuard<'static, BTreeMap<String, u64>> {
    GENERATIONS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn session_generation(server_url: &str) -> u64 {
    generations().get(server_url).copied().unwrap_or(0)
}

/// 세대를 올리고 세션을 지운다(같은 잠금 안에서 — 저장과 엇갈리지 않게).
pub fn end_session(keys: &dyn KeyStore, server_url: &str) -> Result<(), String> {
    let mut g = generations();
    *g.entry(server_url.to_string()).or_default() += 1;
    clear_session(keys)
}

/// 세대가 `generation` 그대로일 때만 저장. 그사이 로그아웃됐으면 `Ok(false)`.
pub fn store_session_if(keys: &dyn KeyStore, s: &Session, generation: u64) -> Result<bool, String> {
    let g = generations();
    if g.get(&s.server_url).copied().unwrap_or(0) != generation {
        return Ok(false);
    }
    store_session(keys, s).map(|()| true)
}

/// 허용 서버. 운영 BE 하나와 개발용 `http://localhost`·`http://127.0.0.1`(포트 무관)만.
const ALLOWED_HTTPS: &[&str] = &["https://green.kiacms.co.kr"];
const ALLOWED_LOCAL_HOSTS: &[&str] = &["localhost", "127.0.0.1"];

/// 허용 목록(`ALLOWED_HTTPS`, 개발용 로컬 http) 안의 주소만.
/// 사용자 정보·경로·쿼리·조각은 거부. 정규형(끝 `/` 없음)을 돌려준다.
pub fn validate_server_url(raw: &str) -> Result<String, String> {
    let bad = || format!("서버 주소가 올바르지 않아요: {raw:?} (허용된 서버만)");
    let url = Url::parse(raw.trim()).map_err(|_| bad())?;
    let origin = url.origin().ascii_serialization();
    let local = url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|h| ALLOWED_LOCAL_HOSTS.contains(&h));
    let allowed = local || ALLOWED_HTTPS.contains(&origin.as_str());
    if !allowed
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    Ok(origin)
}

/// 설정 저장. 서버 주소를 검증하고, 바뀌면 세션을 먼저 지운다(토큰이 다른 서버로 가지 않게).
pub fn save_settings(
    store: &Store,
    keys: &dyn KeyStore,
    mut s: Settings,
) -> Result<Settings, String> {
    s.server_url = validate_server_url(&s.server_url)?;
    let old = store.get_settings()?.server_url;
    if old != s.server_url {
        end_session(keys, &old)?;
    }
    store.set_settings(&s)?;
    Ok(s)
}
