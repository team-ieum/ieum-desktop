//! API 키 여러 개 + 노드마다 고르기. 키체인은 가짜, AI는 로컬 가짜 HTTP 서버.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, FixedOffset};
use ieum_desktop_lib::credentials::{AccountCredentials, MISSING, confirm_cli_account};
use ieum_desktop_lib::plan_usage::TokenRead;
use ieum_desktop_lib::store::StoredRun;
use ieum_desktop_lib::sub_usage::SubscriptionUsage;
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

// ---- 구독 계정(Claude·Codex) ----

/// 계정 폴더 경로에 공백이 있게(`~/Library/Application Support/...` 흉내).
fn setup_spaced() -> (TempDir, Arc<AccountCredentials>) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Application Support").join(K16);
    let c = AccountCredentials::new(root, K16, Arc::new(FakeKeys::default()));
    (t, Arc::new(c))
}

/// 토큰 읽기가 받은 설정 폴더를 남기는 사용량 클라이언트(네트워크 없음 — 토큰 없음).
fn fake_usage(home: &std::path::Path) -> (SubscriptionUsage, Arc<Mutex<Vec<Option<PathBuf>>>>) {
    let seen: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::default();
    let s = seen.clone();
    let u = SubscriptionUsage::with(
        "http://127.0.0.1:1/api/oauth/usage",
        "http://127.0.0.1:1/backend-api/wham/usage",
        Duration::from_secs(60),
        Arc::new(move |d: Option<&std::path::Path>| {
            s.lock().unwrap().push(d.map(|d| d.to_path_buf()));
            TokenRead::Missing
        }),
        Arc::new(|_: Option<&std::path::Path>| None),
        home.to_path_buf(),
    )
    .unwrap();
    (u, seen)
}

#[test]
fn begin_account_command_quotes_path() {
    let (t, c) = setup_spaced();
    let b = c.begin_cli_account("claude", "회사").unwrap();
    assert!(is_hex8(&b.id));
    let dir = t
        .path()
        .join("Application Support")
        .join(K16)
        .join("claude")
        .join(&b.id);
    assert!(dir.is_dir());
    assert_eq!(
        b.command,
        format!("CLAUDE_CONFIG_DIR=\"{}\" claude", dir.display())
    );
    // 그대로 붙여 넣어도 경로가 한 덩어리로 넘어간다.
    let probe = b
        .command
        .replace(" claude", " sh -c 'printf %s \"$CLAUDE_CONFIG_DIR\"'");
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&probe)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        dir.display().to_string()
    );

    let x = c.begin_cli_account("codex", "개인").unwrap();
    let dir = t
        .path()
        .join("Application Support")
        .join(K16)
        .join("codex")
        .join(&x.id);
    assert_eq!(
        x.command,
        format!("CODEX_HOME=\"{}\" codex login", dir.display())
    );
    assert!(c.begin_cli_account("gemini", "x").is_err());

    // 따옴표·$·백틱이 든 경로는 명령을 만들지 않는다.
    let bad = AccountCredentials::new(t.path().join("a\"b"), K16, Arc::new(FakeKeys::default()));
    assert!(bad.begin_cli_account("claude", "x").is_err());
    // ASCII가 아닌 경로도(claude가 NFC로 해시해 폴더가 어긋날 수 있다).
    let bad = AccountCredentials::new(t.path().join("계정"), K16, Arc::new(FakeKeys::default()));
    assert!(bad.begin_cli_account("claude", "x").is_err());
}

/// 키체인 접근을 거부당한 저장소.
struct DeniedKeys;

impl KeyStore for DeniedKeys {
    fn get(&self, _: &str) -> Result<Option<String>, KeyError> {
        Err(KeyError("거부됨".into()))
    }
    fn set(&self, _: &str, _: &str) -> Result<(), KeyError> {
        Err(KeyError("거부됨".into()))
    }
    fn delete(&self, _: &str) -> Result<(), KeyError> {
        Err(KeyError("거부됨".into()))
    }
}

