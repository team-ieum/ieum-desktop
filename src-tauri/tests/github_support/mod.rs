//! 테스트 `Deps`의 GitHub 러너·폴링 토큰과 가짜 GitHub 서버. 토큰은 늘 실패하거나 가짜 값 —
//! 테스트가 진짜 gh·GitHub에 닿지 않게. 서버는 127.0.0.1 임의 포트에만 연다.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ieum_engine::NodeError;
use ieum_local::github::{GithubApi, GithubRunner, GithubToken, Secret, WatchToken};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn not_connected() -> NodeError {
    NodeError {
        message: "GitHub 연결이 필요해요. 설정 > 연동에서 연결해 주세요.".into(),
        code: Some("GITHUB_NOT_CONNECTED".into()),
    }
}

struct NoToken;

#[async_trait]
impl GithubToken for NoToken {
    async fn token(&self) -> Result<String, NodeError> {
        Err(not_connected())
    }
}

pub fn no_token() -> Arc<dyn GithubToken> {
    Arc::new(NoToken)
}

pub fn no_github() -> Arc<GithubRunner> {
    Arc::new(GithubRunner::new(GithubApi::new(), no_token()))
}

/// 폴링 토큰 가짜. `token`·`forced`(force = true일 때)가 `None`이면 연결 없음. 부른 force 값을 남긴다.
pub struct FakeWatch {
    pub token: Mutex<Option<String>>,
    pub forced: Mutex<Option<String>>,
    pub calls: Mutex<Vec<bool>>,
}

#[async_trait]
impl WatchToken for FakeWatch {
    async fn token(&self, force: bool) -> Result<Secret, NodeError> {
        self.calls.lock().unwrap().push(force);
        let t = if force { &self.forced } else { &self.token };
        t.lock()
            .unwrap()
            .clone()
            .map(Secret::new)
            .ok_or_else(not_connected)
    }
}

/// 늘 같은 토큰을 주는 가짜.
pub fn fake_watch(token: &str) -> Arc<FakeWatch> {
    Arc::new(FakeWatch {
        token: Mutex::new(Some(token.into())),
        forced: Mutex::new(Some(token.into())),
        calls: Mutex::default(),
    })
}

/// 연결 없음 — 기존 테스트용(GitHub 트리거가 없으면 부르지도 않는다).
pub fn no_watch() -> Arc<dyn WatchToken> {
    Arc::new(FakeWatch {
        token: Mutex::default(),
        forced: Mutex::default(),
        calls: Mutex::default(),
    })
}

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    pub path: String,
    pub head: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }
}

pub type Log = Arc<Mutex<Vec<Req>>>;

/// 본문 없는 GET만 받는다(폴링은 GET뿐).
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
        let mut first = head.lines().next().unwrap_or("").split(' ');
        let method = first.next().unwrap_or("").to_string();
        let path = first.next().unwrap_or("").to_string();
        return Some(Req { method, path, head });
    }
}

pub async fn bind() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    (l, base)
}

/// 연결마다 요청 하나를 읽어 `respond`가 만든 응답을 보내고 닫는다. 요청은 응답 전에 남긴다.
pub fn serve(l: TcpListener, respond: impl Fn(&Req) -> Vec<u8> + Send + 'static) -> Log {
    let log: Log = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let Some(req) = read_request(&mut sock).await else {
                continue;
            };
            seen.lock().unwrap().push(req.clone());
            let res = respond(&req);
            let _ = sock.write_all(&res).await;
            let _ = sock.shutdown().await;
        }
    });
    log
}

pub fn resp(status: u16, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut v = format!(
        "HTTP/1.1 {status} X\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    v.extend_from_slice(body);
    v
}
