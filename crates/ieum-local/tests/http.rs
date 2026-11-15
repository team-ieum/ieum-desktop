//! `http.request`. 외부 네트워크 없이 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_engine::{Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::HttpRunner;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
struct Req {
    method: String,
    path: String,
    head: String,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }
    fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

type Log = Arc<Mutex<Vec<Req>>>;

async fn read_request(sock: &mut TcpStream) -> Option<Req> {
    let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
    loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..i]).to_string();
        let len: usize = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse().unwrap())
            })
            .unwrap_or(0);
        while buf.len() < i + 4 + len {
            let n = sock.read(&mut tmp).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let mut first = head.lines().next().unwrap_or("").split(' ');
        let method = first.next().unwrap_or("").to_string();
        let path = first.next().unwrap_or("").to_string();
        return Some(Req {
            method,
            path,
            head,
            body: buf[i + 4..i + 4 + len].to_vec(),
        });
    }
}

async fn bind() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    (l, base)
}

/// 연결마다 요청 하나를 읽어 `respond`가 만든 응답을 보내고 닫는다.
fn serve(l: TcpListener, respond: impl Fn(&Req) -> Vec<u8> + Send + 'static) -> Log {
    let log: Log = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let Some(req) = read_request(&mut sock).await else {
                continue;
            };
            let res = respond(&req);
            seen.lock().unwrap().push(req);
            let _ = sock.write_all(&res).await;
            let _ = sock.shutdown().await;
        }
    });
    log
}

fn resp(status: u16, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut v = format!(
        "HTTP/1.1 {status} X\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    v.extend_from_slice(body);
    v
}

fn ok(body: &str) -> Vec<u8> {
    resp(200, "", body.as_bytes())
}

fn redirect(status: u16, location: &str) -> Vec<u8> {
    resp(status, &format!("location: {location}\r\n"), b"")
}

async fn serve_one(response: Vec<u8>) -> (String, Log) {
    let (l, base) = bind().await;
    (base, serve(l, move |_| response.clone()))
}

async fn run_with(
    config: Value,
    cancel: CancellationToken,
) -> Result<Map<String, Value>, NodeError> {
    let node: Node = serde_json::from_value(json!({"id": "n", "event": "http.request"})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    HttpRunner::new()
        .run(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            outputs: &Default::default(),
            events: &tx,
            cancel: &cancel,
        })
        .await
}

async fn run(config: Value) -> Result<Map<String, Value>, NodeError> {
    run_with(config, CancellationToken::new()).await
}

fn err(r: Result<Map<String, Value>, NodeError>) -> NodeError {
    r.expect_err("실패해야 함")
}

fn code(e: &NodeError) -> &str {
    e.code.as_deref().unwrap_or("")
}

// ── 본문 ─────────────────────────────────────────────

#[tokio::test]
async fn string_body_is_sent_as_json_string() {
    let (base, log) = serve_one(ok("{}")).await;
    run(json!({"method": "POST", "url": base, "body": "abc"}))
        .await
        .unwrap();
    let r = log.lock().unwrap()[0].clone();
    assert_eq!(r.body_str(), "\"abc\"");
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("application/json")
    );
}

