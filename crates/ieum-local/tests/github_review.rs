//! GitHub PR 변경 내용 노드. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

mod github_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use github_support::{Log, Req, bind, resp, serve};
use ieum_engine::{Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::github::{GithubApi, GithubRunner, GithubToken};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

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

async fn run_with(
    base: &str,
    token: Arc<dyn GithubToken>,
    config: Value,
) -> Result<Map<String, Value>, NodeError> {
    run_event("github.get_pr_files", base, token, config).await
}

async fn run_event(
    event: &str,
    base: &str,
    token: Arc<dyn GithubToken>,
    config: Value,
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
            cancel: &CancellationToken::new(),
        })
        .await
}

async fn run(base: &str, config: Value) -> Result<Map<String, Value>, NodeError> {
    run_with(base, Arc::new(CountingToken::default()), config).await
}

fn cfg(extra: Value) -> Value {
    let mut c = json!({"owner": "a", "repo": "b", "pull_number": 3});
    c.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    c
}

fn page_of(r: &Req) -> usize {
    let q = r.path.split_once('?').map(|(_, q)| q).unwrap_or("");
    q.split('&')
        .find_map(|kv| kv.strip_prefix("page="))
        .map(|p| p.parse().unwrap())
        .unwrap_or(0)
}

/// PR GET에는 head.sha, files GET에는 `pages(page)`가 준 목록.
async fn server(pages: impl Fn(usize) -> Value + Send + 'static) -> (String, Log) {
    let (l, base) = bind().await;
    let log = serve(l, move |r| {
        let body = if r.path.starts_with("/repos/a/b/pulls/3/files") {
            pages(page_of(r))
        } else {
            json!({"number": 3, "head": {"sha": "abc123"}})
        };
        resp(200, "", body.to_string().as_bytes())
    });
    (base, log)
}

fn file(name: &str, patch: Option<&str>) -> Value {
    let mut f = json!({"filename": name, "status": "modified", "additions": 1, "deletions": 0, "changes": 1, "sha": "x"});
    if let Some(p) = patch {
        f["patch"] = p.into();
    }
    f
}

fn files_server(files: Vec<Value>) -> impl Fn(usize) -> Value {
    move |p| {
        if p == 1 {
            Value::Array(files.clone())
        } else {
            json!([])
        }
    }
}

#[tokio::test]
async fn pr_files_filters_and_reasons() {
    let big = "x".repeat(2000);
    let (base, log) = server(files_server(vec![
        file("a.lock", Some("@@ -1 +1 @@\n+l")),
        file("src/a.rs", Some("@@ -1 +1 @@\n+x")),
        file("img.png", None),
        file("src/b.rs", Some(&big)),
    ]))
    .await;
    let out = run(&base, cfg(json!({"max_diff_chars": 1000})))
        .await
        .unwrap();
    assert_eq!(
        out["skipped"],
        json!([
            {"filename": "a.lock", "reason": "filtered"},
            {"filename": "img.png", "reason": "no_patch"},
            {"filename": "src/b.rs", "reason": "max_diff"},
        ])
    );
    assert_eq!(out["diff"], "--- src/a.rs\n@@ -1 +1 @@\n+x\n");
    assert_eq!(out["head_sha"], "abc123");
    assert_eq!(out["found"], true);
    assert_eq!(out["total"], 4);
    assert_eq!(out["truncated"], false);
    assert_eq!(
        out["items"][0],
        json!({"filename": "a.lock", "status": "modified", "additions": 1, "deletions": 0, "changes": 1})
    );
    assert_eq!(out["items"].as_array().unwrap().len(), 4);
    assert!(!out.contains_key("message"));
    let reqs = log.lock().unwrap().clone();
    assert_eq!(reqs[0].path, "/repos/a/b/pulls/3");
    assert_eq!(reqs[1].path, "/repos/a/b/pulls/3/files?per_page=100&page=1");
}

