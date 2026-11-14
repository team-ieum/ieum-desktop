//! `ieum` 명령용 유닉스 소켓 서버 — 토큰이 워크플로우를 정하고, 끝난·모르는 토큰은 닿지 않는다.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_desktop_lib::channel::{Channel, Handler, MAX_LINE, Session, socket_path};
use ieum_local::builder::{Code, Op, Reply};
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const W1: &str = "11111111-1111-4111-8111-111111111111";
const W2: &str = "22222222-2222-4222-8222-222222222222";

/// 경로가 짧아야 계정 폴더 안 소켓이 열린다(macOS 104바이트).
fn short() -> TempDir {
    tempfile::Builder::new()
        .prefix("i")
        .tempdir_in("/tmp")
        .unwrap()
}

type Seen = Arc<Mutex<Vec<(Session, Op)>>>;

fn rec() -> (Seen, Handler) {
    let seen: Seen = Arc::default();
    let s = seen.clone();
    let h: Handler = Arc::new(move |session, op| {
        s.lock().unwrap().push((session, op));
        Box::pin(async { Reply::ok(json!({"n": 1})) })
    });
    (seen, h)
}

async fn ask(path: &Path, line: &str) -> Reply {
    let mut s = UnixStream::connect(path).await.unwrap();
    // 서버가 너무 긴 줄을 중간에 끊으면 쓰기가 실패할 수 있다 — 응답은 그래도 온다.
    let _ = s.write_all(format!("{line}\n").as_bytes()).await;
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).await.unwrap();
    serde_json::from_str(&out).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn socket_is_owner_only_and_token_binds_workflow() {
    let (t, (seen, h)) = (short(), rec());
    let ch = Channel::new(h, t.path().join("tmp"));
    let root = t.path().join("acc");
    let sock = ch.open("0123456789abcdef", &root).unwrap();
    assert_eq!(sock, root.join("ieum.sock"));
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode(&root), mode(&sock)), (0o700, 0o600));
    let tk = ch.issue(W1).unwrap();
    assert_eq!(tk.len(), 64);
    assert_ne!(tk, ch.issue(W1).unwrap());
    // args로 다른 워크플로우를 가리켜도 토큰의 워크플로우만 간다.
    let r = ask(
        &sock,
        &format!(r#"{{"token":"{tk}","op":"status","args":{{"workflowId":"{W2}"}}}}"#),
    )
    .await;
    assert_eq!(r.to_line(), r#"{"ok":true,"data":{"n":1}}"#);
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Session {
                account: "0123456789abcdef".into(),
                workflow_id: W1.into()
            },
            Op::Status {}
        )]
    );
    // 계정 폴더 경로가 길어 대체 경로로 가도 소켓이 놓이는 폴더는 본인 전용이다.
    let long = t.path().join("a".repeat(100));
    let sock2 = Channel::new(rec().1, t.path().join("tmp"))
        .open("0123456789abcdef", &long)
        .unwrap();
    assert_eq!(sock2, t.path().join("tmp/ieum-0123456789abcdef/ieum.sock"));
    assert_eq!(
        (mode(sock2.parent().unwrap()), mode(&sock2)),
        (0o700, 0o600)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_missing_unknown_and_revoked_tokens() {
    let (t, (seen, h)) = (short(), rec());
    let ch = Channel::new(h, t.path().join("tmp"));
    assert!(ch.issue(W1).is_err()); // 열기 전
    let sock = ch.open("0123456789abcdef", &t.path().join("acc")).unwrap();
    let (a, b) = (ch.issue(W1).unwrap(), ch.issue(W2).unwrap());
    let code = |r: Reply| r.code;
    assert_eq!(
        code(ask(&sock, r#"{"op":"status"}"#).await),
        Some(Code::Unauthorized)
    );
    assert_eq!(
        code(ask(&sock, r#"{"token":"nope","op":"status"}"#).await),
        Some(Code::Unauthorized)
    );
    ch.revoke(&a);
    assert_eq!(
        code(ask(&sock, &format!(r#"{{"token":"{a}","op":"status"}}"#)).await),
        Some(Code::Unauthorized)
    );
    assert!(
        ask(&sock, &format!(r#"{{"token":"{b}","op":"status"}}"#))
            .await
            .ok
    );
    ch.revoke_workflow(W2); // 워크플로우 삭제
    assert_eq!(
        code(ask(&sock, &format!(r#"{{"token":"{b}","op":"status"}}"#)).await),
        Some(Code::Unauthorized)
    );
    assert_eq!(seen.lock().unwrap().len(), 1); // 거부된 요청은 Handler에 안 간다
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_lines_get_bad_request_and_server_survives() {
    let (t, (_seen, h)) = (short(), rec());
    let ch = Channel::new(h, t.path().join("tmp"));
    let sock = ch.open("0123456789abcdef", &t.path().join("acc")).unwrap();
    let tk = ch.issue(W1).unwrap();
    assert_eq!(ask(&sock, "not json").await.code, Some(Code::BadRequest));
    assert_eq!(
        ask(&sock, &format!(r#"{{"token":"{tk}","op":"enable"}}"#))
            .await
            .code,
        Some(Code::BadRequest)
    );
    let huge = format!(
        r#"{{"token":"{tk}","op":"rename","args":{{"id":"a","label":"{}"}}}}"#,
        "x".repeat(MAX_LINE)
    );
    assert_eq!(ask(&sock, &huge).await.code, Some(Code::BadRequest));
    assert!(
        ask(&sock, &format!(r#"{{"token":"{tk}","op":"status"}}"#))
            .await
            .ok
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_request_does_not_block_other_workflow() {
    // W1 요청은 gate가 풀릴 때까지 기다리는 Handler(폴더 고르기 흉내).
    let gate = Arc::new(tokio::sync::Notify::new());
    let g = gate.clone();
    let h: Handler = Arc::new(move |s, _| {
        let g = g.clone();
        Box::pin(async move {
            if s.workflow_id == W1 {
                g.notified().await
            }
            Reply::ok(())
        })
    });
    let t = short();
    let ch = Channel::new(h, t.path().join("tmp"));
    let sock = ch.open("0123456789abcdef", &t.path().join("acc")).unwrap();
    let (a, b) = (ch.issue(W1).unwrap(), ch.issue(W2).unwrap());
    let s = sock.clone();
    let slow =
        tokio::spawn(async move { ask(&s, &format!(r#"{{"token":"{a}","op":"status"}}"#)).await });
    let fast = tokio::time::timeout(
        Duration::from_secs(2),
        ask(&sock, &format!(r#"{{"token":"{b}","op":"status"}}"#)),
    )
    .await;
    assert!(fast.unwrap().ok);
    assert!(!slow.is_finished());
    gate.notify_one();
    assert!(slow.await.unwrap().ok);
}

#[tokio::test(flavor = "multi_thread")]
async fn close_and_account_switch_drop_tokens_and_socket() {
    let (t, (_seen, h)) = (short(), rec());
    let ch = Channel::new(h, t.path().join("tmp"));
    let s1 = ch.open("0123456789abcdef", &t.path().join("a1")).unwrap();
    let old = ch.issue(W1).unwrap();
    assert_eq!(
        ch.open("0123456789abcdef", &t.path().join("a1")).unwrap(),
        s1
    ); // 같은 계정 = 그대로
    assert!(
        ask(&s1, &format!(r#"{{"token":"{old}","op":"status"}}"#))
            .await
            .ok
    );
    let s2 = ch.open("fedcba9876543210", &t.path().join("a2")).unwrap(); // 계정 바뀜
    assert!(!s1.exists());
    assert_eq!(
        ask(&s2, &format!(r#"{{"token":"{old}","op":"status"}}"#))
            .await
            .code,
        Some(Code::Unauthorized)
    );
    ch.close();
    ch.close();
    assert!(!s2.exists() && ch.issue(W1).is_err());
}

#[test]
fn socket_path_falls_back_when_too_long() {
    let tmp = Path::new("/tmp/x");
    assert_eq!(
        socket_path(Path::new("/h/acc"), tmp, "0123456789abcdef"),
        Path::new("/h/acc/ieum.sock")
    );
    let long = PathBuf::from(format!("/{}", "a".repeat(95))); // + "/ieum.sock" = 106바이트
    assert_eq!(
        socket_path(&long, tmp, "0123456789abcdef"),
        Path::new("/tmp/x/ieum-0123456789abcdef/ieum.sock")
    );
    let edge = PathBuf::from(format!("/{}", "a".repeat(92))); // 정확히 103바이트 = 그대로
    assert_eq!(socket_path(&edge, tmp, "k"), edge.join("ieum.sock"));
}
