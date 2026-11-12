//! AI 사용량(토큰) — 세 공급원 스트림, 재시도 합산, 엔진 기록. 가짜 HTTP 서버만 쓴다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ieum_engine::{Catalog, Node, NodeCtx, NodeRunner, Runners, run_workflow};
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{
    AiAnswer, AiError, AiProvider, AiRequest, AiRunner, AiUsage, ApiKind, ApiProvider, ChunkFn,
    KeyStore,
};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

struct Key;

impl KeyStore for Key {
    fn get(&self, _account: &str) -> Result<Option<String>, KeyError> {
        Ok(Some("k".into()))
    }
    fn set(&self, _account: &str, _key: &str) -> Result<(), KeyError> {
        Ok(())
    }
    fn delete(&self, _account: &str) -> Result<(), KeyError> {
        Ok(())
    }
}

/// 연결마다 같은 SSE 응답을 보낸다. 받은 요청 본문을 남긴다.
async fn serve(events: &[Value], done: bool) -> (String, Arc<Mutex<Vec<Value>>>) {
    let mut resp =
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n"
            .to_string();
    for e in events {
        resp.push_str(&format!("data: {e}\n\n"));
    }
    if done {
        resp.push_str("data: [DONE]\n\n");
    }
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Arc<Mutex<Vec<Value>>> = Arc::default();
    let seen = log.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
            let body = loop {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
                let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map(|v| v.trim().parse().unwrap())
                    .unwrap_or(0);
                while buf.len() < i + 4 + len {
                    let n = sock.read(&mut tmp).await.unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                }
                break serde_json::from_slice(&buf[i + 4..i + 4 + len]).unwrap_or(Value::Null);
            };
            seen.lock().unwrap().push(body);
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    (base, log)
}

async fn ask(kind: ApiKind, base: &str, model: Option<&str>) -> AiAnswer {
    let req = AiRequest {
        prompt: "p".into(),
        model: model.map(String::from),
        schema: None,
    };
    ApiProvider::with(kind, base, Arc::new(Key))
        .ask(req, &|_: &str| {})
        .await
        .unwrap()
}

#[tokio::test]
async fn anthropic_stream_usage_parsed() {
    let (base, _) = serve(
        &[
            json!({"type":"message_start","message":{"id":"m","content":[],"usage":{
                "input_tokens":25,"output_tokens":1,
                "cache_creation_input_tokens":3,"cache_read_input_tokens":7}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"파랑"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}),
            json!({"type":"message_stop"}),
        ],
        false,
    )
    .await;
    let a = ask(ApiKind::Anthropic, &base, None).await;
    assert_eq!(a.text, "파랑");
    assert_eq!(
        a.usage,
        Some(AiUsage {
            source: "anthropic".into(),
            model: Some("claude-haiku-5-5".into()),
            input_tokens: 25,
            output_tokens: 15,
            cache_read_tokens: Some(7),
            cache_write_tokens: Some(3),
        })
    );
}

#[tokio::test]
async fn openai_stream_requests_and_parses_usage() {
    let (base, log) = serve(
        &[
            json!({"choices":[{"index":0,"delta":{"content":"파랑"}}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":5,
                "prompt_tokens_details":{"cached_tokens":4}}}),
        ],
        true,
    )
    .await;
    let a = ask(ApiKind::OpenAi, &base, Some("gpt-test")).await;
    assert_eq!(a.text, "파랑");
    assert_eq!(
        log.lock().unwrap()[0]["stream_options"]["include_usage"],
        true
    );
    // 입력 토큰은 캐시에서 읽은 만큼을 뺀 값(Anthropic과 같은 뜻).
    assert_eq!(
        a.usage,
        Some(AiUsage {
            source: "openai".into(),
            model: Some("gpt-test".into()),
            input_tokens: 16,
            output_tokens: 5,
            cache_read_tokens: Some(4),
            cache_write_tokens: None,
        })
    );
}

/// GPT-5.6+는 캐시 쓰기를 따로 센다 — prompt_tokens에서 읽기·쓰기를 빼 일반 입력만 남긴다.
#[tokio::test]
async fn openai_cache_write_tokens_split() {
    let (base, _) = serve(
        &[
            json!({"choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":12000,"completion_tokens":5,
                "prompt_tokens_details":{"cached_tokens":4000,"cache_write_tokens":7000}}}),
        ],
        true,
    )
    .await;
    let u = ask(ApiKind::OpenAi, &base, Some("gpt-6-sol"))
        .await
        .usage
        .unwrap();
    assert_eq!(
        (u.input_tokens, u.cache_read_tokens, u.cache_write_tokens),
        (1000, Some(4000), Some(7000))
    );
}

