//! 회원가입 요청(인증번호 발송·확인·가입). 가짜 HTTP 서버와 가짜 키 저장소만 쓴다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ieum_desktop_lib::web::{BeClient, WebError};
use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::KeyError;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PASSWORD: &str = "Pw1!secret";

#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl KeyStore for FakeKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().insert(account.into(), key.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct Req {
    line: String,
    auth: Option<String>,
    body: Value,
}

type Log = Arc<Mutex<Vec<Req>>>;

async fn read_request(sock: &mut TcpStream) -> Req {
    let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
    loop {
        let n = sock.read(&mut tmp).await.unwrap();
        buf.extend_from_slice(&tmp[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let head = &text[..end];
            let len = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if buf.len() >= end + 4 + len || n == 0 {
                let line = head.lines().next().unwrap_or("");
                let line = line.rsplit_once(' ').map_or(line, |(l, _)| l).to_string();
                let auth = head.lines().find_map(|l| {
                    l.to_ascii_lowercase()
                        .starts_with("authorization:")
                        .then(|| l["authorization:".len()..].trim().to_string())
                });
                let body = serde_json::from_slice(&buf[end + 4..]).unwrap_or(Value::Null);
                return Req { line, auth, body };
            }
        }
    }
}

async fn serve(route: impl Fn(&Req) -> (u16, Value) + Send + Sync + 'static) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let (seen, route) = (seen.clone(), route.clone());
            tokio::spawn(async move {
                let req = read_request(&mut sock).await;
                let (status, body) = route(&req);
                seen.lock().unwrap().push(req);
                let body = body.to_string();
                let r = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(r.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (base, log)
}

fn client(base: &str) -> BeClient {
    BeClient::new(base, Arc::new(FakeKeys::default())).unwrap()
}

fn fail(code: &str) -> Value {
    json!({"success": false, "data": null, "message": format!("echo {PASSWORD}"), "code": code})
}

fn assert_clean(err: &WebError, secrets: &[&str]) {
    let shown = format!("{err} {err:?} {}", serde_json::to_string(err).unwrap());
    for s in secrets {
        assert!(!shown.contains(s), "비밀이 에러에 섞임: {shown}");
    }
}

#[tokio::test]
async fn signup_requests_have_exact_bodies() {
    // 2xx면 data는 보지 않는다(null이어도 성공).
    let (base, log) = serve(|_| (200, json!({"success": true, "data": null}))).await;
    let c = client(&base);
    c.send_signup_code("a@b.c").await.unwrap();
    c.verify_signup_code("a@b.c", "482913").await.unwrap();
    c.register("a@b.c", PASSWORD, "홍길동").await.unwrap();

    let got = log.lock().unwrap().clone();
    let pairs: Vec<(&str, Value)> = got
        .iter()
        .map(|r| (r.line.as_str(), r.body.clone()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (
                "POST /api/v1/auth/email/send-code",
                json!({"email": "a@b.c", "purpose": "SIGNUP"})
            ),
            (
                "POST /api/v1/auth/email/verify-code",
                json!({"email": "a@b.c", "code": "482913", "purpose": "SIGNUP"})
            ),
            (
                "POST /api/v1/auth/register",
                json!({"email": "a@b.c", "password": PASSWORD, "name": "홍길동"})
            ),
        ]
    );
    assert!(got.iter().all(|r| r.auth.is_none()), "{got:?}");
}

#[tokio::test]
async fn register_errors_map_to_codes_and_stay_clean() {
    let cases: &[(u16, &str, &str)] = &[
        (400, "EMAIL_ALREADY_EXISTS", "EMAIL_TAKEN"),
        (409, "EMAIL_ALREADY_EXISTS", "EMAIL_TAKEN"),
        (400, "VERIFICATION_CODE_INVALID", "CODE_INVALID"),
        (400, "VERIFICATION_CODE_EXPIRED", "CODE_INVALID"),
        (400, "VERIFICATION_ATTEMPTS_EXCEEDED", "CODE_INVALID"),
        (400, "EMAIL_NOT_VERIFIED", "CODE_INVALID"),
        (400, "INVALID_INPUT", "BAD_INPUT"),
        (429, "VERIFICATION_RESEND_TOO_SOON", "AUTH_LOCKED"),
        (500, "INTERNAL_SERVER_ERROR", "SERVER"),
    ];
    for &(status, be, want) in cases {
        let (base, _) = serve(move |_| (status, fail(be))).await;
        let err = client(&base)
            .register("a@b.c", PASSWORD, "홍길동")
            .await
            .unwrap_err();
        assert_eq!(err.code(), want, "{status} {be}");
        assert_eq!(serde_json::to_value(&err).unwrap()["code"], want);
        assert_clean(&err, &[PASSWORD]);
    }
    // 본문이 없거나 JSON이 아닌 4xx도 BAD_INPUT.
    let (base, _) = serve(|_| (400, Value::Null)).await;
    let err = client(&base)
        .verify_signup_code("a@b.c", "1")
        .await
        .unwrap_err();
    assert_eq!(err.code(), "BAD_INPUT");

    let err = client("http://127.0.0.1:1")
        .register("a@b.c", PASSWORD, "홍길동")
        .await
        .unwrap_err();
    assert_eq!(err.code(), "NETWORK");
    assert_clean(&err, &[PASSWORD]);
}
