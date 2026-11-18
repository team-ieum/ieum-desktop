//! GitHub 저장소·조직·사용자 조회와 조직 멤버 확인 노드. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

mod github_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use github_support::{Log, bind, resp, serve};
use ieum_engine::{Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::github::{GithubApi, GithubRunner, GithubToken};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

struct FixedToken(&'static str);

#[async_trait]
impl GithubToken for FixedToken {
    async fn token(&self) -> Result<String, NodeError> {
        Ok(self.0.into())
    }
}

/// 몇 번 읽혔는지 센다.
#[derive(Default)]
struct CountingToken(AtomicUsize);

#[async_trait]
impl GithubToken for CountingToken {
    async fn token(&self) -> Result<String, NodeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("t0k".into())
    }
}

async fn run_full(
    base: &str,
    token: Arc<dyn GithubToken>,
    event: &str,
    config: Value,
    cancel: CancellationToken,
) -> Result<Map<String, Value>, NodeError> {
    let runner = GithubRunner::new(GithubApi::with_base(base), token);
    let node: Node = serde_json::from_value(json!({"id": "n", "event": event})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    runner
        .run(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            outputs: &Default::default(),
            events: &tx,
            cancel: &cancel,
        })
        .await
}

async fn run(base: &str, event: &str, config: Value) -> Result<Map<String, Value>, NodeError> {
    run_full(
        base,
        Arc::new(FixedToken("t0k")),
        event,
        config,
        CancellationToken::new(),
    )
    .await
}

/// 모든 요청에 같은 응답을 주는 서버.
async fn server(status: u16, body: Value) -> (String, Log) {
    let (l, base) = bind().await;
    let bytes = body.to_string().into_bytes();
    let log = serve(l, move |_| resp(status, "", &bytes));
    (base, log)
}

fn message(r: Result<Map<String, Value>, NodeError>) -> String {
    r.expect_err("실패해야 함").message
}

fn req_paths(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|r| r.path.clone()).collect()
}

#[tokio::test]
async fn find_repo_shape() {
    let body = json!({
        "name": "b", "full_name": "a/b", "owner": {"login": "a"}, "private": true,
        "default_branch": "main", "html_url": "https://github.com/a/b", "description": null,
        "extra": 1
    });
    let (base, log) = server(200, body).await;
    let out = run(
        &base,
        "github.find_repo",
        json!({"owner": "a", "repo": "b"}),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({
            "found": true, "total": 1, "message": "",
            "items": [{
                "name": "b", "full_name": "a/b", "owner": "a", "private": true,
                "default_branch": "main", "url": "https://github.com/a/b", "description": ""
            }]
        })
    );
    assert_eq!(req_paths(&log), ["/repos/a/b"]);
}

#[tokio::test]
async fn lookup_404_is_empty() {
    for (event, cfg) in [
        ("github.find_repo", json!({"owner": "a", "repo": "b"})),
        ("github.find_org", json!({"org": "o"})),
        ("github.find_user", json!({"username": "u"})),
    ] {
        let (base, _) = server(404, json!({"message": "Not Found"})).await;
        let out = run(&base, event, cfg).await.unwrap();
        assert_eq!(
            Value::Object(out),
            json!({"found": false, "total": 0, "items": [], "message": "결과가 없습니다"}),
            "{event}"
        );
    }
}

#[tokio::test]
async fn find_org_shape() {
    let (base, log) = server(
        200,
        json!({"login": "o", "name": null, "description": null, "html_url": "https://github.com/o"}),
    )
    .await;
    let out = run(&base, "github.find_org", json!({"org": "o"}))
        .await
        .unwrap();
    assert_eq!(
        out["items"],
        json!([{"login": "o", "name": null, "description": "", "url": "https://github.com/o"}])
    );
    assert_eq!(req_paths(&log), ["/orgs/o"]);
}