#[tokio::test]
async fn gemini_usage_metadata_parsed() {
    let (base, _) = serve(
        &[
            json!({"candidates":[{"content":{"parts":[{"text":"파"}]}}],
                "usageMetadata":{"promptTokenCount":30,"candidatesTokenCount":1}}),
            json!({"candidates":[{"content":{"parts":[{"text":"랑"}]},"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":30,"candidatesTokenCount":8,
                    "thoughtsTokenCount":2,"cachedContentTokenCount":10}}),
        ],
        false,
    )
    .await;
    let a = ask(ApiKind::Gemini, &base, None).await;
    assert_eq!(a.text, "파랑");
    // 생각 토큰은 출력 요금이라 출력에 더한다.
    assert_eq!(
        a.usage,
        Some(AiUsage {
            source: "gemini".into(),
            model: Some("gemini-3.5-flash".into()),
            input_tokens: 20,
            output_tokens: 10,
            cache_read_tokens: Some(10),
            cache_write_tokens: None,
        })
    );
}

/// 정해 둔 답을 차례로, 부를 때마다 입력 10·출력 2 토큰.
struct Fake {
    answers: Vec<&'static str>,
    calls: AtomicUsize,
}

#[async_trait]
impl AiProvider for Fake {
    async fn ask(&self, _req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
        let i = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(AiAnswer {
            text: self.answers[i.min(self.answers.len() - 1)].into(),
            usage: Some(usage(10, 2)),
        })
    }
}

fn usage(input: u64, output: u64) -> AiUsage {
    AiUsage {
        source: "anthropic".into(),
        model: Some("m".into()),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: None,
        cache_write_tokens: None,
    }
}

fn fake_runner(answers: Vec<&'static str>) -> AiRunner {
    AiRunner::empty().with_provider(
        "api:anthropic",
        Arc::new(Fake {
            answers,
            calls: AtomicUsize::new(0),
        }),
    )
}

/// 출력 칸 `color`를 요구하는 노드를 `answers`로 돌린다.
async fn run_color(
    answers: Vec<&'static str>,
) -> (
    Result<Map<String, Value>, ieum_engine::NodeError>,
    Option<Value>,
) {
    let runner = fake_runner(answers);
    let node: Node = serde_json::from_value(json!({"id": "n", "event": "local.ai"})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let config = json!({"provider": "api:anthropic", "prompt": "p",
        "outputFields": [{"key": "color", "type": "text"}]});
    runner
        .run_metered(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            events: &tx,
            cancel: &CancellationToken::new(),
        })
        .await
}

#[tokio::test]
async fn retry_sums_usage() {
    let (r, u) = run_color(vec!["JSON 아님", r#"{"color":"파랑"}"#]).await;
    assert_eq!(r.unwrap()["color"], "파랑");
    assert_eq!(u, Some(serde_json::to_value(usage(20, 4)).unwrap()));
}

/// 두 시도 모두 모양이 틀려 노드가 실패해도 쓴 토큰은 남는다.
#[tokio::test]
async fn failed_retries_keep_usage() {
    let (r, u) = run_color(vec!["JSON 아님", "JSON 아님"]).await;
    assert!(r.is_err());
    assert_eq!(u, Some(serde_json::to_value(usage(20, 4)).unwrap()));
}

#[tokio::test]
async fn usage_not_in_node_output() {
    let wf = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/in"}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "api:anthropic", "prompt": "p"}},
        ],
        "edges": [{"source": "t", "target": "a"}],
    }))
    .unwrap();
    let mut runners: Runners = HashMap::new();
    runners.insert("local.ai".into(), Arc::new(fake_runner(vec!["파랑"])));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let run = run_workflow(
        &wf,
        &Catalog::embedded().unwrap(),
        &runners,
        Map::new(),
        tx,
        CancellationToken::new(),
    )
    .await;
    let rec = &run.nodes["a"];
    let out = rec.output.as_ref().unwrap();
    assert_eq!(out.keys().collect::<Vec<_>>(), ["output"]);
    assert_eq!(rec.usage.as_ref().unwrap()["inputTokens"], 10);
    // 트리거 같은 다른 노드는 usage를 쓰지 않는다(JSON에 키도 없다).
    let t = serde_json::to_value(&run.nodes["t"]).unwrap();
    assert!(t.get("usage").is_none(), "{t}");
}
