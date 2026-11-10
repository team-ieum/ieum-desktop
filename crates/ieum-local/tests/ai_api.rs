//! API 키 공급원. 실제 API·키체인은 부르지 않는다 — 로컬 가짜 HTTP 서버와 주입한 키 저장소만.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_engine::{ExecEvent, Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::ai::{
    AiError, AiProvider, AiRequest, AiRunner, ApiKey, ApiKind, ApiProvider, KeyStore,
};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

const KEY: &str = "test-key-123";
const KINDS: [ApiKind; 3] = [ApiKind::Anthropic, ApiKind::OpenAi, ApiKind::Gemini];

struct Store(Option<&'static str>);

impl KeyStore for Store {
    fn get(&self, _account: &str) -> Option<String> {
        self.0.map(String::from)
    }
}

#[derive(Debug, Clone)]
struct Seen {
    head: String,
    body: Value,
}

impl Seen {
    fn request_line(&self) -> &str {
        self.head.lines().next().unwrap_or("")
    }
}

type Log = Arc<Mutex<Vec<Seen>>>;

async fn read_request(sock: &mut TcpStream) -> Seen {
    let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
    loop {
        let n = sock.read(&mut tmp).await.unwrap();
        assert!(n > 0, "요청이 끝나기 전에 끊김");
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
            let n = sock.read(&mut tmp).await.unwrap();
            assert!(n > 0);
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = serde_json::from_slice(&buf[i + 4..i + 4 + len]).unwrap_or(Value::Null);
        return Seen { head, body };
    }
}

/// 연결마다 같은 응답을 보내고 닫는 가짜 서버.
async fn serve(response: String) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let req = read_request(&mut sock).await;
            seen.lock().unwrap().push(req);
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    (base, log)
}

const SSE_HEAD: &str =
    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";

fn sse(events: &[Value]) -> String {
    let mut s = SSE_HEAD.to_string();
    for e in events {
        s.push_str(&format!("data: {e}\n\n"));
    }
    s
}

fn http(status: u16, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn anthropic_text(pieces: &[&str]) -> String {
    let mut ev = vec![json!({"type":"message_start","message":{"id":"m","content":[]}})];
    ev.push(
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
    );
    for p in pieces {
        ev.push(
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":p}}),
        );
    }
    ev.push(json!({"type":"content_block_stop","index":0}));
    ev.push(json!({"type":"message_stop"}));
    sse(&ev)
}

fn openai_text(pieces: &[&str]) -> String {
    let mut s = sse(&pieces
        .iter()
        .map(|p| json!({"choices":[{"index":0,"delta":{"content":p}}]}))
        .collect::<Vec<_>>());
    s.push_str("data: [DONE]\n\n");
    s
}

fn gemini_text(pieces: &[&str]) -> String {
    let n = pieces.len();
    sse(&pieces
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut c = json!({"content":{"role":"model","parts":[{"text":p}]}});
            if i + 1 == n {
                c["finishReason"] = json!("STOP");
            }
            json!({"candidates":[c]})
        })
        .collect::<Vec<_>>())
}

fn text_response(kind: ApiKind, pieces: &[&str]) -> String {
    match kind {
        ApiKind::Anthropic => anthropic_text(pieces),
        ApiKind::OpenAi => openai_text(pieces),
        ApiKind::Gemini => gemini_text(pieces),
    }
}

fn provider(kind: ApiKind, base: &str, key: Option<&'static str>) -> ApiProvider {
    ApiProvider::with(kind, base, Arc::new(Store(key)))
}

async fn ask(
    kind: ApiKind,
    base: &str,
    model: Option<&str>,
) -> (Vec<String>, Result<String, AiError>) {
    let chunks = Mutex::new(Vec::new());
    let req = AiRequest {
        prompt: "하늘 색은?".into(),
        model: model.map(String::from),
        schema: None,
    };
    let r = provider(kind, base, Some(KEY))
        .ask(req, &|c: &str| chunks.lock().unwrap().push(c.to_string()))
        .await;
    (chunks.into_inner().unwrap(), r)
}

async fn run_with(
    runner: &AiRunner,
    config: Value,
    cancel: &CancellationToken,
) -> (Result<Map<String, Value>, NodeError>, Vec<ExecEvent>) {
    let node: Node = serde_json::from_value(json!({"id": "n", "event": "local.ai"})).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let r = runner
        .run(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            events: &tx,
            cancel,
        })
        .await;
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (r, events)
}

fn runner(kind: ApiKind, base: &str) -> AiRunner {
    AiRunner::empty().with_provider(kind.id(), Arc::new(provider(kind, base, Some(KEY))))
}

