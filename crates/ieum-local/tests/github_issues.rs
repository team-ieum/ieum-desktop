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

#[tokio::test]
async fn update_issue_rules() {
    let mut body = issue(json!([]));
    body["state"] = json!("closed");
    let (base, log) = server(200, body).await;
    let c = |extra: Value| {
        let mut m = json!({"owner": "a", "repo": "b", "issue_number": "7"});
        m.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        m
    };
    let out = run(
        &base,
        "github.update_issue",
        c(json!({"state": "closed", "state_reason": "completed", "title": " "})),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({
            "number": 7,
            "url": "https://github.com/a/b/issues/7",
            "title": "T",
            "state": "closed",
            "warnings": []
        })
    );
    let out = run(
        &base,
        "github.update_issue",
        c(json!({"state_reason": "completed", "title": "새"})),
    )
    .await
    .unwrap();
    assert_eq!(out["warnings"][0], "닫는 이유는 상태를 바꿀 때만 적용돼요.");
    {
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(
            (log[0].method.as_str(), log[0].path.as_str()),
            ("PATCH", "/repos/a/b/issues/7")
        );
        assert_eq!(
            log[0].json(),
            json!({"state": "closed", "state_reason": "completed"})
        );
        assert_eq!(log[1].json(), json!({"title": "새"}));
    }
    assert_eq!(
        message(
            run(
                &base,
                "github.update_issue",
                c(json!({"title": "", "body": " ", "state": "", "state_reason": ""}))
            )
            .await
        ),
        "바꿀 칸이 없습니다."
    );
    assert_eq!(
        message(run(&base, "github.update_issue", c(json!({"state": "merged"}))).await),
        "state는 open, closed 중 하나여야 합니다."
    );
    assert_eq!(
        message(
            run(
                &base,
                "github.update_issue",
                c(json!({"state_reason": "x"}))
            )
            .await
        ),
        "state_reason은 completed, not_planned, duplicate, reopened 중 하나여야 합니다."
    );
    assert_eq!(log.lock().unwrap().len(), 2);
}

fn found(n: usize, pr: bool) -> Vec<Value> {
    (1..=n)
        .map(|i| {
            let mut v = json!({
                "number": i, "title": format!("t{i}"), "state": "open",
                "html_url": format!("https://github.com/a/b/issues/{i}"),
                "labels": [{"name": "bug"}], "assignees": [{"login": "me"}],
                "user": {"login": "au"}, "created_at": "2026-01-01T00:00:00Z"
            });
            if pr {
                v["pull_request"] = json!({});
            }
            v
        })
        .collect()
}

fn find_cfg(by: &str, value: Value) -> Value {
    json!({"owner": "a", "repo": "b", "search_by": by, "value": value})
}

const NONE: Value = Value::Null;

#[tokio::test]
async fn find_by_number_404_and_pr_are_empty() {
    let empty = json!({"found": false, "total": 0, "items": [], "message": "결과가 없습니다"});
    let (base, log) = server(404, json!({"message": "Not Found"})).await;
    let out = run(
        &base,
        "github.find_issue",
        find_cfg("number", json!(" #12 ")),
    )
    .await;
    assert_eq!(Value::Object(out.unwrap()), empty);
    assert_eq!(log.lock().unwrap()[0].path, "/repos/a/b/issues/12");
    let (base, _) = server(200, json!({"number": 12, "pull_request": {}})).await;
    let out = run(&base, "github.find_issue", find_cfg("number", json!(12))).await;
    assert_eq!(Value::Object(out.unwrap()), empty);
    let (base, _) = server(200, found(1, false).remove(0)).await;
    let out = run(&base, "github.find_issue", find_cfg("number", json!("1")))
        .await
        .unwrap();
    assert_eq!(
        out["items"][0],
        json!({"number": 1, "title": "t1", "state": "open", "url": "https://github.com/a/b/issues/1",
               "labels": ["bug"], "assignees": ["me"], "author": "au", "created_at": "2026-01-01T00:00:00Z"})
    );
    assert_eq!(
        (
            out["found"].clone(),
            out["total"].clone(),
            out["message"].clone()
        ),
        (json!(true), json!(1), json!(""))
    );
}

