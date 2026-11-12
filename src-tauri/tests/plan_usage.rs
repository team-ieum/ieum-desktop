//! Claude 구독 사용량. 가짜 HTTP 서버와 가짜 토큰 출처만 쓴다(실제 키체인·Anthropic 금지).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ieum_desktop_lib::plan_usage::{
    CACHE_TTL, ClaudeTokenFor, PlanUsage, PlanUsageClient, TokenRead, TokenSource,
    claude_keychain_service, token_from_security,
};
use ieum_desktop_lib::sub_usage::SubscriptionUsage;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "sk-ant-oat01-secret-token";

#[derive(Debug, Clone)]
struct Req {
    head: String,
}

type Log = Arc<Mutex<Vec<Req>>>;

/// 요청마다 `route`가 고른 (상태, 본문 글)을 보내는 가짜 서버.
async fn serve(route: impl Fn(&Req) -> (u16, String) + Send + Sync + 'static) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/oauth/usage", l.local_addr().unwrap());
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
                let req = Req {
                    head: String::from_utf8_lossy(&buf).to_string(),
                };
                let (status, body) = route(&req);
                seen.lock().unwrap().push(req);
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

fn token(t: Option<&str>) -> TokenSource {
    let t = t.map(str::to_string);
    Arc::new(move || t.clone().map_or(TokenRead::Missing, TokenRead::Found))
}

fn client(url: &str, t: Option<&str>) -> PlanUsageClient {
    PlanUsageClient::with_url(url, token(t), CACHE_TTL).unwrap()
}

fn sample() -> Value {
    json!({
        "five_hour": {"utilization": 23.0, "resets_at": "2026-11-12T15:00:00.123+00:00"},
        "seven_day": {"utilization": 61.5, "resets_at": "2026-11-15T03:00:00+00:00"},
        "seven_day_opus": {"utilization": 0, "resets_at": null},
        "seven_day_sonnet": {"utilization": 5.0, "resets_at": "2026-11-15T03:00:00+00:00"}
    })
}

#[tokio::test]
async fn plan_usage_parses_windows() {
    let (url, log) = serve(|_| (200, sample().to_string())).await;
    let u = client(&url, Some(TOKEN)).get().await;
    assert_eq!(u.state, "ok");
    // 초기화 시각이 없는 창(opus)과 모르는 창(sonnet)은 뺀다.
    let got: Vec<(&str, f64, &str)> = u
        .windows
        .iter()
        .map(|w| (w.kind, w.used_pct, w.resets_at.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("five_hour", 23.0, "2026-11-12T15:00:00.123+00:00"),
            ("seven_day", 61.5, "2026-11-15T03:00:00+00:00"),
        ]
    );
    assert!(!u.checked_at.is_empty());
    // 화면이 받는 모양(camelCase).
    let v = serde_json::to_value(&u).unwrap();
    assert_eq!(v["windows"][0]["usedPct"], 23.0);
    assert!(v["windows"][0].get("resetsAt").is_some());
    assert!(v.get("checkedAt").is_some());

    let head = log.lock().unwrap()[0].head.to_ascii_lowercase();
    assert!(head.starts_with("get /api/oauth/usage "), "{head}");
    assert!(head.contains(&format!(
        "authorization: bearer {}",
        TOKEN.to_ascii_lowercase()
    )));
    assert!(head.contains("anthropic-beta: oauth-2025-04-20"));
}

#[tokio::test]
async fn plan_usage_unavailable_on_5xx_and_bad_json() {
    for (status, body) in [
        (500, sample().to_string()),
        (401, sample().to_string()),
        (200, "not json".to_string()),
        (200, "{}".to_string()),
    ] {
        let (url, _) = serve(move |_| (status, body.clone())).await;
        let u = client(&url, Some(TOKEN)).get().await;
        assert_eq!(u.state, "unavailable", "{status}");
        assert!(u.windows.is_empty());
    }
}

#[tokio::test]
async fn plan_usage_not_logged_in_without_keychain_item() {
    let (url, log) = serve(|_| (200, sample().to_string())).await;
    let u = client(&url, None).get().await;
    assert_eq!(u.state, "not_logged_in");
    assert!(u.windows.is_empty());
    assert!(log.lock().unwrap().is_empty(), "토큰 없이 요청하지 않는다");
}

fn assert_clean(u: &PlanUsage, secrets: &[&str]) {
    let shown = format!("{u:?} {}", serde_json::to_string(u).unwrap());
    for s in secrets {
        assert!(!shown.contains(s), "비밀이 결과에 섞임: {shown}");
    }
}

#[tokio::test]
async fn plan_usage_token_never_in_error() {
    // 서버가 받은 헤더를 그대로 되돌려 줘도 결과에는 나가지 않는다.
    for status in [200, 500] {
        let (url, _) = serve(move |r| {
            let echo = r.head.replace(['"', '\r', '\n'], " ");
            let body = json!({
                "error": echo,
                "five_hour": {"utilization": 1.0, "resets_at": echo},
            });
            (status, body.to_string())
        })
        .await;
        let u = client(&url, Some(TOKEN)).get().await;
        assert_clean(&u, &[TOKEN]);
    }
}

#[tokio::test]
async fn plan_usage_cached_60s() {
    let (url, log) = serve(|_| (200, sample().to_string())).await;
    let c = client(&url, Some(TOKEN));
    let a = c.get().await;
    let b = c.get().await;
    assert_eq!(log.lock().unwrap().len(), 1);
    assert_eq!(a.checked_at, b.checked_at);
}

#[test]
fn plan_usage_only_anthropic_or_loopback() {
    let bad = "https://evil.example/api/oauth/usage";
    assert!(PlanUsageClient::with_url(bad, token(None), CACHE_TTL).is_err());
    let ok = "http://127.0.0.1:1/api/oauth/usage";
    assert!(PlanUsageClient::with_url(ok, token(None), CACHE_TTL).is_ok());
}

#[tokio::test]
async fn plan_usage_ok_when_windows_all_unused() {
    // 로그인돼 있고 API도 정상인데 두 창 모두 아직 안 씀 → 창 없이 ok.
    let body = r#"{"five_hour":{"utilization":0,"resets_at":null},"seven_day":{"utilization":0,"resets_at":null},"seven_day_opus":null}"#;
    let (url, _) = serve(move |_| (200, body.to_string())).await;
    let u = client(&url, Some(TOKEN)).get().await;
    assert_eq!(u.state, "ok");
    assert!(u.windows.is_empty());
}

/// 읽을 때마다 세는 가짜 키체인.
fn counting(read: fn() -> TokenRead) -> (TokenSource, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    let src: TokenSource = Arc::new(move || {
        seen.fetch_add(1, Ordering::SeqCst);
        read()
    });
    (src, n)
}

#[tokio::test]
async fn plan_usage_denied_not_asked_again() {
    let url = "http://127.0.0.1:1/api/oauth/usage";
    // 캐시 시간 0: 항목 없음은 매번 다시 보고, 거부는 한 번만 묻는다.
    let (missing, n_missing) = counting(|| TokenRead::Missing);
    let c = PlanUsageClient::with_url(url, missing, Duration::ZERO).unwrap();
    for _ in 0..3 {
        assert_eq!(c.get().await.state, "not_logged_in");
    }
    assert_eq!(n_missing.load(Ordering::SeqCst), 3);

    let (denied, n_denied) = counting(|| TokenRead::Denied);
    let c = PlanUsageClient::with_url(url, denied, Duration::ZERO).unwrap();
    for _ in 0..3 {
        assert_eq!(c.get().await.state, "not_logged_in");
    }
    assert_eq!(n_denied.load(Ordering::SeqCst), 1);
}

/// 계정(설정 폴더)마다 다른 토큰. `deny`인 폴더는 허용 창 거부.
fn per_dir(deny: Option<PathBuf>) -> (ClaudeTokenFor, Arc<Mutex<Vec<Option<PathBuf>>>>) {
    let seen: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::default();
    let s = seen.clone();
    let f: ClaudeTokenFor = Arc::new(move |d: Option<&Path>| {
        s.lock().unwrap().push(d.map(Path::to_path_buf));
        if d.is_some() && d.map(Path::to_path_buf) == deny {
            return TokenRead::Denied;
        }
        TokenRead::Found(format!(
            "{TOKEN}-{}",
            d.map_or("system".into(), |d| d.display().to_string())
        ))
    });
    (f, seen)
}

fn subs(url: &str, token: ClaudeTokenFor) -> SubscriptionUsage {
    let home = std::env::temp_dir().join("ieum-no-home");
    SubscriptionUsage::with(
        url,
        "http://127.0.0.1:1/x",
        CACHE_TTL,
        token,
        Arc::new(|_: Option<&Path>| None),
        home,
    )
    .unwrap()
}

#[tokio::test]
async fn usage_cache_is_per_account() {
    let (url, log) = serve(|_| (200, sample().to_string())).await;
    let (token, _) = per_dir(None);
    let s = subs(&url, token);
    let a = Some(PathBuf::from("/accts/a"));
    let b = Some(PathBuf::from("/accts/b"));
    assert_eq!(s.get("claude", a.clone()).await.unwrap().state, "ok");
    assert_eq!(s.get("claude", b.clone()).await.unwrap().state, "ok");
    assert_eq!(log.lock().unwrap().len(), 2);
    s.get("claude", a.clone()).await.unwrap();
    s.get("claude", b).await.unwrap();
    assert_eq!(log.lock().unwrap().len(), 2);
    // 계정마다 자기 토큰.
    let heads: Vec<String> = log.lock().unwrap().iter().map(|r| r.head.clone()).collect();
    assert!(heads[0].contains("/accts/a") && heads[1].contains("/accts/b"));
    assert!(s.get("gemini", a).await.is_err());
}

#[tokio::test]
async fn denied_account_not_asked_again_others_still_work() {
    let (url, log) = serve(|_| (200, sample().to_string())).await;
    let a = PathBuf::from("/accts/denied");
    let (token, seen) = per_dir(Some(a.clone()));
    let s = subs(&url, token);
    for _ in 0..3 {
        assert_eq!(
            s.get("claude", Some(a.clone())).await.unwrap().state,
            "not_logged_in"
        );
    }
    let asked = |p: &Option<PathBuf>| seen.lock().unwrap().iter().filter(|x| *x == p).count();
    assert_eq!(asked(&Some(a.clone())), 1);
    // 확인(새로 읽기)도 거부한 계정은 다시 묻지 않는다.
    assert_eq!(
        s.refresh("claude", Some(a.clone())).await.unwrap().state,
        "not_logged_in"
    );
    assert_eq!(asked(&Some(a)), 1);
    assert_eq!(s.get("claude", None).await.unwrap().state, "ok");
    assert_eq!(
        s.get("claude", Some("/accts/ok".into()))
            .await
            .unwrap()
            .state,
        "ok"
    );
    assert_eq!(log.lock().unwrap().len(), 2);
}

#[test]
fn keychain_service_follows_config_dir() {
    assert_eq!(claude_keychain_service(None), "Claude Code-credentials");
    // 실측: 이 Mac에서 Orca가 만든 계정 폴더의 항목 이름과 같은 규칙(sha256 앞 8자).
    assert_eq!(
        claude_keychain_service(Some(Path::new(
            "/Users/dobee/Library/Application Support/orca/claude-profiles/62c40c5b-4d1f-4a3c-87ab-222023a1470b/home"
        ))),
        "Claude Code-credentials-a1a82ba4"
    );
}

#[test]
fn security_exit_codes_map_to_token_read() {
    let ok = br#"{"claudeAiOauth":{"accessToken":"abc"}}"#;
    assert!(matches!(token_from_security(Some(0), ok), TokenRead::Found(t) if t == "abc"));
    assert!(matches!(
        token_from_security(Some(0), b"{}"),
        TokenRead::Missing
    ));
    assert!(matches!(
        token_from_security(Some(44), b""),
        TokenRead::Missing
    ));
    assert!(matches!(
        token_from_security(Some(128), b""),
        TokenRead::Denied
    ));
    assert!(matches!(
        token_from_security(Some(51), b""),
        TokenRead::Denied
    ));
    assert!(matches!(token_from_security(None, b""), TokenRead::Missing));
}

/// 수동 확인용(실제 키체인·Anthropic): `cargo test -p ieum-desktop --test plan_usage -- --ignored`.
#[tokio::test]
#[ignore]
async fn plan_usage_live() {
    let s = SubscriptionUsage::new().unwrap();
    let u = s.get("claude", None).await.unwrap();
    println!("state={} windows={}", u.state, u.windows.len());
}
