//! 이음 계정 로그인·BE 클라이언트. 가짜 HTTP 서버와 가짜 키 저장소만 쓴다(실제 키체인·BE 금지).

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use ieum_desktop_lib::store::{Settings, Store};
use ieum_desktop_lib::web::{
    BeClient, SESSION_ACCOUNT, Session, WebError, save_settings, validate_server_url,
};
use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::KeyError;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PASSWORD: &str = "pw-secret-9";

/// 메모리 키 저장소.
#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl FakeKeys {
    fn session(&self) -> Option<Value> {
        let m = self.0.lock().unwrap();
        m.get(SESSION_ACCOUNT)
            .map(|s| serde_json::from_str(s).unwrap())
    }
}

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
    /// `POST /api/v1/auth/login`
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

fn http(status: u16, body: &Value) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn ok(data: Value) -> (u16, Value) {
    (
        200,
        json!({"success": true, "data": data, "message": "OK", "code": "OK"}),
    )
}

fn tokens(access: &str, refresh: &str) -> Value {
    json!({"accessToken": access, "refreshToken": refresh, "expiresIn": 3600})
}

/// 요청마다 `route`가 고른 응답을 보내는 가짜 서버.
async fn serve(route: impl Fn(&Req) -> (u16, Value) + Send + Sync + 'static) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let req = read_request(&mut sock).await;
            let (status, body) = route(&req);
            seen.lock().unwrap().push(req);
            let _ = sock.write_all(http(status, &body).as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    (base, log)
}

fn put_session(keys: &FakeKeys, server: &str, access: &str, refresh: &str) {
    let s = Session {
        email: "a@b.c".into(),
        server_url: server.into(),
        access_token: access.into(),
        refresh_token: refresh.into(),
    };
    keys.set(SESSION_ACCOUNT, &serde_json::to_string(&s).unwrap())
        .unwrap();
}

fn lines(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|r| r.line.clone()).collect()
}

#[tokio::test]
async fn login_stores_session() {
    let (base, log) = serve(|r| match r.line.as_str() {
        "POST /api/v1/auth/login" => ok(tokens("acc-1", "ref-1")),
        _ => (404, json!({})),
    })
    .await;
    let keys = Arc::new(FakeKeys::default());
    let client = BeClient::new(&base, keys.clone()).unwrap();

    let acc = client.login("a@b.c", PASSWORD).await.unwrap();
    assert_eq!(acc.email, "a@b.c");
    assert_eq!(acc.server_url, base);

    let req = log.lock().unwrap()[0].clone();
    assert_eq!(req.body, json!({"email": "a@b.c", "password": PASSWORD}));

    let s = keys.session().unwrap();
    assert_eq!(s["accessToken"], "acc-1");
    assert_eq!(s["refreshToken"], "ref-1");
    assert_eq!(s["email"], "a@b.c");
    // 비밀번호는 어디에도 저장하지 않는다.
    assert!(
        !keys
            .0
            .lock()
            .unwrap()
            .values()
            .any(|v| v.contains(PASSWORD))
    );
    assert_eq!(client.account().unwrap().email, "a@b.c");
}

#[tokio::test]
async fn expired_access_refreshes_once() {
    let (base, log) = serve(|r| match (r.line.as_str(), r.auth.as_deref()) {
        ("GET /api/v1/workflows", Some("Bearer old")) => (401, json!({"success": false})),
        ("GET /api/v1/workflows", Some("Bearer acc-2")) => ok(json!([1, 2])),
        ("POST /api/v1/auth/refresh", _) if r.body == json!({"refreshToken": "ref-1"}) => {
            ok(tokens("acc-2", "ref-2"))
        }
        _ => (400, json!({})),
    })
    .await;
    let keys = Arc::new(FakeKeys::default());
    put_session(&keys, &base, "old", "ref-1");
    let client = BeClient::new(&base, keys.clone()).unwrap();

    let got: Vec<i32> = client.get("/api/v1/workflows").await.unwrap();
    assert_eq!(got, vec![1, 2]);
    assert_eq!(
        lines(&log),
        [
            "GET /api/v1/workflows",
            "POST /api/v1/auth/refresh",
            "GET /api/v1/workflows"
        ]
    );
    let s = keys.session().unwrap();
    assert_eq!(
        (s["accessToken"].as_str(), s["refreshToken"].as_str()),
        (Some("acc-2"), Some("ref-2"))
    );
}

