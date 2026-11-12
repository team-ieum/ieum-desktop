//! Claude 구독 사용량. 가짜 HTTP 서버와 가짜 토큰 출처만 쓴다(실제 키체인·Anthropic 금지).

use std::sync::{Arc, Mutex};

use ieum_desktop_lib::plan_usage::{PlanUsage, PlanUsageClient, TokenSource};
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
    Arc::new(move || t.clone())
}

fn client(url: &str, t: Option<&str>) -> PlanUsageClient {
    PlanUsageClient::with_url(url, token(t)).unwrap()
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
    assert!(
        PlanUsageClient::with_url("https://evil.example/api/oauth/usage", token(None)).is_err()
    );
    assert!(PlanUsageClient::with_url("http://127.0.0.1:1/api/oauth/usage", token(None)).is_ok());
}

/// 수동 확인용(실제 키체인·Anthropic): `cargo test -p ieum-desktop --test plan_usage -- --ignored`.
#[tokio::test]
#[ignore]
async fn plan_usage_live() {
    let c = PlanUsageClient::new(Arc::new(ieum_desktop_lib::plan_usage::keychain_token)).unwrap();
    let u = c.get().await;
    println!("state={} windows={}", u.state, u.windows.len());
}
