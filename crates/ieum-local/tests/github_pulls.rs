//! GitHub PR 만들기·수정·리뷰어 지정·찾거나 만들기 노드. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

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

fn bodies(log: &Log) -> Vec<Value> {
    log.lock().unwrap().iter().map(|r| r.json()).collect()
}

fn pr_json() -> Value {
    json!({"number": 5, "html_url": "https://github.com/a/b/pull/5", "title": " T ", "state": "open", "draft": false, "extra": 1})
}

fn cfg(extra: Value) -> Value {
    let mut c =
        json!({"owner": "a", "repo": "b", "title": " T ", "head": "feat/x", "base": "main"});
    c.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    c
}

#[tokio::test]
async fn create_pr_sends_and_outputs() {
    let (base, log) = server(201, pr_json()).await;
    let out = run(&base, "github.create_pull_request", cfg(json!({})))
        .await
        .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"number": 5, "url": "https://github.com/a/b/pull/5", "title": " T ", "state": "open", "draft": false})
    );
    let r = log.lock().unwrap()[0].clone();
    assert_eq!(
        (r.method.as_str(), r.path.as_str()),
        ("POST", "/repos/a/b/pulls")
    );
    assert_eq!(
        r.json(),
        json!({"title": " T ", "head": "feat/x", "base": "main", "body": "", "draft": false, "maintainer_can_modify": true})
    );
}

#[tokio::test]
async fn create_pr_bool_strings() {
    let (base, log) = server(201, pr_json()).await;
    run(
        &base,
        "github.create_pull_request",
        cfg(json!({"draft": "true", "maintainer_can_modify": "false"})),
    )
    .await
    .unwrap();
    let b = &bodies(&log)[0];
    assert_eq!(
        (&b["draft"], &b["maintainer_can_modify"]),
        (&json!(true), &json!(false))
    );
}

#[tokio::test]
async fn pr_refs_rules() {
    for (head, base_ref, ok) in [
        ("u:feat", "main", true),
        ("feat/x", "u:main", false),
        ("a b:x", "main", false),
        (" feat ", "main", true),
    ] {
        let (base, log) = server(201, pr_json()).await;
        let tok = Arc::new(CountingToken::default());
        let r = run_full(
            &base,
            tok.clone(),
            "github.create_pull_request",
            cfg(json!({"head": head, "base": base_ref})),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(r.is_ok(), ok, "{head} / {base_ref}");
        if ok {
            assert_eq!(bodies(&log)[0]["head"], head.trim());
        } else {
            assert_eq!(log.lock().unwrap().len(), 0);
            assert_eq!(tok.0.load(Ordering::SeqCst), 0);
        }
    }
    let (base, _) = server(201, pr_json()).await;
    assert_eq!(
        message(
            run(
                &base,
                "github.create_pull_request",
                cfg(json!({"head": "a b:x"}))
            )
            .await
        ),
        "head 브랜치 형식이 올바르지 않습니다."
    );
    assert_eq!(
        message(
            run(
                &base,
                "github.create_pull_request",
                cfg(json!({"base": "u:main"}))
            )
            .await
        ),
        "base 브랜치 형식이 올바르지 않습니다."
    );
}

#[tokio::test]
async fn create_pr_blank_title() {
    let (base, _) = server(201, pr_json()).await;
    assert_eq!(
        message(
            run(
                &base,
                "github.create_pull_request",
                cfg(json!({"title": "  "}))
            )
            .await
        ),
        "title을 입력해 주세요."
    );
}

#[tokio::test]
async fn update_pr_rules() {
    let (base, log) = server(200, pr_json()).await;
    let u = |extra: Value| {
        let mut c = json!({"owner": "a", "repo": "b", "pull_number": "#7"});
        c.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        c
    };
    assert_eq!(
        message(
            run(
                &base,
                "github.update_pull_request",
                u(json!({"title": " ", "body": ""}))
            )
            .await
        ),
        "바꿀 칸이 없습니다."
    );
    assert_eq!(
        message(
            run(
                &base,
                "github.update_pull_request",
                u(json!({"state": "merged"}))
            )
            .await
        ),
        "state는 open, closed 중 하나여야 합니다."
    );
    assert_eq!(
        message(
            run(
                &base,
                "github.update_pull_request",
                u(json!({"title": "x", "pull_number": "0"}))
            )
            .await
        ),
        "pull_number는 양의 정수여야 합니다."
    );
    assert_eq!(log.lock().unwrap().len(), 0);
    let out = run(
        &base,
        "github.update_pull_request",
        u(json!({"title": "새 제목", "body": ""})),
    )
    .await
    .unwrap();
    assert!(!out.contains_key("draft"));
    let r = log.lock().unwrap()[0].clone();
    assert_eq!(
        (r.method.as_str(), r.path.as_str()),
        ("PATCH", "/repos/a/b/pulls/7")
    );
    assert_eq!(r.json(), json!({"title": "새 제목"}));
}

#[tokio::test]
async fn update_pr_base_like_create() {
    let (base, log) = server(200, pr_json()).await;
    let u = |b: &str| json!({"owner": "a", "repo": "b", "pull_number": "7", "base": b});
    assert_eq!(
        message(run(&base, "github.update_pull_request", u("u:main")).await),
        "base 브랜치 형식이 올바르지 않습니다."
    );
    assert_eq!(log.lock().unwrap().len(), 0);
    run(&base, "github.update_pull_request", u("\tmain "))
        .await
        .unwrap();
    assert_eq!(log.lock().unwrap()[0].json(), json!({"base": "main"}));
}

#[tokio::test]
async fn request_reviewers_output() {
    let (base, log) = server(
        201,
        json!({"requested_reviewers": [{"login": "x"}, {"login": "z"}]}),
    )
    .await;
    let c = |r: &str| json!({"owner": "a", "repo": "b", "pull_number": 7, "reviewers": r});
    let out = run(&base, "github.request_reviewers", c("x, y,x"))
        .await
        .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"number": 7, "requested_reviewers": ["x", "z"]})
    );
    assert_eq!(bodies(&log)[0], json!({"reviewers": ["x", "y"]}));
    assert_eq!(
        log.lock().unwrap()[0].path,
        "/repos/a/b/pulls/7/requested_reviewers"
    );
    assert_eq!(
        message(run(&base, "github.request_reviewers", c("bad name")).await),
        "reviewers는 쉼표로 구분한 GitHub 사용자 이름이어야 합니다."
    );
}