#[tokio::test]
async fn pr_files_include_and_empty_exclude() {
    let (base, _) = server(files_server(vec![
        file("a.lock", Some("+l")),
        file("src/a.rs", Some("+x")),
        file("docs/r.md", Some("+d")),
    ]))
    .await;
    // 제외 칸이 빈 글이면 제외 없음 — a.lock도 diff에 들어간다.
    let out = run(
        &base,
        cfg(json!({"exclude_paths": "", "include_paths": " *.lock , src/** ,"})),
    )
    .await
    .unwrap();
    assert_eq!(out["diff"], "--- a.lock\n+l\n--- src/a.rs\n+x\n");
    assert_eq!(
        out["skipped"],
        json!([{"filename": "docs/r.md", "reason": "filtered"}])
    );
}

#[tokio::test]
async fn pr_files_max_files() {
    let (base, _) = server(files_server(vec![
        file("a.rs", Some("+a")),
        file("b.rs", Some("+b")),
        file("c.rs", None),
    ]))
    .await;
    let out = run(&base, cfg(json!({"max_files": 1}))).await.unwrap();
    assert_eq!(out["diff"], "--- a.rs\n+a\n");
    assert_eq!(
        out["skipped"],
        json!([
            {"filename": "b.rs", "reason": "max_files"},
            {"filename": "c.rs", "reason": "max_files"},
        ])
    );
}

#[tokio::test]
async fn diff_budget_counts_chars() {
    let han = "가".repeat(900);
    let (base, _) = server(files_server(vec![
        file("a.rs", Some(&han)),
        file("b.rs", Some(&han)),
        file("c.rs", Some("+c")),
    ]))
    .await;
    let out = run(&base, cfg(json!({"max_diff_chars": 1000})))
        .await
        .unwrap();
    // 한글 900자는 바이트로 2700이지만 글자로 세면 들어간다.
    assert_eq!(out["diff"], format!("--- a.rs\n{han}\n"));
    // 한 번 넘치면 이후 patch 있는 파일은 모두 max_diff.
    assert_eq!(
        out["skipped"],
        json!([
            {"filename": "b.rs", "reason": "max_diff"},
            {"filename": "c.rs", "reason": "max_diff"},
        ])
    );
}

fn full_page(p: usize) -> Value {
    Value::Array(
        (0..100)
            .map(|i| file(&format!("p{p}/f{i}.rs"), None))
            .collect(),
    )
}

#[tokio::test]
async fn pr_files_pages() {
    let (base, log) = server(|p| {
        if p < 3 {
            full_page(p)
        } else {
            Value::Array((0..5).map(|i| file(&format!("z{i}"), None)).collect())
        }
    })
    .await;
    let out = run(&base, cfg(json!({}))).await.unwrap();
    assert_eq!(
        (out["total"].clone(), out["truncated"].clone()),
        (json!(205), json!(false))
    );
    let pages: Vec<usize> = log.lock().unwrap()[1..].iter().map(page_of).collect();
    assert_eq!(pages, vec![1, 2, 3]);

    let (base, log) = server(full_page).await;
    let out = run(&base, cfg(json!({}))).await.unwrap();
    assert_eq!(
        (out["total"].clone(), out["truncated"].clone()),
        (json!(1000), json!(true))
    );
    assert_eq!(log.lock().unwrap().len() - 1, 10);
}

