//! Codex·Antigravity 구독 연결 상태와 Claude 계정. 가짜 HTTP 서버·가짜 CLI·가짜 자격만 쓴다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ieum_desktop_lib::plan_usage::{CACHE_TTL, PlanUsage, TokenRead, claude_account_email};
use ieum_desktop_lib::sub_usage::{
    AgyRun, AntigravityUsageClient, CodexAuth, CodexUsageClient, agy_login_from_secret,
    agy_version_ok, codex_auth_from_json,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "codex-secret-access-token";
const AGY_TOKEN: &str = "ya29.agy-secret-access-token";
/// `{"email":"a@b.c"}`의 base64url.
const JWT_PAYLOAD: &str = "eyJlbWFpbCI6ImFAYi5jIn0";

type Log = Arc<Mutex<Vec<String>>>;

/// 요청마다 `route`가 고른 (상태, 본문)을 보내는 가짜 서버. 받은 요청 머리를 모은다.
async fn serve(route: impl Fn(&str) -> (u16, String) + Send + Sync + 'static) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/backend-api/wham/usage", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let (seen, route) = (seen.clone(), route.clone());
            tokio::spawn(async move {
                let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
                while !String::from_utf8_lossy(&buf).contains("\r\n\r\n") {
                    let n = sock.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let (status, body) = route(&head);
                seen.lock().unwrap().push(head);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (url, log)
}

fn assert_clean(u: &PlanUsage, secrets: &[&str]) {
    let shown = format!("{u:?} {}", serde_json::to_string(u).unwrap());
    for s in secrets {
        assert!(!shown.contains(s), "비밀이 결과에 섞임: {shown}");
    }
}

// ---------- Codex ----------

fn codex(url: &str, auth: Option<CodexAuth>) -> CodexUsageClient {
    CodexUsageClient::with_url(url, Arc::new(move || auth.clone()), CACHE_TTL).unwrap()
}

fn codex_auth() -> CodexAuth {
    let raw = json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": format!("eyJhbGciOiJub25lIn0.{JWT_PAYLOAD}.sig"),
            "access_token": TOKEN,
            "refresh_token": "r",
            "account_id": "acct-1"
        }
    });
    codex_auth_from_json(raw.to_string().as_bytes()).unwrap()
}

fn codex_sample() -> Value {
    json!({
        "plan_type": "plus",
        "rate_limit": {
            "primary_window": {"used_percent": 12, "limit_window_seconds": 18000, "reset_at": 1_794_000_000},
            "secondary_window": {"used_percent": 40.5, "limit_window_seconds": 604800, "reset_at": 1_794_500_000}
        }
    })
}

#[tokio::test]
async fn codex_usage_parses_windows() {
    let (url, log) = serve(|_| (200, codex_sample().to_string())).await;
    let u = codex(&url, Some(codex_auth())).get().await;
    assert_eq!(u.state, "ok");
    let got: Vec<(&str, f64)> = u.windows.iter().map(|w| (w.kind, w.used_pct)).collect();
    assert_eq!(got, vec![("session", 12.0), ("weekly", 40.5)]);
    assert!(chrono::DateTime::parse_from_rfc3339(&u.windows[0].resets_at).is_ok());
    assert_eq!(u.account_email.as_deref(), Some("a@b.c"));
    let v = serde_json::to_value(&u).unwrap();
    assert_eq!(v["accountEmail"], "a@b.c");

    let head = log.lock().unwrap()[0].to_ascii_lowercase();
    assert!(head.starts_with("get /backend-api/wham/usage "), "{head}");
    assert!(head.contains(&format!("authorization: bearer {TOKEN}")));
    assert!(head.contains("chatgpt-account-id: acct-1"));
}

#[tokio::test]
async fn codex_not_logged_in_without_credentials() {
    let (url, log) = serve(|_| (200, codex_sample().to_string())).await;
    let u = codex(&url, None).get().await;
    assert_eq!(u.state, "not_logged_in");
    assert!(u.windows.is_empty());
    assert!(log.lock().unwrap().is_empty(), "자격 없이 요청하지 않는다");
    // API 키 모드(구독 아님)·토큰 빈 파일은 로그인 아님.
    assert!(codex_auth_from_json(br#"{"auth_mode":"apikey","OPENAI_API_KEY":"k"}"#).is_none());
    assert!(codex_auth_from_json(b"not json").is_none());
}

#[tokio::test]
async fn codex_unavailable_on_5xx_and_bad_json() {
    for (status, body) in [
        (500, codex_sample().to_string()),
        (401, codex_sample().to_string()),
        (200, "not json".to_string()),
        (200, "{}".to_string()),
    ] {
        let (url, _) = serve(move |_| (status, body.clone())).await;
        let u = codex(&url, Some(codex_auth())).get().await;
        assert_eq!(u.state, "unavailable", "{status}");
        assert!(u.windows.is_empty());
        // 계정은 로컬 파일에서 읽었으니 사용량이 안 돼도 보인다.
        assert_eq!(u.account_email.as_deref(), Some("a@b.c"));
    }
}

#[tokio::test]
async fn codex_token_never_in_error() {
    for status in [200, 500] {
        let (url, _) = serve(move |head| {
            let echo = head.replace(['"', '\r', '\n'], " ");
            let body = json!({
                "error": echo,
                "plan_type": echo,
                "rate_limit": {"primary_window": {"used_percent": 1, "limit_window_seconds": 18000, "reset_at": echo}}
            });
            (status, body.to_string())
        })
        .await;
        let u = codex(&url, Some(codex_auth())).get().await;
        assert_clean(&u, &[TOKEN]);
    }
    assert!(!format!("{:?}", codex_auth()).contains(TOKEN));
}

#[test]
fn codex_only_chatgpt_or_loopback() {
    let none = || Arc::new(|| None) as ieum_desktop_lib::sub_usage::CodexAuthSource;
    assert!(CodexUsageClient::with_url("https://evil.example/x", none(), CACHE_TTL).is_err());
    assert!(CodexUsageClient::with_url("http://127.0.0.1:1/x", none(), CACHE_TTL).is_ok());
}

// ---------- Antigravity ----------

fn agy_sample() -> String {
    let line = json!({
        "status": "SUCCESS",
        "num_turns": 0,
        "command": {"name": "usage", "data": {"groups": [
            {"name": "Gemini Models", "buckets": [
                {"id": "g-w", "name": "Weekly", "window": "weekly", "remaining_fraction": 0.8, "reset_time": "2026-11-20T00:00:00Z"},
                {"id": "g-5", "name": "5h", "window": "5h", "remaining_fraction": 1.0, "reset_time": "2026-11-13T05:00:00Z"}
            ]},
            {"name": "Claude and GPT models", "buckets": [
                {"id": "c-w", "name": "Weekly", "window": "weekly", "remaining_fraction": 0.25, "reset_time": "2026-11-19T00:00:00Z"},
                {"id": "c-5", "name": "5h", "window": "5h", "remaining_fraction": 0.9, "reset_time": "2026-11-13T04:00:00Z"},
                {"id": "off", "name": "Off", "window": "5h", "remaining_fraction": 0.0, "reset_time": "2026-11-13T04:00:00Z", "disabled": true}
            ]}
        ]}}
    });
    format!("some banner\n{line}\n")
}

type Calls = Arc<AtomicUsize>;

fn agy(login: fn() -> TokenRead, run: fn() -> AgyRun) -> (AntigravityUsageClient, Calls) {
    let n: Calls = Arc::default();
    let seen = n.clone();
    let c = AntigravityUsageClient::new(
        Arc::new(move || match login() {
            TokenRead::Found(s) => agy_login_from_secret(s.as_bytes()),
            other => other,
        }),
        Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            run()
        }),
        CACHE_TTL,
    );
    (c, n)
}

fn agy_secret() -> TokenRead {
    let raw = json!({
        "token": {"access_token": AGY_TOKEN, "refresh_token": "r", "expiry": "2026-11-13T00:00:00Z"},
        "auth_method": "oauth",
        "id_token_email": "me@example.com"
    });
    // 키체인에는 go-keyring이 base64로 감싸 둔다.
    let b64 = {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = raw.to_string().into_bytes();
        let mut s = String::new();
        for ch in bytes.chunks(3) {
            let n = ch
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, b)| a | (*b as u32) << (16 - 8 * i));
            for i in 0..=ch.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
        }
        s
    };
    TokenRead::Found(format!("go-keyring-base64:{b64}"))
}

