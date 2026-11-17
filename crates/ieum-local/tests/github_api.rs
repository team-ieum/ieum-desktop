//! GitHub 호출 클라이언트. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_local::github::GithubApi;
use reqwest::Method;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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
    // 연결만 받고 응답하지 않는다.
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            held.push(l.accept().await.unwrap());
        }
    });
    let e = GithubApi::with_base(&base)
        .with_limits(1024, Duration::from_millis(200))
        .call("t0k", Method::GET, "/x", &[], None)
        .await
        .err()
        .unwrap();
    assert_eq!(e.code.unwrap(), "GITHUB_UNREACHABLE");
}