#[tokio::test]
async fn find_by_label_pages_until_short_page() {
    let (l, base) = bind().await;
    let log = serve(l, |r| {
        let page: usize = r.path.rsplit("page=").next().unwrap().parse().unwrap();
        let body = if page == 1 {
            let mut v = found(99, false);
            v.extend(found(1, true));
            v
        } else {
            found(3, false)
        };
        resp(200, "", Value::Array(body).to_string().as_bytes())
    });
    let mut c = find_cfg("label", json!("bug"));
    c["max_results"] = json!(150);
    let out = run(&base, "github.find_issue", c).await.unwrap();
    // 150은 100으로 잘린다(102개를 모아도 앞 100개).
    assert_eq!(out["total"], 100);
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(
        log[0].path,
        "/repos/a/b/issues?labels=bug&state=open&per_page=100&page=1"
    );
}

#[tokio::test]
async fn find_by_label_stops_at_ten_pages() {
    let (l, base) = bind().await;
    let body = Value::Array(found(100, false)).to_string().into_bytes();
    let log = serve(l, move |_| resp(200, "", &body));
    let mut c = find_cfg("assignee", json!("me"));
    c["max_results"] = json!(100);
    let out = run(&base, "github.find_issue", c.clone()).await.unwrap();
    assert_eq!(out["total"], 100);
    assert_eq!(log.lock().unwrap().len(), 1);
    // 앞 쪽이 전부 PR이면 limit에 못 닿아 10쪽까지 간다.
    let (l, base) = bind().await;
    let body = Value::Array(found(100, true)).to_string().into_bytes();
    let log = serve(l, move |_| resp(200, "", &body));
    let out = run(&base, "github.find_issue", c).await.unwrap();
    assert_eq!(out["total"], 0);
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 10);
    assert!(log[0].path.contains("assignee=me"));
}

#[tokio::test]
async fn find_by_title_query_and_filter() {
    let mut items = found(1, false);
    items[0]["repository_url"] = json!("https://api.github.com/repos/A/B");
    let mut other = found(1, false);
    other[0]["repository_url"] = json!("https://api.github.com/repos/a/c");
    let mut pr = found(1, true);
    pr[0]["repository_url"] = json!("https://api.github.com/repos/a/b");
    items.extend(other);
    items.extend(pr);
    let (base, log) = server(200, json!({"items": items})).await;
    let out = run(&base, "github.find_issue", find_cfg("title", json!("버그")))
        .await
        .unwrap();
    assert_eq!(out["total"], 1);
    let q = |r: &github_support::Req| {
        let (_, qs) = r.path.split_once('?').unwrap();
        let url = reqwest::Url::parse(&format!("http://x/?{qs}")).unwrap();
        url.query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>()
    };
    {
        let log = log.lock().unwrap();
        assert!(log[0].path.starts_with("/search/issues?"));
        assert_eq!(
            q(&log[0]),
            vec![
                (
                    "q".to_string(),
                    "버그 in:title repo:a/b is:issue state:open".to_string()
                ),
                ("per_page".to_string(), "20".to_string())
            ]
        );
    }
    let mut c = find_cfg("body", json!("x"));
    c["state"] = json!("all");
    run(&base, "github.find_issue", c).await.unwrap();
    assert_eq!(
        q(&log.lock().unwrap()[1])[0].1,
        "x in:body repo:a/b is:issue"
    );
}

