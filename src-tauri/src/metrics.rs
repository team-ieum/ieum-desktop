//! 설계서 실험의 실측 기록 — 설정 `chat_metrics`가 켜졌을 때만 `metrics/chat.jsonl`에 줄을 더한다.
//! 줄에는 코드·숫자만 싣는다. 사용자 글·AI 글·오류 원문·경로·라벨·키 id는 넣지 않는다.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use ieum_local::chat::{CallOutcome, TurnResult};
use serde_json::{Value, json};

/// 폴더를 만들고 줄마다 JSON 한 줄을 덧붙인다. 한 번에 써서 다른 턴의 줄과 섞이지 않게.
pub fn append(path: &Path, lines: &[Value]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let out: String = lines.iter().map(|l| format!("{l}\n")).collect();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(out.as_bytes())
}

/// AI 오류 원문 → 종류. 원문은 기록하지 않는다(stderr 꼬리·연결 오류에 경로·주소가 섞인다).
// ponytail: 문구로 분류 — AiError에 종류 칸이 생기면 그걸로
pub fn ai_error_kind(message: &str) -> &'static str {
    if message.contains("시간 초과") {
        "timeout"
    } else if message.contains("HTTP 4") {
        "http_4xx"
    } else if message.contains("HTTP 5") {
        "http_5xx"
    } else if message.contains("연결하지 못했어요") {
        "connect"
    } else {
        "other"
    }
}

/// 턴 하나의 호출·동작 줄, 상한에 잘렸으면 `turn_cut_short` 사건 줄.
pub fn turn_lines(at: &str, workflow_id: &str, mode: &str, t: &TurnResult) -> Vec<Value> {
    let calls = t.calls.iter().map(|c| {
        let u = c.usage.as_ref();
        let outcome = match &c.outcome {
            CallOutcome::Ok => "ok".to_string(),
            CallOutcome::Outer => "outer".into(),
            CallOutcome::ActionsJson => "actions_json".into(),
            CallOutcome::Timeout => "timeout".into(),
            CallOutcome::AiError(m) => format!("ai:{}", ai_error_kind(m)),
        };
        json!({
            "at": at, "workflowId": workflow_id, "mode": mode, "kind": "call",
            "source": u.map(|u| &u.source),
            "model": u.and_then(|u| u.model.as_ref()),
            "ms": c.ms,
            "in": u.map(|u| u.input_tokens),
            "out": u.map(|u| u.output_tokens),
            "cacheRead": u.and_then(|u| u.cache_read_tokens),
            "cacheWrite": u.and_then(|u| u.cache_write_tokens),
            "outcome": outcome,
        })
    });
    let ops = t.ops.iter().map(|o| {
        json!({
            "at": at, "workflowId": workflow_id, "mode": mode, "kind": "op",
            "op": o.op, "ok": o.ok, "code": o.code, "rule": o.rule,
            "errors": o.errors, "ui": o.ui,
        })
    });
    let mut lines: Vec<Value> = calls.chain(ops).collect();
    if t.cut_short {
        lines.push(event_line(at, workflow_id, mode, "turn_cut_short", None));
    }
    lines
}

/// 사건 줄. `attempt`는 있을 때만 싣는다.
pub fn event_line(
    at: &str,
    workflow_id: &str,
    mode: &str,
    event: &str,
    attempt: Option<u32>,
) -> Value {
    let mut v = json!({
        "at": at, "workflowId": workflow_id, "mode": mode, "kind": "event", "event": event,
    });
    if let Some(n) = attempt {
        v["attempt"] = n.into();
    }
    v
}
