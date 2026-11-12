//! API 키 여러 개 + 노드마다 고르기. 키체인은 가짜, AI는 로컬 가짜 HTTP 서버.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset};
use ieum_desktop_lib::credentials::{AccountCredentials, MISSING};
use ieum_desktop_lib::store::StoredRun;
use ieum_desktop_lib::usage::summarize;
use ieum_engine::{Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiRunner, AiUsage, ApiKind, ApiProvider, CredentialResolver, KeyStore};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

const K16: &str = "aaaaaaaaaaaaaaaa";

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

fn setup() -> (TempDir, Arc<FakeKeys>, Arc<AccountCredentials>) {
    let t = tempfile::tempdir().unwrap();
    let keys = Arc::new(FakeKeys::default());
    let creds = Arc::new(AccountCredentials::new(
        t.path().to_path_buf(),
        K16,
        keys.clone(),
    ));
    (t, keys, creds)
}

fn is_hex8(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn legacy_key_appears_as_default() {
    let (_t, keys, c) = setup();
    keys.set(&format!("anthropic.{K16}"), "sk-ant-OLD9999")
        .unwrap();
    let v = c.list();
    assert_eq!(v.keychain_error, None);
    let a = &v.api_keys["anthropic"];
    assert_eq!(a.len(), 1);
    assert_eq!(
        (a[0].id.as_str(), a[0].label.as_str(), a[0].is_default),
        ("default", "기본", true)
    );
    assert_eq!(a[0].last4, "9999");
    assert!(v.api_keys["openai"].is_empty());
    // 이 Mac의 로그인은 늘 있다.
    assert_eq!(v.claude_accounts[0].id, "system");
    assert!(v.claude_accounts[0].is_default);
    // 옮겨 쓰지 않는다.
    assert_eq!(keys.0.lock().unwrap().len(), 1);
}

#[test]
fn add_second_key_keeps_first_default() {
    let (_t, keys, c) = setup();
    keys.set(&format!("anthropic.{K16}"), "sk-ant-OLD9999")
        .unwrap();
    let m = c
        .add_api_key("anthropic", "회사", "sk-ant-COMPANY1")
        .unwrap();
    assert!(is_hex8(&m.id), "{}", m.id);
    assert!(!m.is_default);
    let a = &c.list().api_keys["anthropic"];
    assert_eq!(a.len(), 2);
    assert!(a.iter().find(|k| k.id == "default").unwrap().is_default);
    assert_eq!(
        keys.get(&format!("anthropic.{K16}.{}", m.id))
            .unwrap()
            .as_deref(),
        Some("sk-ant-COMPANY1")
    );
}

#[test]
fn first_key_is_default() {
    let (_t, _keys, c) = setup();
    let m = c.add_api_key("openai", "개인", "sk-proj-abcdefgh").unwrap();
    assert!(m.is_default);
}

#[test]
fn delete_default_promotes_oldest() {
    let (_t, keys, c) = setup();
    let a = c.add_api_key("gemini", "A", "key-aaaaaaaa").unwrap();
    let b = c.add_api_key("gemini", "B", "key-bbbbbbbb").unwrap();
    let _c2 = c.add_api_key("gemini", "C", "key-cccccccc").unwrap();
    c.delete("gemini", &a.id).unwrap();
    let list = &c.list().api_keys["gemini"];
    assert_eq!(list.len(), 2);
    assert_eq!(list.iter().filter(|k| k.is_default).count(), 1);
    assert!(list.iter().find(|k| k.id == b.id).unwrap().is_default);
    assert_eq!(keys.get(&format!("gemini.{K16}.{}", a.id)).unwrap(), None);
    // 마지막까지 지우면 저장 안 됨.
    for k in list.clone() {
        c.delete("gemini", &k.id).unwrap();
    }
    assert!(c.list().api_keys["gemini"].is_empty());
}

#[test]
fn rename_and_set_default() {
    let (_t, _keys, c) = setup();
    let a = c.add_api_key("anthropic", "A", "sk-ant-aaaaaaaa").unwrap();
    let b = c.add_api_key("anthropic", "B", "sk-ant-bbbbbbbb").unwrap();
    c.rename("anthropic", &b.id, "회사").unwrap();
    c.set_default("anthropic", &b.id).unwrap();
    let list = &c.list().api_keys["anthropic"];
    let get = |id: &str| list.iter().find(|k| k.id == id).unwrap().clone();
    assert_eq!(get(&b.id).label, "회사");
    assert!(get(&b.id).is_default && !get(&a.id).is_default);
    assert_eq!(c.rename("anthropic", "deadbeef", "x").unwrap_err(), MISSING);
    assert!(c.delete("claude", "system").is_err());
}

#[test]
fn list_never_contains_key_values() {
    let (_t, _keys, c) = setup();
    let m = c
        .add_api_key("anthropic", "회사", "sk-ant-SECRET1234")
        .unwrap();
    assert_eq!(m.last4, "1234");
    let text = serde_json::to_string(&c.list()).unwrap();
    assert!(!text.contains("SECRET"), "{text}");
    let e = c.add_api_key("nope", "x", "sk-ant-SECRET5678").unwrap_err();
    assert!(!e.contains("SECRET"), "{e}");
    // 짧은 키는 끝 4자도 보이지 않는다(키 대부분이 드러난다).
    assert_eq!(c.add_api_key("openai", "짧음", "abcd").unwrap().last4, "");
}

#[test]
fn resolver_picks_named_key() {
    let (_t, _keys, c) = setup();
    let a = c.add_api_key("anthropic", "A", "sk-ant-aaaaaaaa").unwrap();
    let b = c.add_api_key("anthropic", "B", "sk-ant-bbbbbbbb").unwrap();
    assert_eq!(
        c.api_key_name("anthropic", Some(&b.id)).unwrap(),
        format!("anthropic.{K16}.{}", b.id)
    );
    assert_eq!(
        c.api_key_name("anthropic", None).unwrap(),
        format!("anthropic.{K16}.{}", a.id)
    );
    assert_eq!(
        c.api_key_name("anthropic", Some("deadbeef")).unwrap_err(),
        MISSING
    );
    assert_eq!(c.claude_config_dir(None).unwrap(), None);
    assert_eq!(c.claude_config_dir(Some("system")).unwrap(), None);
    assert_eq!(c.claude_config_dir(Some("deadbeef")).unwrap_err(), MISSING);
}

// ---- 노드 실행 ----

/// Anthropic SSE 응답을 주는 가짜 서버. 요청마다 `x-api-key`를 보내고, `gate`가 있으면 허가를 받고 답한다.
async fn serve(gate: Option<Arc<Semaphore>>) -> (String, mpsc::UnboundedReceiver<String>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
            let head_end = loop {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i;
                }
            };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let len: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            while buf.len() < head_end + 4 + len {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
            }
            let key = head
                .lines()
                .find_map(|l| l.strip_prefix("x-api-key: "))
                .unwrap_or("")
                .to_string();
            let _ = tx.send(key);
            if let Some(g) = &gate {
                g.acquire().await.unwrap().forget();
            }
            let mut s =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n"
                    .to_string();
            for e in [
                json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}),
                json!({"type":"message_stop"}),
            ] {
                s.push_str(&format!("data: {e}\n\n"));
            }
            let _ = sock.write_all(s.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    (base, rx)
}

