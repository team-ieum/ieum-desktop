//! Codex·Antigravity 구독이 이 컴퓨터에 연결됐는지(로그인 계정·남은 사용량)만 읽는다.
//! AI 노드 실행과는 무관하다. 토큰은 로그·오류·반환값·프로세스 인자 어디에도 넣지 않는다.
//!
//! - Codex: `~/.codex/auth.json`(또는 `$CODEX_HOME`)의 토큰으로 `chatgpt.com/backend-api/wham/usage`에만 묻는다
//!   (Orca·claude-dashboard가 쓰는 주소).
//! - Antigravity: 로그인 계정은 키체인 `gemini`/`antigravity`(없으면 `~/.gemini/antigravity-cli/antigravity-oauth-token`)의
//!   `id_token_email`. 사용량은 Orca처럼 `agy -p /usage --output-format json`(agy 1.1.11+)이 직접 답한다 — 앱은 토큰을 쓰지 않는다.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{Client, Url};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::plan_usage::{PlanUsage, PlanWindow, TokenRead, pinned_http, usage};

pub const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// agy가 `/usage`를 명령으로 받는 최소 버전. 그 전 버전은 프롬프트로 보내 사용량을 쓴다(Orca 근거).
const AGY_MIN_VERSION: (u64, u64, u64) = (1, 1, 11);
const AGY_VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// agy가 서버에 묻고 답하는 데 실측 6초 — HTTP 5초 상한 대신 CLI 자체 20초 + 프로세스 30초.
const AGY_USAGE_TIMEOUT: Duration = Duration::from_secs(30);
const AGY_ENV: &str = "AGY_CLI_DISABLE_AUTO_UPDATE=true";

/// (저장 시각, 결과, 거부라 계속 쓸지) — `PlanUsageClient`와 같은 규칙.
struct Cache {
    ttl: Duration,
    cell: Mutex<Option<(Instant, PlanUsage, bool)>>,
}

impl Cache {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cell: Mutex::new(None),
        }
    }

    async fn get(&self, fetch: impl Future<Output = (PlanUsage, bool)>) -> PlanUsage {
        let mut cell = self.cell.lock().await;
        if let Some((at, u, sticky)) = cell.as_ref()
            && (*sticky || at.elapsed() < self.ttl)
        {
            return u.clone();
        }
        let (u, sticky) = fetch.await;
        *cell = Some((Instant::now(), u.clone(), sticky));
        u
    }
}

// ---------- Codex ----------

#[derive(Clone)]
pub struct CodexAuth {
    access_token: String,
    account_id: String,
    email: Option<String>,
}

impl std::fmt::Debug for CodexAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexAuth")
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// 자격 읽기(동기 — `spawn_blocking`에서 부른다). 없으면 None.
pub type CodexAuthSource = Arc<dyn Fn() -> Option<CodexAuth> + Send + Sync>;

/// `auth.json`의 `tokens.{access_token, account_id, id_token}`. API 키 모드(구독 아님)는 None.
pub fn codex_auth_from_json(raw: &[u8]) -> Option<CodexAuth> {
    let v: Value = serde_json::from_slice(raw).ok()?;
    let t = &v["tokens"];
    let s = |k: &str| t[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    Some(CodexAuth {
        access_token: s("access_token")?,
        account_id: s("account_id")?,
        email: t["id_token"].as_str().and_then(jwt_email),
    })
}

/// `$CODEX_HOME/auth.json`(기본 `~/.codex/auth.json`).
pub fn codex_auth_file() -> Option<CodexAuth> {
    let home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".codex")))?;
    codex_auth_from_json(&std::fs::read(home.join("auth.json")).ok()?)
}

/// JWT 가운데 조각의 `email`(서명은 보지 않는다 — 화면 표시용).
fn jwt_email(jwt: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(&b64(jwt.split('.').nth(1)?)?).ok()?;
    v["email"].as_str().map(str::to_string)
}

pub struct CodexUsageClient {
    url: Url,
    http: Client,
    auth: CodexAuthSource,
    cache: Cache,
}

impl CodexUsageClient {
    pub fn new(auth: CodexAuthSource) -> Result<Self, String> {
        Self::with_url(CODEX_USAGE_URL, auth, crate::plan_usage::CACHE_TTL)
    }

    /// `url`은 `CODEX_USAGE_URL` 또는 `http://127.0.0.1`(테스트)만.
    pub fn with_url(url: &str, auth: CodexAuthSource, ttl: Duration) -> Result<Self, String> {
        let (url, http) = pinned_http(url, CODEX_USAGE_URL)?;
        Ok(Self {
            url,
            http,
            auth,
            cache: Cache::new(ttl),
        })
    }

