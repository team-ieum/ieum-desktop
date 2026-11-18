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

// ---- submit_pr_review ----

async fn run_sub_with(
    base: &str,
    token: Arc<dyn GithubToken>,
    config: Value,
) -> Result<Map<String, Value>, NodeError> {
    run_event("github.submit_pr_review", base, token, config).await
}

async fn run_sub(base: &str, config: Value) -> Result<Map<String, Value>, NodeError> {
    run_sub_with(base, Arc::new(CountingToken::default()), config).await
}

/// files GET에는 `files`(첫 쪽만), `/user`는 me1, PR 작성자는 `pr_login`.
/// 리뷰 POST는 앞의 `fail_posts`번은 422, 그 뒤 200.
async fn sub_server(files: Vec<Value>, pr_login: &'static str, fail_posts: usize) -> (String, Log) {
    let (l, base) = bind().await;
    let posts = AtomicUsize::new(0);
    let log = serve(l, move |r| {
        let body = if r.method == "POST" {
            if posts.fetch_add(1, Ordering::SeqCst) < fail_posts {
                return resp(422, "", b"{\"message\":\"secret\"}");
            }
            json!({"id": 9, "html_url": "https://g/r/9", "state": "COMMENTED"})
        } else if r.path.starts_with("/repos/a/b/pulls/3/files") {
            Value::Array(if page_of(r) <= 1 {
                files.clone()
            } else {
                vec![]
            })
        } else if r.path == "/user" {
            json!({"login": "me1"})
        } else {
            json!({"number": 3, "user": {"login": pr_login}})
        };
        resp(200, "", body.to_string().as_bytes())
    });
    (base, log)
}

fn posts(log: &Log) -> Vec<Value> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|r| r.method == "POST")
        .map(|r| {
            assert_eq!(r.path, "/repos/a/b/pulls/3/reviews");
            r.json()
        })
        .collect()
}

#[tokio::test]
async fn author_bot_rejected() {
    let (base, log) = sub_server(vec![], "x", 0).await;
    let tok = Arc::new(CountingToken::default());
    let err = run_sub_with(
        &base,
        tok.clone(),
        cfg(json!({"author": "bot", "body": "b"})),
    )
    .await
    .expect_err("실패");
    assert_eq!(
        err.message,
        "데스크톱에서는 내 GitHub 계정으로만 리뷰를 남길 수 있어요."
    );
    assert_eq!(err.code.as_deref(), Some("GITHUB_INPUT"));
    assert_eq!(tok.0.load(Ordering::SeqCst), 0);
    assert_eq!(log.lock().unwrap().len(), 0);
}

#[tokio::test]
async fn comments_unreadable_and_invalid() {
    let (base, log) = sub_server(vec![], "x", 0).await;
    let out = run_sub(&base, cfg(json!({"body": "b", "comments": "{}"})))
        .await
        .unwrap();
    assert_eq!(
        out["warnings"],
        json!(["줄 코멘트를 읽지 못해 본문만 제출했습니다."])
    );
    assert_eq!(posts(&log)[0]["body"], "b\n\n줄 코멘트를 읽지 못했습니다");

    let (base, log) = sub_server(vec![], "x", 0).await;
    let comments =
        json!([{"path": "", "line": 1, "body": "x"}, {"path": "a", "line": "2", "body": "y"}]);
    let out = run_sub(&base, cfg(json!({"comments": comments})))
        .await
        .unwrap();
    assert_eq!(
        out["warnings"],
        json!(["형식이 틀린 줄 코멘트 1개를 버렸습니다."])
    );
    // line "2"는 받는다 — files에 없으니 본문으로.
    assert_eq!(out["moved_comments"], 1);
    assert_eq!(posts(&log)[0]["body"], "### 다른 줄에 대한 의견\n- a:2 — y");
}

#[tokio::test]
async fn inline_vs_moved() {
    let patch = "@@ -1,2 +10,3 @@\n a\n-b\n+c\n d";
    let (base, log) = sub_server(vec![file("a.rs", Some(patch))], "x", 0).await;
    let comments = json!([{"path": "a.rs", "line": 11, "body": "x"}, {"path": "a.rs", "line": 50, "body": "y"}]);
    let out = run_sub(
        &base,
        cfg(json!({"body": "총평", "comments": comments.to_string(), "commit_id": " c1 "})),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"review_id": 9, "url": "https://g/r/9", "state": "COMMENTED",
               "posted_comments": 1, "moved_comments": 1, "warnings": []})
    );
    let sent = &posts(&log)[0];
    assert_eq!(
        *sent,
        json!({"event": "COMMENT",
               "body": "총평\n\n### 다른 줄에 대한 의견\n- a.rs:50 — y",
               "comments": [{"path": "a.rs", "line": 11, "side": "RIGHT", "body": "x"}],
               "commit_id": "c1"})
    );
}