fn runner(base: &str, keys: Arc<FakeKeys>, creds: Arc<AccountCredentials>) -> AiRunner {
    AiRunner::empty()
        .with_provider(
            ApiKind::Anthropic.id(),
            Arc::new(ApiProvider::with(ApiKind::Anthropic, base, keys)),
        )
        .with_resolver(creds)
}

async fn run(
    r: &AiRunner,
    credential: Option<&str>,
) -> (Result<Map<String, Value>, NodeError>, Option<AiUsage>) {
    let node: Node = serde_json::from_value(json!({"id": "n", "event": "local.ai"})).unwrap();
    let mut config = json!({"provider": "api:anthropic", "prompt": "안녕"});
    if let Some(c) = credential {
        config["credential"] = json!(c);
    }
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let (res, usage) = r
        .run_metered(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            events: &tx,
            cancel: &cancel,
        })
        .await;
    (res, usage.map(|u| serde_json::from_value(u).unwrap()))
}

#[tokio::test]
async fn deleted_credential_fails_node_not_fallback() {
    let (_t, keys, c) = setup();
    c.add_api_key("anthropic", "개인", "sk-ant-PERSONAL1")
        .unwrap();
    let work = c
        .add_api_key("anthropic", "회사", "sk-ant-COMPANY1")
        .unwrap();
    c.delete("anthropic", &work.id).unwrap();
    let (base, mut seen) = serve(None).await;
    let r = runner(&base, keys, c);
    let (res, _) = run(&r, Some(&work.id)).await;
    assert_eq!(res.unwrap_err().message, MISSING);
    assert!(seen.try_recv().is_err(), "기본 키로 요청하면 안 된다");
}

