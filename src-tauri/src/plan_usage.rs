//! Claude 구독 사용량(창별 사용 비율·초기화 시각). Claude Code가 키체인에 둔 OAuth 토큰으로
//! `api.anthropic.com`에만 묻는다. 토큰은 로그·오류·반환값 어디에도 넣지 않는다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{Client, Url, redirect};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;

/// 요청은 이 주소로만 간다(테스트는 `with_url`로 127.0.0.1만 허용).
pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TIMEOUT: Duration = Duration::from_secs(5);
pub const CACHE_TTL: Duration = Duration::from_secs(60);
const WINDOWS: [&str; 3] = ["five_hour", "seven_day", "seven_day_opus"];

/// 키체인 읽기 결과.
pub enum TokenRead {
    Found(String),
    /// 항목 없음·읽기 실패 — 60초 뒤 다시 본다.
    Missing,
    /// 허용 창에서 거부·취소 — 앱이 도는 동안 다시 묻지 않는다.
    Denied,
}

/// 토큰 읽기(동기 — 키체인 허용 창이 뜰 수 있어 `spawn_blocking`에서 부른다).
pub type TokenSource = Arc<dyn Fn() -> TokenRead + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWindow {
    pub kind: &'static str,
    pub used_pct: f64,
    pub resets_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanUsage {
    /// `ok` | `unavailable` | `not_logged_in`
    pub state: &'static str,
    pub windows: Vec<PlanWindow>,
    pub checked_at: String,
}

pub struct PlanUsageClient {
    url: Url,
    http: Client,
    token: TokenSource,
    ttl: Duration,
    /// (저장 시각, 결과, 거부라 계속 쓸지)
    cache: Mutex<Option<(Instant, PlanUsage, bool)>>,
}

impl PlanUsageClient {
    pub fn new(token: TokenSource) -> Result<Self, String> {
        Self::with_url(USAGE_URL, token, CACHE_TTL)
    }

    /// `url`은 `USAGE_URL` 또는 `http://127.0.0.1`(테스트)만.
    pub fn with_url(url: &str, token: TokenSource, ttl: Duration) -> Result<Self, String> {
        let url = Url::parse(url).map_err(|e| e.to_string())?;
        let loopback = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
        if url.as_str() != USAGE_URL && !loopback {
            return Err("허용되지 않은 사용량 주소예요.".into());
        }
        let http = Client::builder()
            .redirect(redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            url,
            http,
            token,
            ttl,
            cache: Mutex::new(None),
        })
    }

    /// `ttl` 안에 다시 부르면 저장해 둔 결과(키체인·네트워크를 건드리지 않음). 키체인 거부는 계속 쓴다.
    pub async fn get(&self) -> PlanUsage {
        let mut cache = self.cache.lock().await;
        if let Some((at, u, sticky)) = cache.as_ref()
            && (*sticky || at.elapsed() < self.ttl)
        {
            return u.clone();
        }
        let (u, sticky) = self.fetch().await;
        *cache = Some((Instant::now(), u.clone(), sticky));
        u
    }

    async fn fetch(&self) -> (PlanUsage, bool) {
        let token = self.token.clone();
        let token = match tokio::task::spawn_blocking(move || token()).await {
            Ok(TokenRead::Found(t)) => t,
            Ok(TokenRead::Denied) => return (usage("not_logged_in", Vec::new()), true),
            Ok(TokenRead::Missing) | Err(_) => return (usage("not_logged_in", Vec::new()), false),
        };
        // 오류(reqwest 포함)는 버린다 — 헤더 값이 섞일 여지를 두지 않는다.
        let body: Option<Value> = async {
            let resp = self
                .http
                .get(self.url.clone())
                .bearer_auth(&token)
                .header("anthropic-beta", "oauth-2025-04-20")
                .send()
                .await
                .ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.json().await.ok()
        }
        .await;
        let u = match body.and_then(|b| parse(&b)) {
            Some(w) => usage("ok", w),
            None => usage("unavailable", Vec::new()),
        };
        (u, false)
    }
}

fn usage(state: &'static str, windows: Vec<PlanWindow>) -> PlanUsage {
    PlanUsage {
        state,
        windows,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// `{five_hour: {utilization, resets_at}, ...}`. `resets_at`이 RFC 3339가 아닌 창(아직 안 쓴 창은 null)은 뺀다.
/// 창 키가 하나도 객체로 없으면 `None`(모르는 응답).
fn parse(body: &Value) -> Option<Vec<PlanWindow>> {
    if !WINDOWS
        .iter()
        .any(|k| body.get(k).is_some_and(Value::is_object))
    {
        return None;
    }
    let windows = WINDOWS
        .iter()
        .filter_map(|&kind| {
            let w = body.get(kind)?;
            let resets_at = w.get("resets_at")?.as_str()?;
            chrono::DateTime::parse_from_rfc3339(resets_at).ok()?;
            Some(PlanWindow {
                kind,
                used_pct: w.get("utilization")?.as_f64()?,
                resets_at: resets_at.to_string(),
            })
        })
        .collect();
    Some(windows)
}

/// macOS 키체인 `Claude Code-credentials`의 `claudeAiOauth.accessToken`. 앱이 읽는 항목은 이것 하나.
pub fn keychain_token() -> TokenRead {
    match std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(out) => token_from_security(out.status.code(), &out.stdout),
        Err(_) => TokenRead::Missing,
    }
}

/// `security` 종료 코드는 OSStatus 아래 8비트: 44 = errSecItemNotFound(-25300, 실행 확인),
/// 128 = errSecUserCanceled(-128), 51 = errSecAuthFailed(-25293) — 허용 창 취소·거부.
pub fn token_from_security(code: Option<i32>, stdout: &[u8]) -> TokenRead {
    match code {
        Some(0) => {}
        Some(128 | 51) => return TokenRead::Denied,
        _ => return TokenRead::Missing,
    }
    serde_json::from_slice::<Value>(stdout)
        .ok()
        .and_then(|v| {
            v["claudeAiOauth"]["accessToken"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| !t.is_empty())
        .map_or(TokenRead::Missing, TokenRead::Found)
}
