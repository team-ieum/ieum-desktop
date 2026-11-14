//! Claude 구독 사용량(창별 사용 비율·초기화 시각). Claude Code가 키체인에 둔 OAuth 토큰으로
//! `api.anthropic.com`에만 묻는다. 토큰은 로그·오류·반환값 어디에도 넣지 않는다.

use std::future::Future;
use std::path::Path;
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
/// 성공값이 있으면 [새로 고침]도 이 간격 안에는 다시 묻지 않는다(연타로 429를 만들지 않게).
const MIN_REFRESH: Duration = Duration::from_secs(10);
/// 429 `Retry-After`를 따르되 이보다 오래 기다리진 않는다.
const MAX_WAIT: Duration = Duration::from_secs(15 * 60);
const WINDOWS: [&str; 3] = ["five_hour", "seven_day", "seven_day_opus"];

/// 키체인 읽기 결과.
pub enum TokenRead {
    Found(String),
    /// 토큰과 만료 시각(밀리초 epoch) — Claude 키체인의 `expiresAt`.
    Expiring(String, i64),
    /// `expiresAt`이 지났다 — 그 계정으로 claude를 실행해야 갱신된다(앱은 갱신하지 않는다).
    Expired,
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
    /// 화면에 그대로 쓰는 이름(모델·그룹별 창). 없으면 화면이 `kind`로 번역한다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanUsage {
    /// `ok` | `unavailable` | `not_logged_in` | `expired`
    pub state: &'static str,
    pub windows: Vec<PlanWindow>,
    pub checked_at: String,
    /// 이 컴퓨터에 로그인된 구독 계정(로컬 파일·키체인에서). 모르면 null.
    pub account_email: Option<String>,
}

/// 조회 한 번: (결과, 허용 창 거부라 계속 쓸지, 429면 `Retry-After`).
pub(crate) type Fetched = (PlanUsage, bool, Option<Duration>);

struct Entry {
    at: Instant,
    usage: PlanUsage,
    sticky: bool,
    /// 429 — 이때까지는 [새로 고침]도 묻지 않는다.
    wait_until: Option<Instant>,
}

/// 구독 사용량 캐시 — Claude·Codex·Antigravity 공통.
/// 조회가 "확인 불가"로 끝나도 직전 성공값을 그대로 둔다(그 값의 `checked_at`이 기준 시각).
pub(crate) struct Cache {
    ttl: Duration,
    cell: Mutex<Option<Entry>>,
}

impl Cache {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cell: Mutex::new(None),
        }
    }

    /// 저장한 결과를 버린다(거부·429 대기 표시도 같이).
    pub(crate) async fn clear(&self) {
        *self.cell.lock().await = None;
    }

    /// `fresh`([새로 고침])면 시간 안이라도 새로 읽는다. 거부·429 대기 중이면 그대로.
    pub(crate) async fn get(&self, fresh: bool, fetch: impl Future<Output = Fetched>) -> PlanUsage {
        let mut cell = self.cell.lock().await;
        if let Some(e) = cell.as_ref() {
            let ok = e.usage.state == "ok";
            let keep = if fresh && !ok {
                Duration::ZERO
            } else if fresh {
                MIN_REFRESH.min(self.ttl)
            } else {
                self.ttl
            };
            let waiting = e.wait_until.is_some_and(|w| Instant::now() < w);
            if e.sticky || waiting || e.at.elapsed() < keep {
                return e.usage.clone();
            }
        }
        let (u, sticky, retry_after) = fetch.await;
        let usage = match cell.take() {
            Some(e) if u.state == "unavailable" && !sticky && e.usage.state == "ok" => e.usage,
            _ => u,
        };
        let now = Instant::now();
        *cell = Some(Entry {
            at: now,
            usage: usage.clone(),
            sticky,
            wait_until: retry_after.map(|d| now + d.clamp(self.ttl, MAX_WAIT)),
        });
        usage
    }
}