#[tokio::test]
async fn node_without_credential_uses_default() {
    let (_t, keys, c) = setup();
    keys.set(&format!("anthropic.{K16}"), "sk-ant-LEGACY01")
        .unwrap();
    let (base, mut seen) = serve(None).await;
    let r = runner(&base, keys, c);
    let (res, usage) = run(&r, None).await;
    assert_eq!(res.unwrap()["output"], "ok");
    assert_eq!(seen.recv().await.unwrap(), "sk-ant-LEGACY01");
    assert_eq!(usage.unwrap().credential.as_deref(), Some("default"));
}

#[tokio::test]
async fn delete_while_running_does_not_switch_key() {
    let (_t, keys, c) = setup();
    c.add_api_key("anthropic", "개인", "sk-ant-PERSONAL1")
        .unwrap();
    let work = c
        .add_api_key("anthropic", "회사", "sk-ant-COMPANY1")
        .unwrap();
    let gate = Arc::new(Semaphore::new(0));
    let (base, mut seen) = serve(Some(gate.clone())).await;
    let r = runner(&base, keys, c.clone());
    let deleter = async {
        let key = seen.recv().await.unwrap();
        c.delete("anthropic", &work.id).unwrap();
        gate.add_permits(1);
        key
    };
    let ((res, _), key) = tokio::join!(run(&r, Some(&work.id)), deleter);
    assert_eq!(key, "sk-ant-COMPANY1");
    assert_eq!(res.unwrap()["output"], "ok");
    let (res, _) = run(&r, Some(&work.id)).await;
    assert_eq!(res.unwrap_err().message, MISSING);
    assert!(seen.try_recv().is_err());
}

fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

#[tokio::test]
async fn usage_records_credential_id() {
    let (_t, keys, c) = setup();
    c.add_api_key("anthropic", "개인", "sk-ant-PERSONAL1")
        .unwrap();
    let work = c
        .add_api_key("anthropic", "회사", "sk-ant-COMPANY1")
        .unwrap();
    let (base, _seen) = serve(None).await;
    let r = runner(&base, keys, c);
    let (_, used) = run(&r, Some(&work.id)).await;
    let used = used.unwrap();
    assert_eq!(used.credential.as_deref(), Some(work.id.as_str()));

    let mut old = used.clone();
    old.credential = None;
    let nodes: Map<String, Value> = [used.clone(), used, old]
        .iter()
        .enumerate()
        .map(|(i, u)| {
            (
                format!("a{i}"),
                json!({"status": "succeeded", "output": {}, "error": null, "usage": u}),
            )
        })
        .collect();
    let stored: StoredRun = serde_json::from_value(json!({
        "run": {"runId": uuid::Uuid::new_v4().to_string(), "status": "succeeded",
            "validationErrors": [], "rejectReason": null, "nodes": nodes},
        "workflowId": "w", "startedAt": "2026-11-12T00:00:00.000Z",
        "finishedAt": "2026-11-12T00:00:00.000Z",
        "fileOps": [], "allowedRoots": [], "undo": null
    }))
    .unwrap();
    let table = serde_json::from_value(json!({"asOf": "x", "models": {}})).unwrap();
    let s = summarize(
        &[stored],
        at("2026-11-11T00:00:00Z"),
        at("2026-11-13T00:00:00Z"),
        &table,
    );
    let by: Vec<(&str, &str, u64)> = s
        .by_credential
        .iter()
        .map(|c| (c.source.as_str(), c.credential.as_str(), c.output))
        .collect();
    // 옛 기록(credential 없음)은 옛 키 = `default`.
    let mut want = vec![
        ("anthropic", "default", 5),
        ("anthropic", work.id.as_str(), 10),
    ];
    want.sort();
    let mut by = by;
    by.sort();
    assert_eq!(by, want);
}
