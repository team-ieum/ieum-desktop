//! GitHub 이슈 노드. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

mod github_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use github_support::{Log, bind, hang, resp, serve};
use ieum_engine::{GITHUB_EVENTS, Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::github::{GITHUB_WRITE_EVENTS, GithubApi, GithubRunner, GithubToken};
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

struct NotConnected;

#[async_trait]
impl GithubToken for NotConnected {
    async fn token(&self) -> Result<String, NodeError> {
        Err(NodeError {
            message: "GitHub 연결이 필요해요. 설정 > 연동에서 연결해 주세요.".into(),
            code: Some("GITHUB_NOT_CONNECTED".into()),
        })
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

fn issue(labels: Value) -> Value {
    json!({
        "number": 7,
        "html_url": "https://github.com/a/b/issues/7",
        "title": "T",
        "labels": labels,
        "assignees": [],
        "milestone": null
    })
}

#[tokio::test]
async fn create_issue_sends_and_outputs() {
    let (base, log) = server(201, issue(json!([{"name": "bug"}]))).await;
    let out = run(
        &base,
        "github.create_issue",
        json!({"owner": "a", "repo": "b", "title": "T", "body": "", "labels": "bug, x, bug"}),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({
            "number": 7,
            "url": "https://github.com/a/b/issues/7",
            "title": "T",
            "warnings": ["반영되지 않은 라벨: x"]
        })
    );
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].method.as_str(), log[0].path.as_str()),
        ("POST", "/repos/a/b/issues")
    );
    assert_eq!(
        log[0].json(),
        json!({"title": "T", "body": "", "labels": ["bug", "x"]})
    );
}

#[tokio::test]
async fn dropped_ignores_case() {
    let (base, _log) = server(201, issue(json!([{"name": "bug"}]))).await;
    let out = run(
        &base,
        "github.create_issue",
        json!({"owner": "a", "repo": "b", "title": "T", "labels": "Bug"}),
    )
    .await
    .unwrap();
    assert_eq!(out["warnings"], json!([]));
}

#[tokio::test]
async fn issue_no_accepts_number_hash_and_spaces() {
    let (base, log) = server(201, json!({"id": 1, "html_url": "u"})).await;
    for n in [json!(12), json!(" #12 ")] {
        let c = json!({"owner": "a", "repo": "b", "issue_number": n, "body": "hi"});
        run(&base, "github.create_comment", c).await.unwrap();
    }
    for n in [json!("0"), json!("1.5"), json!(true)] {
        let c = json!({"owner": "a", "repo": "b", "issue_number": n, "body": "hi"});
        assert_eq!(
            message(run(&base, "github.create_comment", c).await),
            "issue_number는 양의 정수여야 합니다."
        );
    }
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 2);
    for r in log.iter() {
        assert_eq!(r.path, "/repos/a/b/issues/12/comments");
    }
}

#[tokio::test]
async fn bad_owner_repo_sends_nothing() {
    let (base, log) = server(201, json!({})).await;
    let token = Arc::new(CountingToken::default());
    for (owner, repo) in [("../x", "b"), ("a b", "b"), ("..", "b"), ("a", "a/b")] {
        let c = json!({"owner": owner, "repo": repo, "title": "T", "issue_number": 1, "body": "x", "labels": "l"});
        for event in [
            "github.create_issue",
            "github.add_labels",
            "github.create_comment",
        ] {
            let r = run_full(
                &base,
                token.clone(),
                event,
                c.clone(),
                CancellationToken::new(),
            )
            .await;
            assert_eq!(
                message(r),
                "owner/repo 형식이 올바르지 않습니다.",
                "{event} {owner}/{repo}"
            );
        }
    }
    assert_eq!(log.lock().unwrap().len(), 0);
    assert_eq!(token.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn milestone_must_be_number() {
    let (base, log) = server(201, json!({})).await;
    let c = json!({"owner": "a", "repo": "b", "title": "T", "milestone": "v1"});
    assert_eq!(
        message(run(&base, "github.create_issue", c).await),
        "milestone은 양의 정수여야 합니다."
    );
    assert_eq!(log.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn comment_limits() {
    let (base, log) = server(201, json!({"id": 5, "html_url": "u"})).await;
    let comment =
        |body: String| json!({"owner": "a", "repo": "b", "issue_number": 1, "body": body});
    assert_eq!(
        message(run(&base, "github.create_comment", comment("  ".into())).await),
        "body를 입력해 주세요."
    );
    assert_eq!(
        message(run(&base, "github.create_comment", comment("a".repeat(65537))).await),
        "코멘트는 65536자 이하여야 합니다."
    );
    assert_eq!(log.lock().unwrap().len(), 0);
    let long = "가".repeat(65536);
    let out = run(&base, "github.create_comment", comment(long.clone()))
        .await
        .unwrap();
    assert_eq!(Value::Object(out), json!({"id": 5, "url": "u"}));
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].json(), json!({"body": long}));
}

#[tokio::test]
async fn add_labels_outputs_all_labels() {
    let (base, log) = server(200, json!([{"name": "a"}, {"name": "b"}])).await;
    let c =
        |labels: &str| json!({"owner": "a", "repo": "b", "issue_number": "3", "labels": labels});
    let out = run(&base, "github.add_labels", c("b")).await.unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"number": 3, "labels": ["a", "b"]})
    );
    assert_eq!(
        message(run(&base, "github.add_labels", c(" , ")).await),
        "labels를 입력해 주세요."
    );
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].path, "/repos/a/b/issues/3/labels");
    assert_eq!(log[0].json(), json!({"labels": ["b"]}));
}

#[tokio::test]
async fn token_error_passes_through() {
    let (base, log) = server(201, json!({})).await;
    let e = run_full(
        &base,
        Arc::new(NotConnected),
        "github.create_issue",
        json!({"owner": "a", "repo": "b", "title": "T"}),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code.as_deref(), Some("GITHUB_NOT_CONNECTED"));
    assert_eq!(log.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn cancel_stops() {
    let (l, base) = bind().await;
    hang(l);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        c2.cancel();
    });
    let e = run_full(
        &base,
        Arc::new(FixedToken("t0k")),
        "github.create_issue",
        json!({"owner": "a", "repo": "b", "title": "T"}),
        cancel,
    )
    .await
    .unwrap_err();
    assert_eq!((e.message.as_str(), e.code), ("취소됐어요.", None));
}

#[test]
fn write_events_are_github_events() {
    for e in GITHUB_WRITE_EVENTS {
        assert!(GITHUB_EVENTS.contains(&e), "{e}");
    }
}