#[tokio::test]
async fn antigravity_usage_parses_windows() {
    let (c, _) = agy(agy_secret, || AgyRun::Out(agy_sample()));
    let u = c.get().await;
    assert_eq!(u.state, "ok");
    // 묶음마다 창 하나, 응답 순서(꺼진 묶음 제외).
    let got: Vec<(&str, &str, f64, &str)> = u
        .windows
        .iter()
        .map(|w| {
            let l = w.label.as_deref().unwrap_or("");
            (w.kind, l, w.used_pct, w.resets_at.as_str())
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "weekly",
                "Gemini Models · Weekly",
                20.0,
                "2026-11-20T00:00:00Z"
            ),
            ("session", "Gemini Models · 5h", 0.0, "2026-11-13T05:00:00Z"),
            (
                "weekly",
                "Claude and GPT models · Weekly",
                75.0,
                "2026-11-19T00:00:00Z"
            ),
            (
                "session",
                "Claude and GPT models · 5h",
                10.0,
                "2026-11-13T04:00:00Z"
            ),
        ]
    );
    assert_eq!(u.account_email.as_deref(), Some("me@example.com"));
}

/// 2026-11-13 실측 모양: Argon만 다 썼는데 "주간 100%"로 합쳐 보이면 안 된다.
#[tokio::test]
async fn antigravity_windows_per_group() {
    fn run() -> AgyRun {
        let b = |name: &str, window: &str, rf: f64| {
            json!({"name": name, "window": window, "remaining_fraction": rf,
                   "reset_time": "2026-11-19T00:00:00Z"})
        };
        let line = json!({
            "status": "SUCCESS",
            "command": {"name": "usage", "data": {"groups": [
                {"name": "Gemini Models", "buckets": [
                    b("Weekly Limit Remaining", "weekly", 0.9976),
                    b("5 Hour Limit Remaining", "5h", 0.9953)
                ]},
                {"name": "Claude and GPT models", "buckets": [
                    b("Weekly Limit Remaining", "weekly", 1.0),
                    b("5 Hour Limit Remaining", "5h", 1.0)
                ]},
                {"name": "Gemini 4 Argon", "buckets": [
                    b("Gemini 4 Argon Weekly Limit", "weekly", 0.0)
                ]}
            ]}}
        });
        AgyRun::Out(line.to_string())
    }
    let (c, _) = agy(agy_secret, run);
    let u = c.get().await;
    let got: Vec<(&str, &str, f64)> = u
        .windows
        .iter()
        .map(|w| (w.kind, w.label.as_deref().unwrap_or(""), w.used_pct))
        .collect();
    assert_eq!(
        got,
        vec![
            ("weekly", "Gemini Models · Weekly Limit Remaining", 0.0),
            ("session", "Gemini Models · 5 Hour Limit Remaining", 0.0),
            (
                "weekly",
                "Claude and GPT models · Weekly Limit Remaining",
                0.0
            ),
            (
                "session",
                "Claude and GPT models · 5 Hour Limit Remaining",
                0.0
            ),
            ("weekly", "Gemini 4 Argon Weekly Limit", 100.0),
        ]
    );
}