/// 429면 `Retry-After`(초, 없으면 0 — 캐시가 최소 `ttl`로 올린다).
pub(crate) fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    (resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS).then(|| {
        let secs = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()?.trim().parse().ok())
            .unwrap_or(0);
        Duration::from_secs(secs)
    })
}

pub struct PlanUsageClient {
    url: Url,
    http: Client,
    token: TokenSource,
    cache: Cache,
    /// 마지막으로 `Found`로 읽은 토큰(메모리에만) — 백그라운드는 키체인 대신 이것을 쓴다.
    /// `Missing`·`Denied`·`Expired`면 비운다. 만료 시각(밀리초)이 있으면 같이 기억한다.
    remembered: std::sync::Mutex<Option<(String, Option<i64>)>>,
}

impl PlanUsageClient {
    pub fn new(token: TokenSource) -> Result<Self, String> {
        Self::with_url(USAGE_URL, token, CACHE_TTL)
    }

    /// `url`은 `USAGE_URL` 또는 `http://127.0.0.1`(테스트)만.
    pub fn with_url(url: &str, token: TokenSource, ttl: Duration) -> Result<Self, String> {
        let (url, http) = pinned_http(url, USAGE_URL)?;
        Ok(Self {
            url,
            http,
            token,
            cache: Cache::new(ttl),
            remembered: Default::default(),
        })
    }

    /// 백그라운드용 — 키체인(허용 창)을 부르지 않는다. 기억한 토큰이 없으면 None,
    /// 있으면 그 토큰으로 캐시를 거쳐 묻는다(실패하면 캐시 규칙대로 직전 성공값).
    /// 기억한 만료 시각이 지났으면 묻지 않고 None — CLI가 이미 갱신했을 수 있어 `expired`라 단정하지 않는다.
    /// 기억한 토큰과 캐시를 비워, 다음 일반 get이 키체인을 다시 읽게 한다(만료 표시는 그 읽기에서만).
    pub async fn get_quiet(&self) -> Option<PlanUsage> {
        let (token, until) = self.remembered().clone()?;
        if self.remembered_expired(until).await {
            return None;
        }
        Some(self.cache.get(false, self.request(token)).await)
    }

    /// 기억한 만료 시각이 지났으면 기억한 토큰과 캐시를 비우고 true.
    async fn remembered_expired(&self, until: Option<i64>) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        if !until.is_some_and(|u| is_expired(u, now)) {
            return false;
        }
        *self.remembered() = None;
        self.cache.clear().await;
        true
    }

    fn remembered(&self) -> std::sync::MutexGuard<'_, Option<(String, Option<i64>)>> {
        self.remembered.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `ttl` 안에 다시 부르면 저장해 둔 결과(키체인·네트워크를 건드리지 않음). 키체인 거부는 계속 쓴다.
    /// 기억한 만료 시각이 지났으면 캐시를 건너뛰고 키체인을 다시 읽는다.
    pub async fn get(&self) -> PlanUsage {
        let until = self.remembered().as_ref().and_then(|(_, u)| *u);
        self.remembered_expired(until).await;
        self.cache.get(false, self.fetch()).await
    }

    /// 저장해 둔 결과를 건너뛰고 새로 읽는다([새로 고침]·계정 추가 [확인]). 거부·429 대기는 그대로.
    pub async fn refresh(&self) -> PlanUsage {
        self.cache.get(true, self.fetch()).await
    }

    async fn fetch(&self) -> Fetched {
        let token = self.token.clone();
        let read = tokio::task::spawn_blocking(move || token()).await;
        *self.remembered() = match &read {
            Ok(TokenRead::Found(t)) => Some((t.clone(), None)),
            Ok(TokenRead::Expiring(t, until)) => Some((t.clone(), Some(*until))),
            _ => None,
        };
        match read {
            Ok(TokenRead::Found(t) | TokenRead::Expiring(t, _)) => self.request(t).await,
            // 만료 = 네트워크 없이 알린다. sticky 아님 — CLI로 갱신하면 다음 get이 다시 읽는다.
            Ok(TokenRead::Expired) => (usage("expired", Vec::new()), false, None),
            // 허용 창 거부 = 확인 불가(다시 묻지 않음, 설계 8절).
            Ok(TokenRead::Denied) => (usage("unavailable", Vec::new()), true, None),
            Ok(TokenRead::Missing) | Err(_) => (usage("not_logged_in", Vec::new()), false, None),
        }
    }

    /// 이미 읽은 토큰으로 사용량을 묻는다.
    async fn request(&self, token: String) -> Fetched {
        let mut wait = None;
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
                wait = retry_after(&resp);
                // 401 = 토큰 만료 — 백그라운드가 이 토큰을 다시 쓰지 않게 버린다(get은 키체인을 다시 읽는다).
                if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
                    let mut r = self.remembered();
                    if r.as_ref().is_some_and(|(t, _)| *t == token) {
                        *r = None;
                    }
                }
                return None;
            }
            resp.json().await.ok()
        }
        .await;
        let u = match body.and_then(|b| parse(&b)) {
            Some(w) => usage("ok", w),
            None => usage("unavailable", Vec::new()),
        };
        (u, false, wait)
    }
}