#[tokio::test]
async fn find_rejects() {
    let (base, log) = server(200, json!([])).await;
    let long = "가".repeat(201);
    let cases = [
        (
            find_cfg("x", json!("v")),
            "search_by는 number, label, assignee, author, title, body 중 하나여야 합니다.",
        ),
        (
            {
                let mut c = find_cfg("label", json!("v"));
                c["state"] = json!("x");
                c
            },
            "state는 open, closed, all 중 하나여야 합니다.",
        ),
        (find_cfg("label", json!("")), "value를 입력해 주세요."),
        (
            find_cfg("number", json!("abc")),
            "이슈 번호는 양의 정수여야 합니다.",
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
        assert_eq!(message(run(&base, "github.find_issue", c).await), want);
    }
    assert_eq!(log.lock().unwrap().len(), 0);
    // 입력이 틀리면 토큰도 읽지 않는다.
    let t = Arc::new(CountingToken::default());
    let r = run_full(
        &base,
        t.clone(),
        "github.find_issue",
        find_cfg("x", NONE),
        CancellationToken::new(),
    )
    .await;
    assert!(r.is_err());
    assert_eq!(t.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn max_results_clamped() {
    let (base, log) = server(200, json!({"items": []})).await;
    for (v, want) in [(json!("500"), "100"), (json!("abc"), "20"), (json!(0), "1")] {
        let mut c = find_cfg("title", json!("x"));
        c["max_results"] = v;
        run(&base, "github.find_issue", c).await.unwrap();
        let log = log.lock().unwrap();
        assert!(
            log.last()
                .unwrap()
                .path
                .ends_with(&format!("per_page={want}"))
        );
    }
}

#[tokio::test]
async fn list_issues_shape() {
    let mut items = found(2, false);
    items.extend(found(1, true));
    let (base, log) = server(200, Value::Array(items)).await;
    let out = run(
        &base,
        "github.list_issues",
        json!({"owner": "a", "repo": "b", "state": ""}),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"issues": [
            {"number": 1, "title": "t1", "state": "open", "labels": ["bug"], "created_at": "2026-01-01T00:00:00Z"},
            {"number": 2, "title": "t2", "state": "open", "labels": ["bug"], "created_at": "2026-01-01T00:00:00Z"}
        ], "total": 2})
    );
    assert_eq!(
        log.lock().unwrap()[0].path,
        "/repos/a/b/issues?per_page=20&state=open&sort=updated"
    );
    let (base, _) = server(500, json!({})).await;
    assert!(
        run(
            &base,
            "github.list_issues",
            json!({"owner": "a", "repo": "b"})
        )
        .await
        .is_err()
    );
}

/// 검색(GET)에는 `search`, 만들기(POST)에는 201 이슈를 주는 서버.
async fn find_or_create_server(search: Value) -> (String, Log) {
    let (l, base) = bind().await;
    let log = serve(l, move |r| {
        if r.method == "POST" {
            resp(201, "", issue(json!([])).to_string().as_bytes())
        } else {
            resp(200, "", search.to_string().as_bytes())
        }
    });
    (base, log)
}

fn titled(titles: &[&str]) -> Value {
    let items: Vec<Value> = found(titles.len(), false)
        .into_iter()
        .zip(titles)
        .map(|(mut v, t)| {
            v["title"] = json!(t);
            v["repository_url"] = json!("https://api.github.com/repos/a/b");
            v
        })
        .collect();
    json!({ "items": items })
}

fn foc_cfg(value: &str, title: &str) -> Value {
    let mut c = find_cfg("title", json!(value));
    c["title"] = json!(title);
    c
}

#[tokio::test]
async fn find_or_create_title_exact() {
    let (base, log) = find_or_create_server(titled(&["일일 리포트 11-16", "일일  리포트"])).await;
    let out = run(
        &base,
        "github.find_or_create_issue",
        foc_cfg("일일 리포트", "일일 리포트"),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"created": false, "number": 2, "url": "https://github.com/a/b/issues/2",
               "title": "일일  리포트", "state": "open", "warnings": []})
    );
    {
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].path.contains("per_page=100"));
    }

    let (base, log) = find_or_create_server(titled(&["일일 리포트 11-16"])).await;
    let out = run(
        &base,
        "github.find_or_create_issue",
        foc_cfg("일일 리포트", "일일 리포트"),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"created": true, "number": 7, "url": "https://github.com/a/b/issues/7",
               "title": "T", "state": "open", "warnings": []})
    );
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(
        (log[1].method.as_str(), log[1].path.as_str()),
        ("POST", "/repos/a/b/issues")
    );
    assert_eq!(log[1].json(), json!({"title": "일일 리포트", "body": ""}));
}

#[tokio::test]
async fn find_or_create_non_title_limit_one_and_blank_title() {
    let (base, log) = find_or_create_server(json!([])).await;
    let mut c = find_cfg("label", json!("bug"));
    c["title"] = json!("x");
    run(&base, "github.find_or_create_issue", c).await.unwrap();
    assert_eq!(log.lock().unwrap().len(), 2);
    let (base, log) = find_or_create_server(json!([])).await;
    let m = message(run(&base, "github.find_or_create_issue", foc_cfg("a", " ")).await);
    assert_eq!(m, "title을 입력해 주세요.");
    assert!(log.lock().unwrap().is_empty());
}