#[tokio::test]
async fn retry_on_422() {
    let (base, log) = sub_server(vec![file("a.rs", Some("@@ -1 +1 @@\n+x"))], "x", 1).await;
    let out = run_sub(
        &base,
        cfg(json!({"comments": [{"path": "a.rs", "line": 1, "body": "x"}]})),
    )
    .await
    .unwrap();
    assert_eq!(
        out["warnings"],
        json!(["GitHub가 줄 코멘트를 받지 않아 1개를 본문으로 옮겨 다시 제출했습니다."])
    );
    assert_eq!(out["posted_comments"], 0);
    assert_eq!(out["moved_comments"], 1);
    let sent = posts(&log);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0]["comments"].as_array().unwrap().len(), 1);
    assert_eq!(sent[1]["comments"], json!([]));
    assert_eq!(sent[1]["body"], "### 다른 줄에 대한 의견\n- a.rs:1 — x");

    // 두 번째도 422면 expect 오류(본문은 싣지 않는다).
    let (base, _) = sub_server(vec![file("a.rs", Some("@@ -1 +1 @@\n+x"))], "x", 2).await;
    let err = run_sub(
        &base,
        cfg(json!({"comments": [{"path": "a.rs", "line": 1, "body": "x"}]})),
    )
    .await
    .expect_err("실패");
    assert_eq!(err.code.as_deref(), Some("GITHUB_INVALID"));
    assert!(!err.message.contains("secret"));
}

#[tokio::test]
async fn self_pr_approve_becomes_comment() {
    let (base, log) = sub_server(vec![], "me1", 0).await;
    let out = run_sub(&base, cfg(json!({"event": "APPROVE", "body": "좋아요"})))
        .await
        .unwrap();
    assert_eq!(
        out["warnings"],
        json!(["자기 PR에는 APPROVE를 할 수 없어 코멘트로 제출했습니다."])
    );
    assert_eq!(posts(&log)[0]["event"], "COMMENT");
    let paths: Vec<String> = log.lock().unwrap().iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        ["/user", "/repos/a/b/pulls/3", "/repos/a/b/pulls/3/reviews"]
    );
}

#[tokio::test]
async fn empty_review_rejected() {
    let (base, log) = sub_server(vec![], "x", 0).await;
    for event in [json!("COMMENT"), json!(""), json!("REQUEST_CHANGES")] {
        let tok = Arc::new(CountingToken::default());
        let err = run_sub_with(
            &base,
            tok.clone(),
            cfg(json!({"event": event, "body": "  "})),
        )
        .await
        .expect_err("실패");
        assert_eq!(err.message, "리뷰 내용이 없습니다.");
        assert_eq!(tok.0.load(Ordering::SeqCst), 0);
    }
    assert_eq!(log.lock().unwrap().len(), 0);

    let out = run_sub(&base, cfg(json!({"event": "APPROVE"})))
        .await
        .unwrap();
    assert_eq!(out["warnings"], json!([]));
    let sent = &posts(&log)[0];
    assert_eq!(sent["event"], "APPROVE");
    assert_eq!(sent["body"], "");

    // 자기 PR이라 COMMENT로 낮춰지면 빈 리뷰가 된다.
    let (base, log) = sub_server(vec![], "me1", 0).await;
    let err = run_sub(&base, cfg(json!({"event": "APPROVE"})))
        .await
        .expect_err("실패");
    assert_eq!(err.message, "리뷰 내용이 없습니다.");
    assert!(posts(&log).is_empty());
}

#[tokio::test]
async fn review_body_cut_chars() {
    let (base, log) = sub_server(vec![], "x", 0).await;
    let out = run_sub(&base, cfg(json!({"body": "가".repeat(70000)})))
        .await
        .unwrap();
    assert_eq!(
        out["warnings"],
        json!(["본문이 65536자를 넘어 잘랐습니다."])
    );
    assert_eq!(
        posts(&log)[0]["body"].as_str().unwrap().chars().count(),
        65536
    );
}

#[tokio::test]
async fn comments_cap_50() {
    let (base, log) = sub_server(vec![], "x", 0).await;
    let many: Vec<Value> = (1..=52)
        .map(|i| json!({"path": "a.rs", "line": i, "body": "x"}))
        .collect();
    let out = run_sub(&base, cfg(json!({"comments": many})))
        .await
        .unwrap();
    assert_eq!(
        out["warnings"],
        json!(["줄 코멘트는 50개까지라 2개를 버렸습니다."])
    );
    assert_eq!(out["moved_comments"], 50);
    assert_eq!(posts(&log).len(), 1);
}

#[tokio::test]
async fn submit_rejects() {
    let (base, log) = sub_server(vec![], "x", 0).await;
    for (c, msg) in [
        (
            json!({"pull_number": "#3"}),
            "pull_number는 양의 정수여야 합니다.",
        ),
        (
            json!({"event": "approve"}),
            "event는 COMMENT, APPROVE, REQUEST_CHANGES 중 하나여야 합니다.",
        ),
        (
            json!({"author": "me "}),
            "데스크톱에서는 내 GitHub 계정으로만 리뷰를 남길 수 있어요.",
        ),
    ] {
        let tok = Arc::new(CountingToken::default());
        let err = run_sub_with(&base, tok.clone(), cfg(c))
            .await
            .expect_err("실패");
        assert_eq!(err.message, msg);
        assert_eq!(err.code.as_deref(), Some("GITHUB_INPUT"));
        assert_eq!(tok.0.load(Ordering::SeqCst), 0);
    }
    assert_eq!(log.lock().unwrap().len(), 0);
}