#[test]
fn legacy_key_with_keychain_denied_reports_keychain() {
    let t = tempfile::tempdir().unwrap();
    let c = AccountCredentials::new(t.path().to_path_buf(), K16, Arc::new(DeniedKeys));
    let e = c.api_key_name("anthropic", Some("default")).unwrap_err();
    assert_ne!(e, MISSING);
    assert!(e.contains("키체인"), "{e}");
}

#[test]
fn add_key_rolls_back_vault_when_meta_write_fails() {
    use std::os::unix::fs::PermissionsExt;
    let (t, keys, c) = setup();
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = c.add_api_key("openai", "개인", "sk-proj-abcdefgh");
    std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(r.is_err());
    assert!(keys.0.lock().unwrap().is_empty());
}

/// 목록 파일을 고쳐 `../victim` 같은 id를 넣어도 계정 폴더 밖은 건드리지 않는다.
#[test]
fn traversal_id_never_leaves_account_dir() {
    let (t, _keys, c) = setup();
    let m = json!({"id": "../victim", "label": "x", "isDefault": false, "createdAt": "", "email": null});
    std::fs::write(
        t.path().join("credentials.json"),
        json!({"claudeAccounts": [m], "pendingClaudeAccounts": [m]}).to_string(),
    )
    .unwrap();
    let victim = t.path().join("victim");
    std::fs::create_dir(&victim).unwrap();
    std::fs::write(victim.join("keep"), "x").unwrap();
    assert_eq!(
        c.cancel_cli_account("claude", "../victim").unwrap_err(),
        MISSING
    );
    assert_eq!(c.delete("claude", "../victim").unwrap_err(), MISSING);
    assert_eq!(c.claude_config_dir(Some("../victim")).unwrap_err(), MISSING);
    assert!(victim.join("keep").exists());
}

#[test]
fn codex_account_reads_only_its_auth_json() {
    use ieum_desktop_lib::sub_usage::codex_auth_file;
    let t = tempfile::tempdir().unwrap();
    // 빈 계정 폴더: 이 Mac의 `~/.codex`로 넘어가지 않는다.
    assert!(codex_auth_file(Some(t.path())).is_none());
    std::fs::write(
        t.path().join("auth.json"),
        json!({"tokens": {"access_token": "at", "account_id": "acc",
            "id_token": "e30.eyJlbWFpbCI6ImNAeC5jb20ifQ.sig"}})
        .to_string(),
    )
    .unwrap();
    let a = codex_auth_file(Some(t.path())).unwrap();
    assert!(format!("{a:?}").contains("c@x.com"), "{a:?}");
}