#[tokio::test]
async fn find_or_create_found_and_created() {
    let (base, log) = server(200, json!([pr_json()])).await;
    let out = run(&base, "github.find_or_create_pull_request", cfg(json!({})))
        .await
        .unwrap();
    assert_eq!(out["created"], json!(false));
    assert_eq!(out["number"], json!(5));
    let reqs = log.lock().unwrap().clone();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].path,
        "/repos/a/b/pulls?head=a%3Afeat%2Fx&base=main&state=open&per_page=1"
    );

    let (l, base) = bind().await;
    let log = serve(l, |r| {
        if r.method == "GET" {
            resp(200, "", b"[]")
        } else {
            resp(201, "", pr_json().to_string().as_bytes())
        }
    });
    let out = run(
        &base,
        "github.find_or_create_pull_request",
        cfg(json!({"head": "feat"})),
    )
    .await
    .unwrap();
    assert_eq!(out["created"], json!(true));
    let reqs = log.lock().unwrap().clone();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[1].method, "POST");
    assert!(reqs[0].path.contains("head=a%3Afeat&"), "{}", reqs[0].path);
}

#[tokio::test]
async fn pulls_422_is_invalid() {
    let (base, _) = server(422, json!({"message": "secret"})).await;
    let e = run(&base, "github.create_pull_request", cfg(json!({})))
        .await
        .unwrap_err();
    assert_eq!(e.code.as_deref(), Some("GITHUB_INVALID"));
    assert!(!e.message.contains("secret"));
}

fn find_cfg(by: &str, value: Value) -> Value {
    json!({"owner": "a", "repo": "b", "search_by": by, "value": value})
}