/// `url`은 `fixed` 그대로이거나 `http://127.0.0.1`(테스트)만. 리다이렉트 끔, 5초.
pub(crate) fn pinned_http(url: &str, fixed: &str) -> Result<(Url, Client), String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    let loopback = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if url.as_str() != fixed && !loopback {
        return Err("허용되지 않은 사용량 주소예요.".into());
    }
    let http = Client::builder()
        .redirect(redirect::Policy::none())
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    Ok((url, http))
}

pub(crate) fn usage(state: &'static str, windows: Vec<PlanWindow>) -> PlanUsage {
    PlanUsage {
        state,
        windows,
        checked_at: chrono::Utc::now().to_rfc3339(),
        account_email: None,
    }
}

/// `~/.claude.json`의 `oauthAccount.emailAddress`. 없거나 못 읽으면 None.
pub fn claude_account_email(path: &std::path::Path) -> Option<String> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    v["oauthAccount"]["emailAddress"]
        .as_str()
        .filter(|e| !e.is_empty())
        .map(str::to_string)
}

/// `{five_hour: {utilization, resets_at}, ...}`. `resets_at`이 RFC 3339가 아닌 창(아직 안 쓴 창은 null)은 뺀다.
/// 이어서 `limits[]`의 `weekly_scoped`마다 모델별 주간 창(`seven_day_model`, label = 모델 이름),
/// Fable이 없으면 옛 키(`seven_day_fable`→`fable_weekly`→`fable_seven_day`)를 Fable로. 같은 모델은 하나만.
/// 창 키가 하나도 객체로 없고 모델 창도 없으면 `None`(모르는 응답).
fn parse(body: &Value) -> Option<Vec<PlanWindow>> {
    let known = WINDOWS
        .iter()
        .any(|k| body.get(k).is_some_and(Value::is_object));
    let window = |kind, label: Option<&str>, used: Option<f64>, resets: Option<&str>| {
        let resets_at = resets?;
        chrono::DateTime::parse_from_rfc3339(resets_at).ok()?;
        Some(PlanWindow {
            kind,
            used_pct: used?,
            resets_at: resets_at.to_string(),
            label: label.map(str::to_string),
        })
    };
    let mut windows: Vec<PlanWindow> = WINDOWS
        .iter()
        .filter_map(|&kind| {
            let w = body.get(kind)?;
            window(
                kind,
                None,
                w["utilization"].as_f64(),
                w["resets_at"].as_str(),
            )
        })
        .collect();
    let scoped = body["limits"].as_array().into_iter().flatten();
    for l in scoped.filter(|l| l["kind"] == "weekly_scoped") {
        // 모델 이름이 없으면 무엇의 한도인지 보일 수 없어 뺀다.
        let Some(name) = l["scope"]["model"]["display_name"]
            .as_str()
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        let w = window(
            "seven_day_model",
            Some(name),
            l["percent"].as_f64(),
            l["resets_at"].as_str(),
        );
        if let Some(w) = w.filter(|w| !windows.iter().any(|x| x.label == w.label)) {
            windows.push(w);
        }
    }
    if !windows.iter().any(|w| w.label.as_deref() == Some("Fable")) {
        let legacy = ["seven_day_fable", "fable_weekly", "fable_seven_day"]
            .iter()
            .find_map(|k| {
                let w = body.get(k)?;
                window(
                    "seven_day_model",
                    Some("Fable"),
                    w["utilization"].as_f64(),
                    w["resets_at"].as_str(),
                )
            });
        windows.extend(legacy);
    }
    (known || !windows.is_empty()).then_some(windows)
}