#[tokio::test]
async fn refresh_failure_clears_session() {
    // 갱신이 거절되면 세션을 지운다.
    let (base, log) = serve(|r| match r.line.as_str() {
        "POST /api/v1/auth/refresh" => (401, json!({"success": false})),
        _ => (401, json!({"success": false})),
    })
    .await;
    let keys = Arc::new(FakeKeys::default());
    put_session(&keys, &base, "old", "ref-1");
    let client = BeClient::new(&base, keys.clone()).unwrap();
    let err = client.get::<Value>("/api/v1/workflows").await.unwrap_err();
    assert!(matches!(err, WebError::AuthExpired), "{err:?}");
    assert!(keys.session().is_none());
    assert_eq!(lines(&log).len(), 2);

    // 갱신은 됐는데 다시 401이면 한 번만 갱신하고 세션을 지운다.
    let (base, log) = serve(|r| match r.line.as_str() {
        "POST /api/v1/auth/refresh" => ok(tokens("acc-2", "ref-2")),
        _ => (401, json!({"success": false})),
    })
    .await;
    put_session(&keys, &base, "old", "ref-1");
    let client = BeClient::new(&base, keys.clone()).unwrap();
    let err = client.get::<Value>("/api/v1/workflows").await.unwrap_err();
    assert!(matches!(err, WebError::AuthExpired), "{err:?}");
    assert!(keys.session().is_none());
    assert_eq!(lines(&log).len(), 3);
}

#[tokio::test]
async fn logout_clears_even_if_server_fails() {
    let (base, log) = serve(|_| (500, json!({"success": false}))).await;
    let keys = Arc::new(FakeKeys::default());
    put_session(&keys, &base, "acc", "ref-1");
    let client = BeClient::new(&base, keys.clone()).unwrap();
    client.logout().await.unwrap();
    assert!(keys.session().is_none());
    assert!(client.account().is_none());
    let req = log.lock().unwrap()[0].clone();
    assert_eq!(req.line, "POST /api/v1/auth/logout");
    assert_eq!(req.body, json!({"refreshToken": "ref-1"}));

    // 서버에 닿지 않아도 지운다.
    put_session(&keys, "http://127.0.0.1:1", "acc", "ref-1");
    let client = BeClient::new("http://127.0.0.1:1", keys.clone()).unwrap();
    client.logout().await.unwrap();
    assert!(keys.session().is_none());
}

fn assert_clean(err: &WebError, secrets: &[&str]) {
    let shown = format!("{err} {err:?} {}", serde_json::to_string(err).unwrap());
    for s in secrets {
        assert!(!shown.contains(s), "비밀이 에러에 섞임: {shown}");
    }
}

#[tokio::test]
async fn errors_never_contain_tokens() {
    // 서버가 받은 값을 메시지에 되돌려 줘도 에러에는 나가지 않는다.
    let (base, _) = serve(|r| {
        let echo = format!(
            "{} {} {}",
            r.auth.clone().unwrap_or_default(),
            r.body,
            r.line
        );
        let status = if r.line.ends_with("/login") { 401 } else { 500 };
        (status, json!({"success": false, "message": echo}))
    })
    .await;
    let keys = Arc::new(FakeKeys::default());
    let client = BeClient::new(&base, keys.clone()).unwrap();

    let err = client.login("a@b.c", PASSWORD).await.unwrap_err();
    assert!(matches!(err, WebError::AuthFailed), "{err:?}");
    assert_eq!(err.code(), "AUTH_FAILED");
    assert_clean(&err, &[PASSWORD]);

    put_session(&keys, &base, "acc-secret", "ref-secret");
    let err = client.get::<Value>("/api/v1/workflows").await.unwrap_err();
    assert!(matches!(err, WebError::Server(_)), "{err:?}");
    assert_eq!(err.code(), "SERVER");
    assert_clean(&err, &["acc-secret", "ref-secret"]);

    let client = BeClient::new("http://127.0.0.1:1", keys.clone()).unwrap();
    put_session(&keys, "http://127.0.0.1:1", "acc-secret", "ref-secret");
    let err = client.get::<Value>("/api/v1/workflows").await.unwrap_err();
    assert_eq!(err.code(), "NETWORK");
    assert_clean(&err, &["acc-secret", "ref-secret"]);
    assert_eq!(WebError::AuthExpired.code(), "AUTH_EXPIRED");
}

#[tokio::test]
async fn rejects_oversized_response_and_bad_path() {
    let (base, log) = serve(|_| ok(json!("x".repeat(1024 * 1024 + 1)))).await;
    let keys = Arc::new(FakeKeys::default());
    put_session(&keys, &base, "acc", "ref");
    let client = BeClient::new(&base, keys.clone()).unwrap();
    let err = client.get::<String>("/api/v1/workflows").await.unwrap_err();
    assert_eq!(err.code(), "SERVER");
    // 경로가 `/`로 시작하지 않으면 보내지 않는다(호스트 바꿔치기 차단).
    assert!(client.get::<Value>("@evil.example/x").await.is_err());
    assert_eq!(lines(&log).len(), 1);
}