#[tokio::test]
async fn antigravity_not_logged_in_without_credentials() {
    let (c, calls) = agy(|| TokenRead::Missing, || AgyRun::Out(agy_sample()));
    let u = c.get().await;
    assert_eq!(u.state, "not_logged_in");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "자격 없이 CLI를 부르지 않는다"
    );
    // 자격은 있는데 agy가 없음 → 로그인 안내.
    let (c, _) = agy(agy_secret, || AgyRun::NotInstalled);
    assert_eq!(c.get().await.state, "not_logged_in");
    // 키체인 거부 = 확인 불가(로그인 안내 아님).
    let (c, _) = agy(|| TokenRead::Denied, || AgyRun::Out(agy_sample()));
    assert_eq!(c.get().await.state, "unavailable");
    // 토큰 없는 비밀은 로그인 아님.
    assert!(matches!(
        agy_login_from_secret(br#"{"id_token_email":"x@y.z"}"#),
        TokenRead::Missing
    ));
}

#[tokio::test]
async fn antigravity_unavailable_on_5xx_and_bad_json() {
    let error = json!({"status": "ERROR", "error": "server 503"}).to_string();
    let no_groups =
        json!({"status": "SUCCESS", "command": {"name": "usage", "data": {"groups": []}}})
            .to_string();
    for out in [
        AgyRun::Failed,
        AgyRun::Out("not json".into()),
        AgyRun::Out(error),
        AgyRun::Out(no_groups),
    ] {
        let out = Arc::new(Mutex::new(Some(out)));
        let c = AntigravityUsageClient::new(
            Arc::new(|| match agy_secret() {
                TokenRead::Found(s) => agy_login_from_secret(s.as_bytes()),
                o => o,
            }),
            Arc::new(move || out.lock().unwrap().take().unwrap_or(AgyRun::Failed)),
            CACHE_TTL,
        );
        let u = c.get().await;
        assert_eq!(u.state, "unavailable");
        assert!(u.windows.is_empty());
        assert_eq!(u.account_email.as_deref(), Some("me@example.com"));
    }
}