#[tokio::test]
async fn pr_files_rejects() {
    let (base, log) = server(|_| json!([])).await;
    for n in [json!("#3"), json!(0), json!("0")] {
        let tok = Arc::new(CountingToken::default());
        let err = run_with(&base, tok.clone(), cfg(json!({"pull_number": n})))
            .await
            .expect_err("실패해야 함");
        assert_eq!(err.message, "pull_number는 양의 정수여야 합니다.");
        assert_eq!(err.code.as_deref(), Some("GITHUB_INPUT"));
        assert_eq!(tok.0.load(Ordering::SeqCst), 0);
    }
    assert_eq!(log.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn pr_files_http_error_has_no_body() {
    let (l, base) = bind().await;
    serve(l, |_| resp(404, "", b"{\"message\":\"secret body\"}"));
    let err = run(&base, cfg(json!({}))).await.expect_err("실패해야 함");
    assert_eq!(err.code.as_deref(), Some("GITHUB_NOT_FOUND"));
    assert!(!err.message.contains("secret"));
}

// ---- get_pr_reviews ----

async fn run_rv_with(
    base: &str,
    token: Arc<dyn GithubToken>,
    config: Value,
) -> Result<Map<String, Value>, NodeError> {
    run_event("github.get_pr_reviews", base, token, config).await
}

async fn run_rv(base: &str, config: Value) -> Result<Map<String, Value>, NodeError> {
    run_rv_with(base, Arc::new(CountingToken::default()), config).await
}

fn review(id: u64, login: &str, state: &str, body: &str) -> Value {
    json!({"id": id, "user": {"login": login, "type": "User"}, "state": state, "body": body,
           "submitted_at": "2025-01-01T00:00:00Z", "commit_id": "c1"})
}

/// reviews GET에는 `reviews`(첫 쪽만), comments GET에는 `comments`(첫 쪽만).
async fn rv_server(reviews: Vec<Value>, comments: Vec<Value>) -> (String, Log) {
    let (l, base) = bind().await;
    let log = serve(l, move |r| {
        let all = if r.path.starts_with("/repos/a/b/pulls/3/reviews") {
            &reviews
        } else {
            &comments
        };
        let body = if page_of(r) <= 1 { all.clone() } else { vec![] };
        resp(200, "", Value::Array(body).to_string().as_bytes())
    });
    (base, log)
}

#[tokio::test]
async fn reviews_last_decision_per_reviewer() {
    let (base, _) = rv_server(
        vec![
            review(1, "u1", "APPROVED", "a"),
            review(2, "u1", "COMMENTED", "b"),
            review(3, "u2", "CHANGES_REQUESTED", "c"),
            review(4, "u2", "APPROVED", "d"),
            review(5, "u3", "DISMISSED", "e"),
            review(6, "u4", "PENDING", "f"),
        ],
        vec![],
    )
    .await;
    let out = run_rv(&base, cfg(json!({}))).await.unwrap();
    assert_eq!(out["approved_count"], 2);
    assert_eq!(out["changes_requested_count"], 0);
    let ids: Vec<_> = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].clone())
        .collect();
    assert_eq!(ids, vec![json!(5), json!(4), json!(3), json!(2), json!(1)]);
    assert_eq!(out["items"][0]["author"], "u3");
    assert_eq!(out["items"][0]["author_type"], "human");
    assert_eq!(out["found"], true);
    assert_eq!(out["total"], 5);
    assert_eq!(out["message"], "");
    assert_eq!(out["comments"], json!([]));
    assert_eq!(out["truncated"], false);
}

#[tokio::test]
async fn reviews_states_filter_and_cap() {
    let many: Vec<Value> = (0..60).map(|i| review(i, "u", "COMMENTED", "")).collect();
    let (base, _) = rv_server(many, vec![]).await;
    let out = run_rv(&base, cfg(json!({"states": "commented"})))
        .await
        .unwrap();
    assert_eq!(out["items"].as_array().unwrap().len(), 50);
    assert_eq!(out["total"], 50);
    assert_eq!(out["truncated"], true);
    // 다른 상태만 고르면 0건.
    let out = run_rv(&base, cfg(json!({"states": "approved"})))
        .await
        .unwrap();
    assert_eq!(out["found"], false);
    assert_eq!(out["message"], "결과가 없습니다");
}