fn query(r: &github_support::Req) -> Vec<(String, String)> {
    let (_, qs) = r.path.split_once('?').unwrap();
    reqwest::Url::parse(&format!("http://x/?{qs}"))
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn find_pr_by_number() {
    let (base, log) = server(
        200,
        json!({"number": 3, "title": "t", "state": "open", "html_url": "u", "draft": true,
               "user": {"login": "au"}, "created_at": "2026-01-01T00:00:00Z"}),
    )
    .await;
    let out = run(
        &base,
        "github.find_pull_request",
        find_cfg("number", json!("#3")),
    )
    .await
    .unwrap();
    assert_eq!(log.lock().unwrap()[0].path, "/repos/a/b/pulls/3");
    assert_eq!(out["found"], true);
    let keys: Vec<&String> = out["items"][0].as_object().unwrap().keys().collect();
    let mut want = vec![
        "number",
        "title",
        "state",
        "url",
        "draft",
        "author",
        "created_at",
    ];
    want.sort();
    let mut got: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    got.sort();
    assert_eq!(got, want);
    let (base, _) = server(404, json!({})).await;
    let out = run(
        &base,
        "github.find_pull_request",
        find_cfg("number", json!(3)),
    )
    .await
    .unwrap();
    assert_eq!(out["found"], false);
    assert_eq!(out["message"], "결과가 없습니다");
}

#[tokio::test]
async fn find_pr_by_branch_query() {
    let (base, log) = server(200, json!([])).await;
    run(
        &base,
        "github.find_pull_request",
        find_cfg("branch", json!("feat")),
    )
    .await
    .unwrap();
    let l = log.lock().unwrap();
    assert!(l[0].path.starts_with("/repos/a/b/pulls?"));
    assert_eq!(
        query(&l[0]),
        vec![
            ("head".to_string(), "a:feat".to_string()),
            ("state".to_string(), "open".to_string()),
            ("per_page".to_string(), "20".to_string())
        ]
    );
}

#[tokio::test]
async fn find_pr_search_filters() {
    let hit = |url: &str, pr: bool| {
        let mut v = json!({"number": 1, "title": "t", "state": "open", "html_url": "u",
            "user": {"login": "au"}, "created_at": "c", "repository_url": url});
        if pr {
            v["pull_request"] = json!({});
        }
        v
    };
    let (base, log) = server(
        200,
        json!({"items": [
            hit("https://api.github.com/repos/A/B", true),
            hit("https://api.github.com/repos/a/c", true),
            hit("https://api.github.com/repos/a/b", false),
        ]}),
    )
    .await;
    let mut c = find_cfg("title", json!("fix"));
    c["state"] = json!("all");
    let out = run(&base, "github.find_pull_request", c).await.unwrap();
    assert_eq!(out["total"], 1);
    let mut c = find_cfg("author", json!("me"));
    c["state"] = json!("closed");
    run(&base, "github.find_pull_request", c).await.unwrap();
    let l = log.lock().unwrap();
    assert!(l[0].path.starts_with("/search/issues?"));
    assert_eq!(query(&l[0])[0].1, "fix in:title repo:a/b is:pr");
    assert_eq!(query(&l[1])[0].1, "author:me is:pr repo:a/b state:closed");
}

#[tokio::test]
async fn find_pr_rejects() {
    let (base, log) = server(200, json!([])).await;
    let long = "가".repeat(201);
    let mut bad_state = find_cfg("title", json!("x"));
    bad_state["state"] = json!("x");
    let cases = [
        (
            find_cfg("label", json!("x")),
            "search_by는 number, branch, title, body, author 중 하나여야 합니다.",
        ),
        (bad_state, "state는 open, closed, all 중 하나여야 합니다."),
        (find_cfg("title", json!("")), "value를 입력해 주세요."),
        (
            find_cfg("number", json!("abc")),
            "PR 번호는 양의 정수여야 합니다.",
        ),
        (
            find_cfg("author", json!("a b")),
            "GitHub 사용자 이름 형식이 올바르지 않습니다.",
        ),
        (
            find_cfg("title", json!(long)),
            "검색어는 200자 이하여야 합니다.",
        ),
    ];
    for (c, want) in cases {
        assert_eq!(
            message(run(&base, "github.find_pull_request", c).await),
            want
        );
    }
    assert_eq!(log.lock().unwrap().len(), 0);
}