    pub async fn get(&self) -> PlanUsage {
        self.cache.get(self.fetch()).await
    }

    async fn fetch(&self) -> (PlanUsage, bool) {
        let src = self.auth.clone();
        let Some(auth) = tokio::task::spawn_blocking(move || src())
            .await
            .ok()
            .flatten()
        else {
            return (usage("not_logged_in", Vec::new()), false);
        };
        // 오류(reqwest 포함)는 버린다 — 헤더 값이 섞일 여지를 두지 않는다.
        let body: Option<Value> = async {
            let resp = self
                .http
                .get(self.url.clone())
                .bearer_auth(&auth.access_token)
                .header("ChatGPT-Account-Id", &auth.account_id)
                .send()
                .await
                .ok()?;
            if !resp.status().is_success() {
                return None;
            }
            resp.json().await.ok()
        }
        .await;
        let mut u = match body.and_then(|b| parse_codex(&b)) {
            Some(w) => usage("ok", w),
            None => usage("unavailable", Vec::new()),
        };
        u.account_email = auth.email;
        (u, false)
    }
}

/// `{plan_type, rate_limit: {primary_window, secondary_window}}`, 창은 `{used_percent, limit_window_seconds, reset_at(초)}`.
/// 하루 이하 창은 `session`, 그보다 길면 `weekly`.
fn parse_codex(body: &Value) -> Option<Vec<PlanWindow>> {
    body["plan_type"].as_str()?;
    let rl = body.get("rate_limit")?.as_object()?;
    let windows = ["primary_window", "secondary_window"]
        .iter()
        .filter_map(|k| {
            let w = rl.get(*k)?;
            let secs = w["limit_window_seconds"].as_u64()?;
            let reset = chrono::DateTime::from_timestamp(w["reset_at"].as_i64()?, 0)?;
            Some(PlanWindow {
                kind: if secs <= 86_400 { "session" } else { "weekly" },
                used_pct: w["used_percent"].as_f64()?,
                resets_at: reset.to_rfc3339(),
            })
        })
        .collect();
    Some(windows)
}

// ---------- Antigravity ----------

/// `agy -p /usage` 실행 결과.
pub enum AgyRun {
    Out(String),
    /// 로그인 셸 PATH에 `agy`가 없음.
    NotInstalled,
    /// 실패·시간 초과·너무 옛 버전.
    Failed,
}

/// 로그인 읽기 — `Found(이메일, 모르면 빈 글)`·`Missing`·`Denied`(허용 창 거부, 다시 묻지 않음).
pub type AgyLoginSource = Arc<dyn Fn() -> TokenRead + Send + Sync>;
pub type AgyRunner = Arc<dyn Fn() -> AgyRun + Send + Sync>;

pub struct AntigravityUsageClient {
    login: AgyLoginSource,
    run: AgyRunner,
    cache: Cache,
}

impl AntigravityUsageClient {
    pub fn new(login: AgyLoginSource, run: AgyRunner, ttl: Duration) -> Self {
        Self {
            login,
            run,
            cache: Cache::new(ttl),
        }
    }

    pub async fn get(&self) -> PlanUsage {
        self.cache.get(self.fetch()).await
    }

    async fn fetch(&self) -> (PlanUsage, bool) {
        let login = self.login.clone();
        let email = match tokio::task::spawn_blocking(move || login()).await {
            Ok(TokenRead::Found(e)) => e,
            Ok(TokenRead::Denied) => return (usage("not_logged_in", Vec::new()), true),
            Ok(TokenRead::Missing) | Err(_) => return (usage("not_logged_in", Vec::new()), false),
        };
        let run = self.run.clone();
        let mut u = match tokio::task::spawn_blocking(move || run()).await {
            Ok(AgyRun::Out(out)) => match parse_agy(&out) {
                Some(w) => usage("ok", w),
                None => usage("unavailable", Vec::new()),
            },
            Ok(AgyRun::NotInstalled) => usage("not_logged_in", Vec::new()),
            Ok(AgyRun::Failed) | Err(_) => usage("unavailable", Vec::new()),
        };
        u.account_email = Some(email).filter(|e| !e.is_empty());
        (u, false)
    }
}