/// 설정 폴더(계정)별 토큰 읽기. None = 이 Mac의 기본 로그인.
pub type ClaudeTokenFor = Arc<dyn Fn(Option<&Path>) -> TokenRead + Send + Sync>;

/// claude가 토큰을 두는 키체인 서비스 이름. `CLAUDE_CONFIG_DIR`가 있으면
/// `Claude Code-credentials-<sha256(폴더) 앞 8자>`(claude 2.1.322 바이너리의 규칙, 이 Mac 항목으로 실측).
/// ponytail: claude는 폴더 경로를 NFC로 맞춰 해시한다 — 앱 데이터 경로가 ASCII라 그대로 쓴다.
pub fn claude_keychain_service(config_dir: Option<&Path>) -> String {
    use sha2::{Digest, Sha256};
    match config_dir {
        None => "Claude Code-credentials".into(),
        Some(d) => {
            let h = Sha256::digest(d.to_string_lossy().as_bytes());
            format!("Claude Code-credentials-{}", &hex::encode(h)[..8])
        }
    }
}

/// 그 계정 키체인 항목의 `claudeAiOauth.accessToken`. 계정마다 항목 하나(허용 창도 계정마다 한 번).
pub fn keychain_token(config_dir: Option<&Path>) -> TokenRead {
    match std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            &claude_keychain_service(config_dir),
            "-w",
        ])
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(out) => token_from_security(
            out.status.code(),
            &out.stdout,
            chrono::Utc::now().timestamp_millis(),
        ),
        Err(_) => TokenRead::Missing,
    }
}

/// `security` 종료 코드는 OSStatus 아래 8비트: 44 = errSecItemNotFound(-25300, 실행 확인),
/// 128 = errSecUserCanceled(-128), 51 = errSecAuthFailed(-25293) — 허용 창 취소·거부.
/// 만료 경계 — 만료 시각과 같은 ms면 만료. 키체인 판정과 기억한 토큰 판정이 같이 쓴다.
pub fn is_expired(until_ms: i64, now_ms: i64) -> bool {
    until_ms <= now_ms
}

/// `now_ms`보다 이르거나 같은 `claudeAiOauth.expiresAt`(밀리초)은 `Expired`. 없거나 숫자가 아니면 `Found`.
pub fn token_from_security(code: Option<i32>, stdout: &[u8], now_ms: i64) -> TokenRead {
    match code {
        Some(0) => {}
        Some(128 | 51) => return TokenRead::Denied,
        _ => return TokenRead::Missing,
    }
    let Ok(v) = serde_json::from_slice::<Value>(stdout) else {
        return TokenRead::Missing;
    };
    let oauth = &v["claudeAiOauth"];
    let Some(t) = oauth["accessToken"].as_str().filter(|t| !t.is_empty()) else {
        return TokenRead::Missing;
    };
    match oauth["expiresAt"].as_i64() {
        Some(e) if is_expired(e, now_ms) => TokenRead::Expired,
        Some(e) => TokenRead::Expiring(t.to_string(), e),
        None => TokenRead::Found(t.to_string()),
    }
}