#[tokio::test]
async fn object_body_is_json() {
    let (base, log) = serve_one(ok("{}")).await;
    run(json!({"method": "POST", "url": base, "body": {"a": 1}}))
        .await
        .unwrap();
    assert_eq!(log.lock().unwrap()[0].body_str(), r#"{"a":1}"#);
}

#[tokio::test]
async fn empty_string_body_is_sent() {
    let (base, log) = serve_one(ok("{}")).await;
    run(json!({"method": "PUT", "url": base, "body": ""}))
        .await
        .unwrap();
    let r = log.lock().unwrap()[0].clone();
    assert_eq!(r.method, "PUT");
    assert_eq!(r.body_str(), "\"\"");
}

/// BE `body_stringReferenceWithSpecialChars_staysValidJson`·`body_nestedValuesRenderedAndScalarsKept`
/// — 치환은 엔진 몫, 러너는 치환된 값을 깨지지 않게 직렬화한다.
#[tokio::test]
async fn nested_body_and_special_chars_stay_valid_json() {
    let (base, log) = serve_one(ok("{}")).await;
    let text = "그가 말했다 \"안녕\"\n다음 줄 \\ 경로 $1";
    let body = json!({"message": text, "tags": ["이음", "고정"], "retries": 3, "dryRun": false});
    run(json!({"method": "POST", "url": base, "body": body.clone()}))
        .await
        .unwrap();
    let sent: Value = serde_json::from_slice(&log.lock().unwrap()[0].body).unwrap();
    assert_eq!(sent, body);
}

#[tokio::test]
async fn null_body_sends_nothing_and_get_is_default() {
    let (base, log) = serve_one(ok("{}")).await;
    run(json!({"url": base, "body": null})).await.unwrap();
    let r = log.lock().unwrap()[0].clone();
    assert_eq!(r.method, "GET");
    assert!(r.body.is_empty());
}

#[tokio::test]
async fn user_header_overrides_content_type() {
    let (base, log) = serve_one(ok("{}")).await;
    run(json!({"url": base, "headers": {"content-type": "text/plain", "X-A": "1"}}))
        .await
        .unwrap();
    let r = log.lock().unwrap()[0].clone();
    assert_eq!(r.header("content-type").as_deref(), Some("text/plain"));
    assert_eq!(r.header("x-a").as_deref(), Some("1"));
}

#[tokio::test]
async fn output_shapes() {
    let cases: [(&str, Value); 6] = [
        ("", Value::Null),
        ("  \n", Value::Null),
        (r#"{"a":1}"#, json!({"a": 1})),
        ("[1]", json!("[1]")),
        ("hi", json!("hi")),
        ("123", json!("123")),
    ];
    for (body, want) in cases {
        let (base, _) = serve_one(ok(body)).await;
        let out = run(json!({"url": base})).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out["statusCode"], json!(200));
        assert_eq!(out["body"], want, "본문 {body:?}");
    }
}

// ── 리다이렉트 ─────────────────────────────────────────

#[tokio::test]
async fn redirect_followed_relative_to_current() {
    let (l, base) = bind().await;
    let log = serve(l, |r| match r.path.as_str() {
        "/" => redirect(302, "/next"),
        _ => ok(r#"{"ok":true}"#),
    });
    let out = run(json!({"url": base})).await.unwrap();
    assert_eq!(out["statusCode"], json!(200));
    assert_eq!(out["body"], json!({"ok": true}));
    assert_eq!(log.lock().unwrap()[1].path, "/next");
}

#[tokio::test]
async fn redirect_303_becomes_get_and_drops_body() {
    let (l, base) = bind().await;
    let log = serve(l, |r| match r.path.as_str() {
        "/" => redirect(303, "/done"),
        _ => ok("{}"),
    });
    run(json!({"method": "POST", "url": base, "body": {"a": 1}}))
        .await
        .unwrap();
    let hop = log.lock().unwrap()[1].clone();
    assert_eq!((hop.method.as_str(), hop.path.as_str()), ("GET", "/done"));
    assert!(hop.body.is_empty());
}

#[tokio::test]
async fn redirect_307_keeps_method_and_body() {
    let (l, base) = bind().await;
    let log = serve(l, |r| match r.path.as_str() {
        "/" => redirect(307, "/again"),
        _ => ok("{}"),
    });
    run(json!({"method": "POST", "url": base, "body": {"a": 1}}))
        .await
        .unwrap();
    let hop = log.lock().unwrap()[1].clone();
    assert_eq!((hop.method.as_str(), hop.path.as_str()), ("POST", "/again"));
    assert_eq!(hop.body_str(), r#"{"a":1}"#);
}

#[tokio::test]
async fn exactly_three_redirects_succeed() {
    let (l, base) = bind().await;
    serve(l, |r| match r.path.as_str() {
        "/" => redirect(302, "/a"),
        "/a" => redirect(301, "/b"),
        "/b" => redirect(308, "/c"),
        _ => ok("{}"),
    });
    assert_eq!(
        run(json!({"url": base})).await.unwrap()["statusCode"],
        json!(200)
    );
}

#[tokio::test]
async fn fourth_redirect_fails() {
    let (l, base) = bind().await;
    let log = serve(l, |r| match r.path.as_str() {
        "/" => redirect(302, "/a"),
        "/a" => redirect(302, "/b"),
        "/b" => redirect(302, "/c"),
        _ => redirect(302, "/d"),
    });
    let e = err(run(json!({"url": base})).await);
    assert_eq!(
        e.message,
        "리다이렉트가 3회를 넘었습니다 — 마지막 Location: /d"
    );
    assert_eq!(code(&e), "HTTP_UNREACHABLE");
    assert_eq!(log.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn cross_origin_redirect_drops_auth_and_never_restores() {
    let (la, a) = bind().await;
    let (lb, b) = bind().await;
    let a2 = a.clone();
    let log_b = serve(lb, move |_| redirect(302, &format!("{a2}/second")));
    let log_a = serve(la, move |r| match r.path.as_str() {
        "/" => redirect(302, &format!("{b}/x")),
        _ => ok("{}"),
    });
    let headers = json!({
        "authorization": "Bearer secret",
        "Cookie": "sid=1",
        "Proxy-Authorization": "Basic x",
        "X-Custom": "keep",
    });
    run(json!({"url": a, "headers": headers})).await.unwrap();
    let first = log_a.lock().unwrap()[0].clone();
    assert_eq!(
        first.header("authorization").as_deref(),
        Some("Bearer secret")
    );
    for r in [
        log_b.lock().unwrap()[0].clone(),
        log_a.lock().unwrap()[1].clone(),
    ] {
        assert_eq!(r.header("authorization"), None, "{}", r.path);
        assert_eq!(r.header("cookie"), None);
        assert_eq!(r.header("proxy-authorization"), None);
        assert_eq!(r.header("x-custom").as_deref(), Some("keep"));
    }
}

#[tokio::test]
async fn same_origin_redirect_keeps_auth() {
    let (l, base) = bind().await;
    let log = serve(l, |r| match r.path.as_str() {
        "/" => redirect(302, "/next"),
        _ => ok("{}"),
    });
    let headers = json!({"Authorization": "Bearer secret", "Cookie": "sid=1"});
    run(json!({"url": base, "headers": headers})).await.unwrap();
    let hop = log.lock().unwrap()[1].clone();
    assert_eq!(
        hop.header("authorization").as_deref(),
        Some("Bearer secret")
    );
    assert_eq!(hop.header("cookie").as_deref(), Some("sid=1"));
}

/// BE `redirect_portChange_stripsCredentials` — 같은 호스트라도 포트가 다르면 다른 출처.
#[tokio::test]
async fn port_change_drops_auth() {
    let (lb, b) = bind().await;
    let log_b = serve(lb, |_| ok("{}"));
    let (base, _) = serve_one(redirect(302, &format!("{b}/next"))).await;
    run(json!({"url": base, "headers": {"Authorization": "Bearer secret"}}))
        .await
        .unwrap();
    assert_eq!(log_b.lock().unwrap()[0].header("authorization"), None);
}

#[tokio::test]
async fn redirect_without_location_succeeds() {
    let (base, _) = serve_one(resp(302, "", b"")).await;
    let out = run(json!({"url": base})).await.unwrap();
    assert_eq!(out["statusCode"], json!(302));
    assert_eq!(out["body"], Value::Null);
}

#[tokio::test]
async fn created_with_location_is_not_followed() {
    let (base, log) = serve_one(resp(201, "location: /created/1\r\n", b"{}")).await;
    let out = run(json!({"url": base})).await.unwrap();
    assert_eq!(out["statusCode"], json!(201));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn encoded_location_is_not_double_encoded() {
    let (l, base) = bind().await;
    let log = serve(l, |r| match r.path.as_str() {
        "/" => redirect(302, "/login?sig=a%2Fb%3D&q=a%20b"),
        _ => ok("{}"),
    });
    run(json!({"url": base})).await.unwrap();
    assert_eq!(log.lock().unwrap()[1].path, "/login?sig=a%2Fb%3D&q=a%20b");
}

#[tokio::test]
async fn scheme_relative_location_is_followed() {
    let (lb, b) = bind().await;
    let log_b = serve(lb, |_| ok("{}"));
    let host = b.trim_start_matches("http:").to_string();
    let (base, _) = serve_one(redirect(302, &format!("{host}/next"))).await;
    run(json!({"url": base})).await.unwrap();
    assert_eq!(log_b.lock().unwrap()[0].path, "/next");
}

#[tokio::test]
async fn non_http_location_fails_url_check() {
    for loc in ["file:///etc/passwd", "data:text/plain,hi", "ftp://x/y"] {
        let (base, log) = serve_one(redirect(302, loc)).await;
        let e = err(run(json!({"url": base})).await);
        assert_eq!(e.message, format!("http·https URL만 허용됩니다: {loc}"));
        assert_eq!(code(&e), "HTTP_UNREACHABLE");
        assert_eq!(log.lock().unwrap().len(), 1);
    }
}

// ── 상태·주소·칸 ───────────────────────────────────────

#[tokio::test]
async fn status_404_fails() {
    let (base, _) = serve_one(resp(404, "", b"not here")).await;
    let e = err(run(json!({"url": base})).await);
    assert_eq!(e.message, "HTTP 404: not here");
    assert_eq!(code(&e), "HTTP_STATUS 404");
}

#[tokio::test]
async fn error_body_in_message_is_capped_at_2000_utf16_units() {
    // 앞 1999자 + 한 글자에 UTF-16 두 단위인 이모지 → 2000을 넘으므로 이모지를 자르지 않고 뺀다.
    let body = format!("{}😀{}", "a".repeat(1999), "b".repeat(5000));
    let (base, _) = serve_one(resp(500, "", body.as_bytes())).await;
    let e = err(run(json!({"url": base})).await);
    assert_eq!(e.message, format!("HTTP 500: {}…", "a".repeat(1999)));
    assert_eq!(code(&e), "HTTP_STATUS 500");

    let exact = "가".repeat(2000);
    let (base, _) = serve_one(resp(503, "", exact.as_bytes())).await;
    let e = err(run(json!({"url": base})).await);
    assert_eq!(e.message, format!("HTTP 503: {exact}"));
}

#[tokio::test]
async fn localhost_and_private_are_allowed() {
    let (base, log) = serve_one(ok(r#"{"x":1}"#)).await;
    let local = base.replace("127.0.0.1", "localhost");
    for url in [base, local] {
        let out = run(json!({"url": url})).await.unwrap();
        assert_eq!(out["body"], json!({"x": 1}));
    }
    assert_eq!(log.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn bad_scheme_and_method() {
    let e = err(run(json!({"url": "ftp://x"})).await);
    assert_eq!(e.message, "http·https URL만 허용됩니다: ftp://x");
    assert_eq!(code(&e), "FIELD_INVALID url");

    let e = err(run(json!({"url": "http://127.0.0.1:1/", "method": "patch"})).await);
    assert_eq!(e.message, "지원하지 않는 HTTP 메서드: PATCH");
    assert_eq!(code(&e), "FIELD_INVALID method");

    let e = err(run(json!({"url": "http://"})).await);
    assert_eq!(e.message, "URL에 호스트가 없습니다: http://");
}

#[tokio::test]
async fn upper_case_scheme_and_method_are_allowed() {
    let (base, log) = serve_one(ok("{}")).await;
    run(json!({"url": base.replace("http", "HTTP"), "method": "delete"}))
        .await
        .unwrap();
    assert_eq!(log.lock().unwrap()[0].method, "DELETE");
}

#[tokio::test]
async fn empty_url_fails() {
    for cfg in [json!({}), json!({"url": "  "})] {
        let e = err(run(cfg).await);
        assert_eq!(e.message, "'URL' 칸이 비어 있어요");
        assert_eq!(code(&e), "FIELD_EMPTY url");
    }
}

#[tokio::test]
async fn webhook_credential_is_refused() {
    let (base, log) = serve_one(ok("{}")).await;
    let e = err(run(json!({"url": base, "webhookCredentialId": "abc"})).await);
    assert_eq!(
        e.message,
        "저장된 웹훅 주소는 데스크톱에서 쓸 수 없어요. URL 칸에 주소를 직접 넣어 주세요."
    );
    assert_eq!(code(&e), "FIELD_INVALID webhookCredentialId");
    assert!(log.lock().unwrap().is_empty());
    // 빈 값은 없는 것과 같다.
    run(json!({"url": base, "webhookCredentialId": " "}))
        .await
        .unwrap();
}

// ── 크기·인코딩·취소·보안 ───────────────────────────────

#[tokio::test]
async fn body_over_1mb_is_truncated_not_failed() {
    let big = vec![b'x'; 2 * 1024 * 1024];
    let (base, _) = serve_one(resp(200, "", &big)).await;
    let out = run(json!({"url": base})).await.unwrap();
    assert_eq!(out["body"].as_str().unwrap().len(), 1_048_576);
}

/// Content-Length를 믿지 않는다 — 길이를 숨긴(chunked 없이 닫을 때까지) 응답도 1MB에서 끊는다.
#[tokio::test]
async fn body_without_length_is_capped_by_counting() {
    let mut raw = b"HTTP/1.1 200 X\r\nconnection: close\r\n\r\n".to_vec();
    raw.extend(vec![b'y'; 3 * 1024 * 1024]);
    let (base, _) = serve_one(raw).await;
    let out = run(json!({"url": base})).await.unwrap();
    assert_eq!(out["body"].as_str().unwrap().len(), 1_048_576);
}

#[tokio::test]
async fn gzip_body_is_not_decompressed() {
    let gz = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
    let (base, _) = serve_one(resp(200, "content-encoding: gzip\r\n", &gz)).await;
    let out = run(json!({"url": base})).await.unwrap();
    // 풀지 않은 바이트가 손실 변환된 글로 나온다.
    assert!(out["body"].as_str().unwrap().contains('\u{FFFD}'));
}

#[tokio::test]
async fn non_utf8_body_is_lossy_string() {
    let bytes = [0xFF, 0xFE, b'a', 0xE2, 0x82];
    let (base, _) = serve_one(resp(200, "", &bytes)).await;
    let out = run(json!({"url": base})).await.unwrap();
    assert_eq!(out["body"], json!("\u{FFFD}\u{FFFD}a\u{FFFD}"));
}

#[tokio::test]
async fn cancel_stops_request() {
    let (l, base) = bind().await;
    // 받기만 하고 응답하지 않는다.
    tokio::spawn(async move {
        let mut socks = Vec::new();
        loop {
            let (s, _) = l.accept().await.unwrap();
            socks.push(s);
        }
    });
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c.cancel();
    });
    let e = tokio::time::timeout(
        Duration::from_secs(5),
        run_with(json!({"url": base}), cancel),
    )
    .await
    .expect("취소 뒤 곧 끝나야 함")
    .expect_err("실패해야 함");
    assert_eq!(e.message, "취소됐어요.");
}

#[tokio::test]
async fn unreachable_message_has_no_address() {
    // 바로 닫아 포트를 비운다 — 연결 거부.
    let (l, base) = bind().await;
    drop(l);
    let e = err(run(json!({"url": format!("{base}/secret-path")})).await);
    assert_eq!(e.message, "요청을 보내지 못했어요: 연결 실패");
    assert_eq!(code(&e), "HTTP_UNREACHABLE");
}

#[tokio::test]
async fn error_never_contains_header_values() {
    let (base, _) = serve_one(resp(404, "", b"nope")).await;
    let e = err(run(json!({
        "method": "POST",
        "url": base,
        "headers": {"Authorization": "Bearer SECRET"},
        "body": {"token": "BODYSECRET"},
    }))
    .await);
    assert!(!e.message.contains("SECRET"), "{}", e.message);
    assert!(!format!("{e:?}").contains("SECRET"));

    // 헤더 값이 잘못돼도 값은 싣지 않는다.
    let e = err(run(json!({"url": base, "headers": {"Authorization": "Bearer SECRET\n"}})).await);
    assert!(!e.message.contains("SECRET"), "{}", e.message);
    assert_eq!(code(&e), "FIELD_INVALID headers");
}
