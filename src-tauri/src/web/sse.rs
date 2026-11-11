//! SSE 읽기(`text/event-stream`). BE는 이벤트 이름 = `ExecutionEvent.type`, data = 그 JSON,
//! keep-alive 주석은 보내지 않는다. `data:` 줄만 모아 JSON으로 넘기고 `event:`·`id:`·주석(`:`)은 무시한다.

use std::time::Duration;

use reqwest::Response;
use serde_json::Value;
use tokio::time::{Instant, timeout, timeout_at};

use super::client::{STREAM_CONNECT, WebError, net};

#[derive(Debug, Clone)]
pub struct SseLimits {
    /// 이벤트(바이트 조각) 사이 최대 유휴.
    pub idle: Duration,
    /// 이벤트 1건 `data` 최대 바이트 — 넘으면 버린다.
    pub max_event: usize,
    /// 연결 하나의 전체 시간(구독은 재연결을 합쳐 이 시간까지).
    pub total: Duration,
    /// 연결부터 응답 헤더까지(`BeClient::stream`).
    pub connect: Duration,
}

impl Default for SseLimits {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(90),
            max_event: 256 * 1024,
            total: Duration::from_secs(30 * 60),
            connect: STREAM_CONNECT,
        }
    }
}

/// 받은 이벤트를 순서대로 `on_event`에 넘긴다. `on_event`가 `true`면(끝 이벤트) 거기서 멈추고 `Ok(true)`.
/// 서버가 닫으면 `Ok(false)`, 유휴·전체 시간 초과나 끊김이면 `Err`.
/// 크기 상한을 넘거나 JSON이 아닌 이벤트는 버리고 경고 1건.
pub async fn read_events(
    mut resp: Response,
    limits: &SseLimits,
    mut on_event: impl FnMut(Value) -> bool,
) -> Result<bool, WebError> {
    let deadline = Instant::now() + limits.total;
    let timed_out = || WebError::Network("시간 초과".into());
    let mut p = Parser::new(limits.max_event);
    loop {
        let chunk = timeout_at(deadline, timeout(limits.idle, resp.chunk()))
            .await
            .map_err(|_| timed_out())?
            .map_err(|_| timed_out())?
            .map_err(net)?;
        let Some(chunk) = chunk else {
            return Ok(false);
        };
        for &b in chunk.iter() {
            if let Some(v) = p.byte(b)
                && on_event(v)
            {
                return Ok(true);
            }
        }
    }
}

struct Parser {
    max: usize,
    line: Vec<u8>,
    data: Vec<u8>,
    has_data: bool,
    too_big: bool,
}

impl Parser {
    fn new(max: usize) -> Self {
        Self {
            max,
            line: Vec::new(),
            data: Vec::new(),
            has_data: false,
            too_big: false,
        }
    }

    /// 이벤트가 끝나 넘길 값이 생기면 `Some`.
    fn byte(&mut self, b: u8) -> Option<Value> {
        if b != b'\n' {
            // 한 줄이 상한을 넘으면 더 모으지 않는다(메모리 상한).
            if self.line.len() <= self.max + 8 {
                self.line.push(b);
            } else {
                self.too_big = true;
            }
            return None;
        }
        let mut line = std::mem::take(&mut self.line);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            return self.dispatch();
        }
        if let Some(rest) = line.strip_prefix(b"data:") {
            let rest = rest.strip_prefix(b" ").unwrap_or(rest);
            if self.has_data {
                self.data.push(b'\n');
            }
            self.has_data = true;
            if self.data.len() + rest.len() > self.max {
                self.too_big = true;
                self.data.clear();
            } else if !self.too_big {
                self.data.extend_from_slice(rest);
            }
        }
        None
    }

    fn dispatch(&mut self) -> Option<Value> {
        let (data, has, big) = (
            std::mem::take(&mut self.data),
            std::mem::replace(&mut self.has_data, false),
            std::mem::replace(&mut self.too_big, false),
        );
        if big {
            eprintln!("[ieum] 실행 이벤트가 너무 커서 버렸어요.");
            return None;
        }
        if !has {
            return None;
        }
        match serde_json::from_slice(&data) {
            Ok(v) => Some(v),
            Err(_) => {
                eprintln!("[ieum] JSON이 아닌 실행 이벤트를 버렸어요.");
                None
            }
        }
    }
}