#[tokio::test]
async fn pending_account_hidden_until_confirmed() {
    let (t, c) = setup_spaced();
    let (usage, seen) = fake_usage(t.path());
    let b = c.begin_cli_account("claude", "회사").unwrap();
    let ids = |c: &AccountCredentials| -> Vec<String> {
        c.list().claude_accounts.into_iter().map(|a| a.id).collect()
    };
    assert_eq!(ids(&c), ["system"]);
    // 노드에서도 고를 수 없다.
    assert_eq!(c.claude_config_dir(Some(&b.id)).unwrap_err(), MISSING);
    // 아직 로그인 전: 그대로 대기.
    let u = confirm_cli_account(c.clone(), &usage, "claude", &b.id)
        .await
        .unwrap();
    assert_eq!(u.state, "not_logged_in");
    assert_eq!(ids(&c), ["system"]);
    // 다시 [확인]하면 캐시된 not_logged_in이 아니라 토큰을 새로 읽는다.
    confirm_cli_account(c.clone(), &usage, "claude", &b.id)
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    // 취소하면 폴더째 지운다.
    let dir = c.pending_dir("claude", &b.id).unwrap();
    c.cancel_cli_account("claude", &b.id).unwrap();
    assert!(!dir.exists());
    assert!(
        confirm_cli_account(c.clone(), &usage, "claude", &b.id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn confirm_reads_usage_in_account_dir() {
    let (t, c) = setup_spaced();
    let (usage, seen) = fake_usage(t.path());
    let b = c.begin_cli_account("claude", "회사").unwrap();
    let dir = c.pending_dir("claude", &b.id).unwrap();
    // 로그인하면 claude가 설정 폴더에 계정을 적는다.
    std::fs::write(
        dir.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"work@corp.com"}}"#,
    )
    .unwrap();
    let u = confirm_cli_account(c.clone(), &usage, "claude", &b.id)
        .await
        .unwrap();
    assert_eq!(u.account_email.as_deref(), Some("work@corp.com"));
    assert_eq!(seen.lock().unwrap().as_slice(), [Some(dir.clone())]);
    let list = c.list().claude_accounts;
    let a = list.iter().find(|a| a.id == b.id).unwrap();
    assert_eq!(
        (a.label.as_str(), a.email.as_deref(), a.is_default),
        ("회사", Some("work@corp.com"), false)
    );
    // 고른 노드는 그 폴더로, 기본으로 바꾸면 비운 노드도 그 폴더로.
    assert_eq!(c.claude_config_dir(Some(&b.id)).unwrap(), Some(dir.clone()));
    assert_eq!(c.claude_config_dir(None).unwrap(), None);
    c.set_default("claude", &b.id).unwrap();
    assert_eq!(c.claude_config_dir(None).unwrap(), Some(dir.clone()));
    assert_eq!(c.claude_config_dir(Some("system")).unwrap(), None);
    // 지우면 메타와 폴더가 함께 사라지고, 고른 노드는 실패.
    c.delete("claude", &b.id).unwrap();
    assert!(!dir.exists());
    assert_eq!(c.claude_config_dir(Some(&b.id)).unwrap_err(), MISSING);
    assert_eq!(c.claude_config_dir(None).unwrap(), None);
}

#[test]
fn delete_system_account_rejected() {
    let (_t, c) = setup_spaced();
    for kind in ["claude", "codex"] {
        assert!(c.delete(kind, "system").is_err());
        assert_eq!(c.list().claude_accounts[0].id, "system");
    }
}

#[test]
fn hide_system_account_then_restore() {
    let (_t, c) = setup_spaced();
    c.set_system_hidden("claude", true).unwrap();
    let v = c.list();
    assert!(v.claude_accounts.iter().all(|m| m.id != "system"));
    assert_eq!(v.codex_accounts[0].id, "system");
    // 숨긴 로그인을 고른 노드·기본 노드는 다른 계정으로 대신하지 않고 실패.
    assert_eq!(c.claude_config_dir(Some("system")).unwrap_err(), MISSING);
    assert_eq!(c.claude_config_dir(None).unwrap_err(), MISSING);
    assert!(c.set_system_hidden("gemini", true).is_err());
    c.set_system_hidden("claude", false).unwrap();
    let v = c.list();
    assert_eq!(v.claude_accounts[0].id, "system");
    assert!(v.claude_accounts[0].is_default);
    assert_eq!(c.claude_config_dir(None).unwrap(), None);
}

/// `get`을 몇 번 불렀는지 센다.
#[derive(Default)]
struct CountingKeys(std::sync::atomic::AtomicUsize);

impl KeyStore for CountingKeys {
    fn get(&self, _: &str) -> Result<Option<String>, KeyError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), KeyError> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), KeyError> {
        Ok(())
    }
}

#[test]
fn subscription_accounts_never_touch_keychain() {
    let t = tempfile::tempdir().unwrap();
    let keys = Arc::new(CountingKeys::default());
    let c = AccountCredentials::new(t.path().to_path_buf(), K16, keys.clone());
    let v = c.subscription_accounts().unwrap();
    assert_eq!(keys.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    // `system` 줄 규칙은 `list`와 같다.
    let full = c.list();
    assert_eq!(v.claude_accounts, full.claude_accounts);
    assert_eq!(v.codex_accounts, full.codex_accounts);
    assert_eq!(v.claude_accounts[0].id, "system");
    assert!(v.claude_accounts[0].is_default);
    c.set_system_hidden("codex", true).unwrap();
    assert!(c.subscription_accounts().unwrap().codex_accounts.is_empty());
}