/// `{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"buckets":[{window,remaining_fraction,reset_time,disabled}]}]}}}` 줄.
/// 창(`5h`→`session`, `weekly`)마다 가장 많이 쓴 묶음(Orca 표시와 같음). 쓸 묶음이 없으면 None.
fn parse_agy(out: &str) -> Option<Vec<PlanWindow>> {
    let v = out
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .find(|v| v["status"] == "SUCCESS" && v["command"]["name"] == "usage")?;
    let mut best: Vec<PlanWindow> = Vec::new();
    let buckets = v["command"]["data"]["groups"]
        .as_array()?
        .iter()
        .flat_map(|g| g["buckets"].as_array().into_iter().flatten());
    for b in buckets {
        let kind = match b["window"].as_str() {
            _ if b["disabled"] == true => continue,
            Some("5h") => "session",
            Some("weekly") => "weekly",
            _ => continue,
        };
        let (Some(rf), Some(reset)) = (b["remaining_fraction"].as_f64(), b["reset_time"].as_str())
        else {
            continue;
        };
        if chrono::DateTime::parse_from_rfc3339(reset).is_err() {
            continue;
        }
        let w = PlanWindow {
            kind,
            used_pct: ((1.0 - rf) * 100.0).clamp(0.0, 100.0).round(),
            resets_at: reset.to_string(),
        };
        match best.iter_mut().find(|x| x.kind == kind) {
            Some(x) if x.used_pct >= w.used_pct => {}
            Some(x) => *x = w,
            None => best.push(w),
        }
    }
    best.sort_by_key(|w| w.kind != "session");
    Some(best).filter(|b| !b.is_empty())
}

/// agy가 저장한 자격 글(go-keyring `go-keyring-base64:`/`go-keyring-encoded:` 감싸기 또는 날 JSON)에서
/// 토큰이 있는지와 `id_token_email`만 본다.
pub fn agy_login_from_secret(raw: &[u8]) -> TokenRead {
    let parsed = || -> Option<String> {
        let s = std::str::from_utf8(raw).ok()?.trim();
        let json = if let Some(b) = s.strip_prefix("go-keyring-base64:") {
            b64(b)?
        } else if let Some(h) = s.strip_prefix("go-keyring-encoded:") {
            hex::decode(h).ok()?
        } else {
            s.as_bytes().to_vec()
        };
        let v: Value = serde_json::from_slice(&json).ok()?;
        let tok = v["token"]["access_token"]
            .as_str()
            .or(v["access_token"].as_str());
        tok.filter(|t| !t.trim().is_empty())?;
        Some(v["id_token_email"].as_str().unwrap_or("").to_string())
    };
    parsed().map_or(TokenRead::Missing, TokenRead::Found)
}

/// 키체인 `gemini`/`antigravity`(agy가 쓰는 항목), 없으면 토큰 파일. 종료 코드 뜻은 `token_from_security`와 같다.
pub fn agy_login() -> TokenRead {
    let out = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "gemini",
            "-a",
            "antigravity",
            "-w",
        ])
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => return agy_login_from_secret(&o.stdout),
        Ok(o) if matches!(o.status.code(), Some(128 | 51)) => return TokenRead::Denied,
        _ => {}
    }
    std::env::home_dir()
        .map(|h| h.join(".gemini/antigravity-cli/antigravity-oauth-token"))
        .and_then(|p| std::fs::read(p).ok())
        .map_or(TokenRead::Missing, |raw| agy_login_from_secret(&raw))
}

/// 로그인 셸 PATH의 `agy`로 `/usage`. 버전을 먼저 본다(옛 버전은 사용량을 쓴다).
pub fn agy_usage_cli() -> AgyRun {
    use ieum_local::ai::shell_output;
    let Some(v) = shell_output(
        &format!("command -v agy >/dev/null && {AGY_ENV} agy --version"),
        AGY_VERSION_TIMEOUT,
    ) else {
        return AgyRun::NotInstalled;
    };
    let mut parts = v.trim().split('.').map(|p| p.parse::<u64>().ok());
    let ver = (parts.next(), parts.next(), parts.next());
    let (Some(Some(a)), Some(Some(b)), Some(Some(c))) = ver else {
        return AgyRun::Failed;
    };
    if (a, b, c) < AGY_MIN_VERSION {
        return AgyRun::Failed;
    }
    shell_output(
        &format!("{AGY_ENV} agy -p /usage --output-format json --print-timeout 20s"),
        AGY_USAGE_TIMEOUT,
    )
    .map_or(AgyRun::Failed, AgyRun::Out)
}

/// base64(표준·URL-safe 둘 다, 패딩 무시). 새 의존성 없이 JWT·go-keyring 감싸기만 푼다.
fn b64(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in s.trim().trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6 | u32::from(v)) & 0xFFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
