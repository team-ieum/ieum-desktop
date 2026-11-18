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

async fn expect_err(response: Vec<u8>) -> (String, String) {
    let (l, base) = bind().await;
    serve(l, move |_| response.clone());
    let r = GithubApi::with_base(&base)
        .call("t0k", Method::GET, "/x", &[], None)
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
            "이 저장소에 쓸 권한이 없어요.".into(),
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
        let (c, m) = expect_err(response).await;
        assert_eq!((c.as_str(), m), (code, message));
    }
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
        let (c, m) = expect_err(resp(status, "", SECRET.as_bytes())).await;
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
