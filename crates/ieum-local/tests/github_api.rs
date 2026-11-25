//! GitHub 호출 클라이언트. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

mod github_support;

use std::time::Duration;

use github_support::{bind, hang, resp, serve};
use ieum_local::github::GithubApi;
use reqwest::Method;
use serde_json::json;

const SECRET: &str = r#"{"message":"SECRET-ECHO t0k"}"#;

#[tokio::test]
async fn sends_fixed_headers() {
    let (l, base) = bind().await;
    let log = serve(l, |_| resp(200, "", b"{\"ok\":true}"));
    let api = GithubApi::with_base(&base);
    let q = [
        ("state", "open".to_string()),
        ("per_page", "20".to_string()),
    ];
    let r = api
        .call("t0k", Method::GET, "/repos/o/r/issues", &q, None)
        .await
        .unwrap();
    assert_eq!(r.expect(200).unwrap(), json!({"ok": true}));
    let b = json!({"title": "제목"});
    api.call("t0k", Method::POST, "/repos/o/r/issues", &[], Some(&b))
        .await
        .unwrap();
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 2);
    let g = &log[0];
    assert_eq!(g.method, "GET");
    assert_eq!(g.path, "/repos/o/r/issues?state=open&per_page=20");
    assert_eq!(g.header("authorization").unwrap(), "Bearer t0k");
    assert_eq!(g.header("accept").unwrap(), "application/vnd.github+json");
    assert_eq!(g.header("x-github-api-version").unwrap(), "2022-11-28");
    assert_eq!(g.header("user-agent").unwrap(), "ieum-desktop");
    let p = &log[1];
    assert_eq!(p.method, "POST");
    assert_eq!(p.path, "/repos/o/r/issues");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&p.body).unwrap(),
        b
    );
}

async fn expect_err(method: Method, response: Vec<u8>) -> (String, String) {
    let (l, base) = bind().await;
    serve(l, move |_| response.clone());
    let r = GithubApi::with_base(&base)
        .call("t0k", method, "/x", &[], None)
        .await
        .unwrap();
    let e = r.expect(200).unwrap_err();
    (e.code.unwrap(), e.message)
}

#[tokio::test]
async fn status_table() {
    let auth = "GitHub 로그인이 만료됐어요. 설정 > 연동에서 다시 연결해 주세요.";
    let rate = "GitHub 요청 한도에 닿았어요. 잠시 뒤 다시 실행해 주세요.";
    let cases: Vec<(Vec<u8>, &str, String)> = vec![
        (resp(401, "", b""), "GITHUB_AUTH", auth.into()),
        (
            resp(403, "", b""),
            "GITHUB_FORBIDDEN",
            "이 저장소·조직을 읽을 권한이 없어요.".into(),
        ),
        (
            resp(403, "x-ratelimit-remaining: 0\r\n", b""),
            "GITHUB_RATE_LIMIT",
            rate.into(),
        ),
        (
            resp(429, "retry-after: 3\r\n", b""),
            "GITHUB_RATE_LIMIT",
            rate.into(),
        ),
        (
            resp(404, "", b""),
            "GITHUB_NOT_FOUND",
            "저장소나 이슈를 찾지 못했어요. 이름과 권한을 확인해 주세요.".into(),
        ),
        (
            resp(301, "", b""),
            "GITHUB_MOVED",
            "저장소 주소가 바뀌었어요. 소유자·저장소 이름을 확인해 주세요.".into(),
        ),
        (
            resp(410, "", b""),
            "GITHUB_GONE",
            "이 저장소는 이슈를 쓰지 않아요.".into(),
        ),
        (
            resp(422, "", b""),
            "GITHUB_INVALID",
            "GitHub가 입력값을 받지 않았어요. 칸 값을 확인해 주세요.".into(),
        ),
        (
            resp(500, "", b""),
            "GITHUB_UNREACHABLE",
            "GitHub에 닿지 못했어요. 잠시 뒤 다시 실행해 주세요.".into(),
        ),
        (
            resp(418, "", b""),
            "GITHUB_STATUS 418",
            "GitHub가 요청을 거절했어요 (HTTP 418).".into(),
        ),
    ];
    for (response, code, message) in cases {
        let (c, m) = expect_err(Method::GET, response).await;
        assert_eq!((c.as_str(), m), (code, message));
    }
    let (c, m) = expect_err(Method::POST, resp(403, "", b"")).await;
    assert_eq!(
        (c.as_str(), m.as_str()),
        ("GITHUB_FORBIDDEN", "이 저장소에 쓸 권한이 없어요.")
    );
}

