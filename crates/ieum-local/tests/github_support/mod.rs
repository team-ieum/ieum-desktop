//! GitHub 테스트용 가짜 서버. 127.0.0.1 임의 포트에만 연다. 테스트 파일에서 `mod github_support;`로 쓴다.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    pub path: String,
    pub head: String,
    pub body: Vec<u8>,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

pub type Log = Arc<Mutex<Vec<Req>>>;

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

pub async fn bind() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    (l, base)
}

/// 연결마다 요청 하나를 읽어 `respond`가 만든 응답을 보내고 닫는다.
pub fn serve(l: TcpListener, respond: impl Fn(&Req) -> Vec<u8> + Send + 'static) -> Log {
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

/// 연결만 받고 응답하지 않는다.
pub fn hang(l: TcpListener) {
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            held.push(l.accept().await.unwrap());
        }
    });
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