#[tokio::test]
async fn find_user_blank_is_me() {
    let body = json!({"login": "octo", "name": "O", "type": "User", "html_url": "https://github.com/octo"});
    for (cfg, path) in [
        (json!({"username": ""}), "/user"),
        (json!({}), "/user"),
        (json!({"username": "octo"}), "/users/octo"),
    ] {
        let (base, log) = server(200, body.clone()).await;
        let out = run(&base, "github.find_user", cfg).await.unwrap();
        assert_eq!(
            out["items"],
            json!([{"login": "octo", "name": "O", "type": "User", "url": "https://github.com/octo"}])
        );
        assert_eq!(req_paths(&log), [path]);
    }
}

#[tokio::test]
async fn membership_member() {
    let (base, log) = server(204, Value::Null).await;
    let out = run(
        &base,
        "github.check_org_membership",
        json!({"org": "o", "username": "u"}),
    )
    .await
    .unwrap();
    assert_eq!(out["member"], json!(true));
    assert_eq!(out["found"], json!(true));
    assert_eq!(out["total"], json!(1));
    assert_eq!(out["message"], json!(""));
    assert_eq!(out["items"], json!([{"org": "o", "username": "u"}]));
    assert_eq!(req_paths(&log), ["/orgs/o/members/u"]);
}

#[tokio::test]
async fn membership_not_member() {
    let (base, _) = server(404, Value::Null).await;
    let out = run(
        &base,
        "github.check_org_membership",
        json!({"org": "o", "username": "u"}),
    )
    .await
    .unwrap();
    assert_eq!(out["member"], json!(false));
    assert_eq!(out["found"], json!(false));
    assert_eq!(out["items"], json!([]));
    assert_eq!(out["message"], json!("멤버가 아닙니다"));
}

#[tokio::test]
async fn membership_redirect_once() {
    let (l, base) = bind().await;
    let log = serve(l, |r| {
        if r.path.contains("/public_members/") {
            resp(204, "", b"")
        } else {
            resp(302, "location: https://example.invalid/x\r\n", b"")
        }
    });
    let out = run(
        &base,
        "github.check_org_membership",
        json!({"org": "o", "username": "u"}),
    )
    .await
    .unwrap();
    assert_eq!(out["member"], json!(true));
    assert_eq!(
        req_paths(&log),
        ["/orgs/o/members/u", "/orgs/o/public_members/u"]
    );

    // 두 번째도 302면 더 따라가지 않고 오류.
    let (base, log) = server(302, Value::Null).await;
    let e = run(
        &base,
        "github.check_org_membership",
        json!({"org": "o", "username": "u"}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code.as_deref(), Some("GITHUB_MOVED"));
    assert_eq!(log.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn lookup_bad_input_reads_no_token() {
    let long = "a".repeat(40);
    let cases = [
        (
            "github.find_repo",
            json!({"owner": "../x", "repo": "b"}),
            "owner/repo 형식이 올바르지 않습니다.",
        ),
        (
            "github.find_org",
            json!({"org": "a b"}),
            "org 형식이 올바르지 않습니다.",
        ),
        (
            "github.find_user",
            json!({"username": long}),
            "username 형식이 올바르지 않습니다.",
        ),
        (
            "github.check_org_membership",
            json!({"org": "o", "username": "a b"}),
            "org/username 형식이 올바르지 않습니다.",
        ),
    ];
    for (event, cfg, want) in cases {
        let (base, log) = server(200, json!({})).await;
        let tok = Arc::new(CountingToken::default());
        let r = run_full(&base, tok.clone(), event, cfg, CancellationToken::new()).await;
        assert_eq!(message(r), want, "{event}");
        assert_eq!(tok.0.load(Ordering::SeqCst), 0, "{event}");
        assert!(log.lock().unwrap().is_empty(), "{event}");
    }
}

#[tokio::test]
async fn lookup_403_reads_message() {
    let (base, _) = server(403, json!({"message": "secret body"})).await;
    let m = message(
        run(
            &base,
            "github.find_repo",
            json!({"owner": "a", "repo": "b"}),
        )
        .await,
    );
    assert_eq!(m, "이 저장소·조직을 읽을 권한이 없어요.");
}