#[tokio::test]
async fn redirect_not_followed() {
    let (l, base) = bind().await;
    let log = serve(l, |_| resp(301, "location: /elsewhere\r\n", b""));
    let r = GithubApi::with_base(&base)
        .call("t0k", Method::GET, "/x", &[], None)
        .await
        .unwrap();
    assert_eq!(r.expect(200).unwrap_err().code.unwrap(), "GITHUB_MOVED");
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn error_message_never_contains_body() {
    for status in [401, 422] {
        let (c, m) = expect_err(Method::GET, resp(status, "", SECRET.as_bytes())).await;
        for s in [&c, &m] {
            assert!(!s.contains("SECRET") && !s.contains("t0k"), "{s}");
        }
    }
}

#[tokio::test]
async fn too_large_fails() {
    let (l, base) = bind().await;
    serve(l, |_| resp(200, "", &vec![b'a'; 2048]));
    let e = GithubApi::with_base(&base)
        .with_limits(1024, Duration::from_secs(30))
        .call("t0k", Method::GET, "/x", &[], None)
        .await
        .err()
        .unwrap();
    assert_eq!(e.code.unwrap(), "GITHUB_TOO_LARGE");
}

#[tokio::test]
async fn timeout_is_unreachable() {
    let (l, base) = bind().await;
    hang(l);
    let e = GithubApi::with_base(&base)
        .with_limits(1024, Duration::from_millis(200))
        .call("t0k", Method::GET, "/x", &[], None)
        .await
        .err()
        .unwrap();
    assert_eq!(e.code.unwrap(), "GITHUB_UNREACHABLE");
}

#[tokio::test]
async fn get_if_sends_etag_and_reads_headers() {
    let (l, base) = bind().await;
    let log = serve(l, |r| {
        assert_eq!(r.header("if-none-match").as_deref(), Some("\"abc\""));
        resp(
            304,
            "etag: \"abc\"\r\nx-ratelimit-remaining: 4999\r\nx-ratelimit-limit: 5000\r\nx-ratelimit-reset: 1800000000\r\n",
            b"",
        )
    });
    let r = GithubApi::with_base(&base)
        .get_if("t0k", "/x", &[], Some("\"abc\""))
        .await
        .unwrap();
    assert_eq!(
        log.lock().unwrap()[0].header("if-none-match").unwrap(),
        "\"abc\""
    );
    assert_eq!(r.status, 304);
    assert_eq!(r.body, serde_json::Value::Null);
    assert_eq!(r.etag.as_deref(), Some("\"abc\""));
    assert_eq!(r.remaining, Some(4999));
    assert_eq!(r.limit, Some(5000));
    assert_eq!(r.reset_at, Some(1800000000));
}

#[tokio::test]
async fn get_if_without_etag_omits_header() {
    let (l, base) = bind().await;
    let log = serve(l, |_| {
        resp(200, "x-ratelimit-remaining: zz\r\n", b"{\"a\":1}")
    });
    let r = GithubApi::with_base(&base)
        .get_if("t0k", "/x", &[], None)
        .await
        .unwrap();
    assert!(log.lock().unwrap()[0].header("if-none-match").is_none());
    assert_eq!(r.status, 200);
    assert_eq!(r.body, json!({"a": 1}));
    assert_eq!(r.remaining, None);
    assert_eq!(r.etag, None);
}

/// `retry-after`가 음수면 지금으로, 너무 크면 넘치지 않고 끝값으로.
#[tokio::test]
async fn get_if_retry_after_clamped() {
    for (retry, check) in [
        (
            "-100",
            (|r: i64, now: i64| r >= now && r <= now + 5) as fn(i64, i64) -> bool,
        ),
        ("9223372036854775807", |r, _| r == i64::MAX),
    ] {
        let (l, base) = bind().await;
        let header = format!("retry-after: {retry}\r\n");
        serve(l, move |_| resp(429, &header, b""));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let r = GithubApi::with_base(&base)
            .get_if("t0k", "/x", &[], None)
            .await
            .unwrap();
        let reset = r.reset_at.unwrap();
        assert!(check(reset, now), "{retry}: {reset}");
    }
}