#[tokio::test]
async fn antigravity_token_never_in_error() {
    let (c, _) = agy(agy_secret, || AgyRun::Failed);
    assert_clean(&c.get().await, &[AGY_TOKEN]);
    let (c, _) = agy(agy_secret, || AgyRun::Out(agy_sample()));
    assert_clean(&c.get().await, &[AGY_TOKEN]);
    let TokenRead::Found(s) = agy_secret() else {
        unreachable!()
    };
    let read = agy_login_from_secret(s.as_bytes());
    assert!(matches!(&read, TokenRead::Found(e) if e == "me@example.com"));
}

#[tokio::test]
async fn antigravity_cached_60s() {
    let (c, calls) = agy(agy_secret, || AgyRun::Out(agy_sample()));
    let a = c.get().await;
    let b = c.get().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(a.checked_at, b.checked_at);
}

#[tokio::test]
async fn antigravity_denied_not_asked_again() {
    // 캐시 시간 0: 없음은 매번 다시 보고, 거부는 한 번만 묻는다.
    /// (읽기, 물은 횟수, 상태)
    type Case = (fn() -> TokenRead, usize, &'static str);
    let cases: [Case; 2] = [
        (|| TokenRead::Missing, 3, "not_logged_in"),
        (|| TokenRead::Denied, 1, "unavailable"),
    ];
    for (read, want, state) in cases {
        let n: Calls = Arc::default();
        let seen = n.clone();
        let c = AntigravityUsageClient::new(
            Arc::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                read()
            }),
            Arc::new(|| AgyRun::Out(agy_sample())),
            std::time::Duration::ZERO,
        );
        for _ in 0..3 {
            assert_eq!(c.get().await.state, state);
        }
        assert_eq!(n.load(Ordering::SeqCst), want);
    }
}