#[tokio::test]
async fn anthropic_streams_text() {
    let (base, log) = serve(anthropic_text(&["파", "랑"])).await;
    let (chunks, r) = ask(ApiKind::Anthropic, &base, None).await;
    assert_eq!(chunks, vec!["파", "랑"]);
    assert_eq!(r.unwrap(), "파랑");
    let seen = log.lock().unwrap()[0].clone();
    assert!(
        seen.request_line().starts_with("POST /v1/messages "),
        "{}",
        seen.head
    );
    assert!(seen.head.contains(&format!("x-api-key: {KEY}")));
    assert!(seen.head.contains("anthropic-version: "));
    assert_eq!(seen.body["model"], "claude-haiku-5-5");
    assert_eq!(seen.body["stream"], true);
    assert_eq!(seen.body["messages"][0]["content"], "하늘 색은?");
}

#[tokio::test]
async fn openai_streams_text() {
    let (base, log) = serve(openai_text(&["파", "랑"])).await;
    let (chunks, r) = ask(ApiKind::OpenAi, &base, Some("gpt-test")).await;
    assert_eq!(chunks, vec!["파", "랑"]);
    assert_eq!(r.unwrap(), "파랑");
    let seen = log.lock().unwrap()[0].clone();
    assert!(
        seen.request_line()
            .starts_with("POST /v1/chat/completions "),
        "{}",
        seen.head
    );
    assert!(seen.head.contains(&format!("authorization: Bearer {KEY}")));
    assert_eq!(seen.body["model"], "gpt-test");
    assert_eq!(seen.body["stream"], true);
}

#[tokio::test]
async fn gemini_streams_text() {
    let (base, log) = serve(gemini_text(&["파", "랑"])).await;
    let (chunks, r) = ask(ApiKind::Gemini, &base, None).await;
    assert_eq!(chunks, vec!["파", "랑"]);
    assert_eq!(r.unwrap(), "파랑");
    let seen = log.lock().unwrap()[0].clone();
    assert!(
        seen.request_line()
            .starts_with("POST /v1beta/models/gemini-3.5-flash:streamGenerateContent?alt=sse "),
        "{}",
        seen.head
    );
    assert_eq!(seen.body["contents"][0]["parts"][0]["text"], "하늘 색은?");
}

#[tokio::test]
async fn gemini_key_never_in_url() {
    let (base, log) = serve(gemini_text(&["ok"])).await;
    ask(ApiKind::Gemini, &base, None).await.1.unwrap();
    let seen = log.lock().unwrap()[0].clone();
    let line = seen.request_line();
    assert!(!line.contains(KEY) && !line.contains("key="), "{line}");
    assert!(seen.head.contains(&format!("x-goog-api-key: {KEY}")));
}

#[tokio::test]
async fn missing_key_fails_with_message() {
    for kind in KINDS {
        let (base, log) = serve(text_response(kind, &["x"])).await;
        let req = AiRequest {
            prompt: "p".into(),
            model: Some("m".into()),
            schema: None,
        };
        let err = provider(kind, &base, None)
            .ask(req, &|_: &str| {})
            .await
            .unwrap_err();
        assert!(err.0.contains("API 키가 없어요"), "{err}");
        assert!(log.lock().unwrap().is_empty(), "키 없이 요청을 보냄");
    }
}

#[tokio::test]
async fn error_message_never_contains_key() {
    // 스트림 중간 오류가 키를 되울려도
    for kind in KINDS {
        let echo =
            json!({"type":"error","error":{"type":"bad","message":format!("key {KEY} rejected")}});
        let (base, _) = serve(sse(&[echo])).await;
        let err = ask(kind, &base, Some("m")).await.1.unwrap_err();
        assert!(!err.0.contains(KEY), "{err}");
        assert!(err.0.contains("rejected"), "{err}");
    }
    // 연결 실패도
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let err = ask(ApiKind::Gemini, &format!("http://127.0.0.1:{port}"), None)
        .await
        .1
        .unwrap_err();
    assert!(!err.0.contains(KEY), "{err}");
}

#[tokio::test]
async fn error_body_echoing_key_is_masked() {
    let body = json!({"error":{"type":"invalid_request_error","message":format!("invalid x-api-key: {KEY}")}});
    let (base, _) = serve(http(400, &body.to_string())).await;
    let err = ask(ApiKind::Anthropic, &base, None).await.1.unwrap_err();
    assert!(!err.0.contains(KEY), "{err}");
    assert!(
        err.0.contains("***") && err.0.contains("invalid_request_error"),
        "{err}"
    );

    // 잘림 경계에 걸쳐도 키 앞부분이 새지 않는다.
    let long = format!("{}{KEY}{}", "a".repeat(290), "b".repeat(50));
    let body = json!({"error":{"message": long}});
    let (base, _) = serve(http(400, &body.to_string())).await;
    let err = ask(ApiKind::OpenAi, &base, Some("m")).await.1.unwrap_err();
    assert!(!err.0.contains("test-key"), "{err}");

    // JSON이 아닌 본문은 그대로 싣지 않는다.
    let (base, _) = serve(http(400, &format!("raw {KEY}"))).await;
    let err = ask(ApiKind::Gemini, &base, None).await.1.unwrap_err();
    assert!(!err.0.contains(KEY) && !err.0.contains("raw"), "{err}");
}