#[tokio::test]
async fn reviews_author_type_bot() {
    let mut bot = review(1, "bot[bot]", "APPROVED", "");
    bot["user"]["type"] = "Bot".into();
    let (base, _) = rv_server(vec![bot, review(2, "me", "APPROVED", "")], vec![]).await;
    let out = run_rv(&base, cfg(json!({"author_type": " bot "})))
        .await
        .unwrap();
    assert_eq!(out["total"], 1);
    assert_eq!(out["items"][0]["author_type"], "bot");
    assert_eq!(out["approved_count"], 1);
    let err = run_rv(&base, cfg(json!({"author_type": "Bot"})))
        .await
        .expect_err("실패");
    assert_eq!(
        err.message,
        "author_type은 human, bot, all 중 하나여야 합니다."
    );
    assert_eq!(err.code.as_deref(), Some("GITHUB_INPUT"));
}

#[tokio::test]
async fn reviews_budget() {
    let long = "가".repeat(3000);
    let (base, _) = rv_server(vec![review(1, "u", "COMMENTED", &long)], vec![]).await;
    let out = run_rv(&base, cfg(json!({}))).await.unwrap();
    assert_eq!(
        out["items"][0]["body"].as_str().unwrap().chars().count(),
        2000
    );
    assert_eq!(out["truncated"], true);

    // 2000자 리뷰 75개(50개 상한 → 100000자) + 코멘트 1000자 60개 → 예산 150000에서 코멘트 50개째부터 잘림.
    let rv: Vec<Value> = (0..75)
        .map(|i| review(i, "u", "COMMENTED", &"a".repeat(2000)))
        .collect();
    let cm: Vec<Value> = (0..60)
        .map(|i| json!({"path": "p", "line": i, "body": "b".repeat(1000), "user": {"login": "x"}}))
        .collect();
    let (base, _) = rv_server(rv, cm).await;
    let out = run_rv(&base, cfg(json!({}))).await.unwrap();
    let bodies: Vec<usize> = out["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["body"].as_str().unwrap().chars().count())
        .collect();
    assert_eq!(bodies[49], 1000);
    assert_eq!(bodies[50], 0);
    assert_eq!(out["comments"][59]["body"], "");
    assert_eq!(out["truncated"], true);
}

#[tokio::test]
async fn reviews_comments_desc_and_line_fallback() {
    let cm = vec![
        json!({"path": "p", "line": null, "original_line": 9, "body": "hi",
                         "user": {"login": "x", "type": "User"}, "created_at": "t"}),
    ];
    let (base, log) = rv_server(vec![], cm).await;
    let out = run_rv(&base, cfg(json!({}))).await.unwrap();
    assert_eq!(
        out["comments"],
        json!([{"path": "p", "line": 9, "body": "hi", "author": "x", "created_at": "t"}])
    );
    let reqs = log.lock().unwrap().clone();
    assert_eq!(
        reqs[0].path,
        "/repos/a/b/pulls/3/reviews?per_page=100&page=1"
    );
    assert!(
        reqs[1].path.contains("sort=created&direction=desc"),
        "{}",
        reqs[1].path
    );

    for v in [json!("false"), json!(false)] {
        let (base, log) = rv_server(vec![], vec![]).await;
        let out = run_rv(&base, cfg(json!({"include_comments": v})))
            .await
            .unwrap();
        assert_eq!(out["comments"], json!([]));
        assert_eq!(log.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn reviews_rejects() {
    let (base, log) = rv_server(vec![], vec![]).await;
    for (c, msg) in [
        (
            json!({"states": "PENDING"}),
            "states는 APPROVED, CHANGES_REQUESTED, COMMENTED, DISMISSED 중에서 골라야 합니다.",
        ),
        (
            json!({"pull_number": 0}),
            "pull_number는 양의 정수여야 합니다.",
        ),
    ] {
        let tok = Arc::new(CountingToken::default());
        let err = run_rv_with(&base, tok.clone(), cfg(c))
            .await
            .expect_err("실패");
        assert_eq!(err.message, msg);
        assert_eq!(err.code.as_deref(), Some("GITHUB_INPUT"));
        assert_eq!(tok.0.load(Ordering::SeqCst), 0);
    }
    assert_eq!(log.lock().unwrap().len(), 0);
}