#[tokio::test]
async fn antigravity_concurrent_calls_run_agy_once() {
    let (c, calls) = agy(agy_secret, || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        AgyRun::Out(agy_sample())
    });
    let (a, b) = tokio::join!(c.get(), c.get());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "동시 호출에도 agy는 하나만"
    );
    assert_eq!(a.checked_at, b.checked_at);
}

#[test]
fn agy_version_gate() {
    assert!(!agy_version_ok("1.1.10\n"));
    assert!(agy_version_ok("1.1.11\n"));
    assert!(agy_version_ok("1.5.1"));
    assert!(agy_version_ok("2.0.0"));
    assert!(!agy_version_ok("0.9.99"));
    assert!(!agy_version_ok("unknown"));
    assert!(!agy_version_ok(""));
}

#[tokio::test]
async fn codex_window_kinds_by_length() {
    // 무료 플랜: 30일 창 하나(secondary null). 7일 정확히는 weekly.
    let body = json!({
        "plan_type": "free",
        "rate_limit": {
            "primary_window": {"used_percent": 3, "limit_window_seconds": 2_592_000, "reset_at": 1_794_000_000},
            "secondary_window": null
        }
    });
    let (url, _) = serve(move |_| (200, body.to_string())).await;
    let u = codex(&url, Some(codex_auth())).get().await;
    let got: Vec<&str> = u.windows.iter().map(|w| w.kind).collect();
    assert_eq!(got, vec!["monthly"]);

    let body = json!({
        "plan_type": "plus",
        "rate_limit": {
            "primary_window": {"used_percent": 1, "limit_window_seconds": 86_400, "reset_at": 1_794_000_000},
            "secondary_window": {"used_percent": 2, "limit_window_seconds": 604_800, "reset_at": 1_794_000_000}
        }
    });
    let (url, _) = serve(move |_| (200, body.to_string())).await;
    let u = codex(&url, Some(codex_auth())).get().await;
    let got: Vec<&str> = u.windows.iter().map(|w| w.kind).collect();
    assert_eq!(got, vec!["session", "weekly"]);
}

// ---------- Claude 계정 ----------

#[test]
fn claude_account_email_read_from_config() {
    let dir = std::env::temp_dir().join(format!("ieum-claude-json-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(".claude.json");
    std::fs::write(
        &f,
        r#"{"oauthAccount":{"emailAddress":"me@example.com","accountUuid":"u"}}"#,
    )
    .unwrap();
    assert_eq!(claude_account_email(&f).as_deref(), Some("me@example.com"));
    std::fs::write(&f, r#"{"numStartups":3}"#).unwrap();
    assert_eq!(claude_account_email(&f), None);
    assert_eq!(claude_account_email(&dir.join("missing.json")), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 수동 확인용(실제 파일·키체인·agy·chatgpt.com): `cargo test -p ieum-desktop --test sub_usage -- --ignored --nocapture`.
/// 값은 찍지 않고 상태·창 개수·이메일 유무만.
#[tokio::test]
#[ignore]
async fn sub_usage_live() {
    use ieum_desktop_lib::sub_usage;
    let c = CodexUsageClient::new(Arc::new(|| sub_usage::codex_auth_file(None))).unwrap();
    let u = c.get().await;
    println!(
        "codex state={} windows={} email={}",
        u.state,
        u.windows.len(),
        u.account_email.is_some()
    );
    let a = AntigravityUsageClient::new(
        Arc::new(sub_usage::agy_login),
        Arc::new(sub_usage::agy_usage_cli),
        CACHE_TTL,
    );
    let u = a.get().await;
    println!(
        "antigravity state={} windows={} email={}",
        u.state,
        u.windows.len(),
        u.account_email.is_some()
    );
    let home = std::env::home_dir().unwrap();
    println!(
        "claude email={}",
        claude_account_email(&home.join(".claude.json")).is_some()
    );
}