#[test]
fn server_url_validation() {
    assert_eq!(
        validate_server_url("https://green.kiacms.co.kr").unwrap(),
        "https://green.kiacms.co.kr"
    );
    assert_eq!(
        validate_server_url("https://green.kiacms.co.kr/").unwrap(),
        "https://green.kiacms.co.kr"
    );
    assert_eq!(
        validate_server_url("http://localhost:8080").unwrap(),
        "http://localhost:8080"
    );
    assert!(validate_server_url("http://127.0.0.1:8080").is_ok());
    for bad in [
        "http://green.kiacms.co.kr",
        "http://localhost.evil.com",
        "https://user:pw@green.kiacms.co.kr",
        "https://user@green.kiacms.co.kr",
        "https://green.kiacms.co.kr/?a=1",
        "https://green.kiacms.co.kr/#x",
        "https://green.kiacms.co.kr/api",
        "ftp://green.kiacms.co.kr",
        "file:///etc/passwd",
        "",
        "green.kiacms.co.kr",
    ] {
        assert!(validate_server_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn changing_server_clears_session() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let keys = FakeKeys::default();
    put_session(&keys, "https://green.kiacms.co.kr", "acc", "ref");

    // 기본값은 dev 서버, 같은 주소면 세션 유지.
    let s = Settings::default();
    assert_eq!(s.server_url, "https://green.kiacms.co.kr");
    save_settings(&store, &keys, s.clone()).unwrap();
    assert!(keys.session().is_some());

    // 잘못된 주소는 저장하지 않고 세션도 그대로.
    let bad = Settings {
        server_url: "http://evil.example".into(),
        ..s.clone()
    };
    assert!(save_settings(&store, &keys, bad).is_err());
    assert!(keys.session().is_some());
    assert_eq!(
        store.get_settings().unwrap().server_url,
        "https://green.kiacms.co.kr"
    );

    let other = Settings {
        server_url: "https://other.example".into(),
        ..s
    };
    save_settings(&store, &keys, other).unwrap();
    assert!(keys.session().is_none());
    assert_eq!(
        store.get_settings().unwrap().server_url,
        "https://other.example"
    );
}

#[test]
fn navigation_stays_in_app() {
    use ieum_desktop_lib::is_app_url;
    let u = |s: &str| tauri::Url::parse(s).unwrap();
    let dev = u("http://localhost:1420");
    assert!(is_app_url(&u("tauri://localhost/index.html"), None));
    assert!(is_app_url(&u("http://localhost:1420/x"), Some(&dev)));
    assert!(!is_app_url(&u("http://localhost:1420/x"), None));
    assert!(!is_app_url(&u("https://green.kiacms.co.kr/"), Some(&dev)));
    assert!(!is_app_url(&u("http://localhost:1421/"), Some(&dev)));
    assert!(!is_app_url(&u("tauri://evil/"), None));
}

#[test]
fn token_debug_masked() {
    let s = Session {
        email: "a@b.c".into(),
        server_url: "https://x".into(),
        access_token: "acc-secret".into(),
        refresh_token: "ref-secret".into(),
    };
    let shown = format!("{s:?} {s:#?}");
    assert!(
        !shown.contains("acc-secret") && !shown.contains("ref-secret"),
        "{shown}"
    );
    assert!(shown.contains("a@b.c"));
}

#[tokio::test]
async fn no_token_in_settings_file() {
    let (base, _) = serve(|_| ok(tokens("acc-secret", "ref-secret"))).await;
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let keys = Arc::new(FakeKeys::default());
    let s = Settings {
        server_url: base.clone(),
        ..Settings::default()
    };
    save_settings(&store, keys.as_ref(), s.clone()).unwrap();
    BeClient::new(&base, keys.clone())
        .unwrap()
        .login("a@b.c", PASSWORD)
        .await
        .unwrap();
    save_settings(&store, keys.as_ref(), s).unwrap();

    let mut n = 0;
    for e in fs::read_dir(t.path()).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            n += 1;
            let text = fs::read_to_string(&p).unwrap();
            for secret in ["acc-secret", "ref-secret", PASSWORD] {
                assert!(!text.contains(secret), "{}", p.display());
            }
        }
    }
    assert!(n >= 1);
    assert!(keys.session().is_some());
}