#[tokio::test]
async fn http_status_messages() {
    let cases = [
        (401, "API 키를 확인해 주세요."),
        (403, "API 키를 확인해 주세요."),
        (429, "요청이 너무 많아요. 잠시 뒤 다시 시도해 주세요."),
        (500, "AI 서비스에 문제가 있어요."),
        (503, "AI 서비스에 문제가 있어요."),
    ];
    for kind in KINDS {
        for (status, msg) in cases {
            let (base, log) = serve(http(status, r#"{"error":{"message":"nope"}}"#)).await;
            let cfg = json!({
                "provider": kind.id(), "model": "m", "prompt": "p",
                "outputFields": [{"key": "color", "type": "text"}],
            });
            let (r, _) = run_with(&runner(kind, &base), cfg, &CancellationToken::new()).await;
            let err = r.unwrap_err().message;
            assert!(err.contains(msg), "{status}: {err}");
            // 429·5xx도 노드 수준 재시도는 없다.
            assert_eq!(log.lock().unwrap().len(), 1, "{status}");
        }
    }
}

#[tokio::test]
async fn openai_requires_model() {
    let (base, log) = serve(openai_text(&["x"])).await;
    let cfg = json!({"provider": "api:openai", "prompt": "p", "model": " "});
    let (r, _) = run_with(
        &runner(ApiKind::OpenAi, &base),
        cfg,
        &CancellationToken::new(),
    )
    .await;
    assert!(r.unwrap_err().message.contains("모델을 골라 주세요"));
    assert!(log.lock().unwrap().is_empty());
}

#[tokio::test]
async fn debug_output_masks_key() {
    let k = ApiKey::new(KEY);
    let shown = format!("{k:?}");
    assert!(!shown.contains(KEY) && shown.contains("***"), "{shown}");
    for kind in KINDS {
        let shown = format!("{:?}", provider(kind, "http://127.0.0.1:1", Some(KEY)));
        assert!(!shown.contains(KEY), "{shown}");
    }
}

#[tokio::test]
async fn cancel_aborts_request() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = l.accept().await.unwrap();
        read_request(&mut sock).await;
        let first = json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"첫"}});
        sock.write_all(format!("{SSE_HEAD}data: {first}\n\n").as_bytes())
            .await
            .unwrap();
        // 답을 끝내지 않고 붙든다 — 클라이언트가 끊으면 read가 0을 돌려준다.
        let mut b = [0u8; 64];
        while sock.read(&mut b).await.is_ok_and(|n| n > 0) {}
        let _ = closed_tx.send(());
    });
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        c.cancel();
    });
    let cfg = json!({"provider": "api:anthropic", "prompt": "p"});
    let (r, events) = run_with(&runner(ApiKind::Anthropic, &base), cfg, &cancel).await;
    assert!(r.unwrap_err().message.contains("취소"));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ExecEvent::NodeChunk { text, .. } if text == "첫")),
        "{events:?}"
    );
    tokio::time::timeout(Duration::from_secs(5), closed_rx)
        .await
        .expect("취소 뒤에도 연결이 열려 있음")
        .unwrap();
}

#[tokio::test]
async fn structured_output_each_provider() {
    let answer = r#"{"color":"파랑"}"#;
    for kind in KINDS {
        let response = match kind {
            ApiKind::Anthropic => sse(&[
                json!({"type":"message_start","message":{"id":"m","content":[]}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t","name":"output","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"color\":"}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"파랑\"}"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_stop"}),
            ]),
            _ => text_response(kind, &[&answer[..9], &answer[9..]]),
        };
        let (base, log) = serve(response).await;
        let cfg = json!({
            "provider": kind.id(), "model": "m", "prompt": "색은?",
            "outputFields": [{"key": "color", "type": "text", "description": "색 이름"}],
        });
        let (r, _) = run_with(&runner(kind, &base), cfg, &CancellationToken::new()).await;
        assert_eq!(r.unwrap()["color"], "파랑", "{kind:?}");
        let body = log.lock().unwrap()[0].body.clone();
        match kind {
            ApiKind::Anthropic => {
                assert_eq!(
                    body["tool_choice"],
                    json!({"type": "tool", "name": "output"})
                );
                assert_eq!(body["tools"][0]["name"], "output");
                assert_eq!(
                    body["tools"][0]["input_schema"]["required"],
                    json!(["color"])
                );
            }
            ApiKind::OpenAi => {
                let rf = &body["response_format"];
                assert_eq!(rf["type"], "json_schema");
                assert_eq!(rf["json_schema"]["strict"], true);
                assert_eq!(rf["json_schema"]["schema"]["additionalProperties"], false);
                assert_eq!(
                    rf["json_schema"]["schema"]["properties"]["color"]["type"],
                    "string"
                );
            }
            ApiKind::Gemini => {
                let g = &body["generationConfig"];
                assert_eq!(g["responseMimeType"], "application/json");
                assert_eq!(g["responseSchema"]["type"], "OBJECT");
                assert_eq!(g["responseSchema"]["properties"]["color"]["type"], "STRING");
                assert_eq!(g["responseSchema"]["required"], json!(["color"]));
            }
        }
    }
}
