//! GitHub 칸 목록(소유자·저장소·브랜치·마일스톤) 조회. 127.0.0.1 임의 포트의 가짜 서버에만 보낸다.

mod github_support;

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use github_support::{Log, bind, resp, serve};
use ieum_engine::NodeError;
use ieum_local::github::{GithubApi, GithubToken, OptionList, options};
use serde_json::{Map, Value, json};

#[derive(Default)]
struct CountingToken(AtomicUsize);

#[async_trait]
impl GithubToken for CountingToken {
    async fn token(&self) -> Result<String, NodeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("t0k".into())
    }
}

/// 경로(쿼리 제외)별로 응답을 고르는 서버. 모르는 경로는 404.
async fn server(routes: Vec<(&'static str, u16, Value)>) -> (String, Log) {
    let (l, base) = bind().await;
    let log = serve(l, move |req| {
        let path = req.path.split('?').next().unwrap_or("");
        match routes.iter().find(|(p, _, _)| *p == path) {
            Some((_, s, b)) => resp(*s, "", b.to_string().as_bytes()),
            None => resp(404, "", b"{}"),
        }
    });
    (base, log)
}

fn inputs(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

async fn run(base: &str, source: &str, v: Value) -> Result<OptionList, NodeError> {
    options(
        &GithubApi::with_base(base),
        &CountingToken::default(),
        source,
        &inputs(v),
    )
    .await
}

fn paths(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|r| r.path.clone()).collect()
}

#[tokio::test]
async fn owners_me_first_then_orgs() {
    let (base, log) = server(vec![
        ("/user", 200, json!({"login": "me"})),
        (
            "/user/orgs",
            200,
            json!([{"login": "acme"}, {"login": "zeta"}]),
        ),
    ])
    .await;
    let out = run(&base, "github.owners", json!({})).await.unwrap();
    assert_eq!(
        serde_json::to_value(&out).unwrap(),
        json!({"items": [
            {"id": "me", "name": "me", "hint": "내 계정", "updatedAt": null},
            {"id": "acme", "name": "acme", "hint": "조직", "updatedAt": null},
            {"id": "zeta", "name": "zeta", "hint": "조직", "updatedAt": null},
        ], "truncated": false})
    );
    assert_eq!(paths(&log), ["/user", "/user/orgs?per_page=100&page=1"]);
}

#[tokio::test]
async fn owners_org_403_only_me() {
    let (base, _) = server(vec![
        ("/user", 200, json!({"login": "me"})),
        ("/user/orgs", 403, json!({})),
    ])
    .await;
    let out = run(&base, "github.owners", json!({})).await.unwrap();
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].id, "me");
}

#[tokio::test]
async fn repos_me_uses_user_repos() {
    let repos = json!([
        {"name": "a", "private": false, "updated_at": "2026-01-01T00:00:00Z"},
        {"name": "b", "private": true, "updated_at": "2026-01-02T00:00:00Z"},
    ]);
    for owner in ["me", "ME"] {
        let (base, log) = server(vec![
            ("/user", 200, json!({"login": "me"})),
            ("/user/repos", 200, repos.clone()),
        ])
        .await;
        let out = run(&base, "github.repos", json!({"owner": owner}))
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            json!({"items": [
                {"id": "a", "name": "a", "hint": "공개", "updatedAt": "2026-01-01T00:00:00Z"},
                {"id": "b", "name": "b", "hint": "비공개", "updatedAt": "2026-01-02T00:00:00Z"},
            ], "truncated": false})
        );
        assert_eq!(
            paths(&log),
            [
                "/user",
                "/user/repos?affiliation=owner&sort=updated&per_page=100&page=1"
            ]
        );
    }
}

#[tokio::test]
async fn repos_org_then_user_fallback() {
    let (base, log) = server(vec![
        ("/user", 200, json!({"login": "me"})),
        (
            "/users/bob/repos",
            200,
            json!([{"name": "x", "private": false}]),
        ),
    ])
    .await;
    let out = run(&base, "github.repos", json!({"owner": "bob"}))
        .await
        .unwrap();
    assert_eq!(out.items[0].id, "x");
    assert_eq!(
        paths(&log),
        [
            "/user",
            "/orgs/bob/repos?sort=updated&per_page=100&page=1",
            "/users/bob/repos?sort=updated&per_page=100&page=1"
        ]
    );
}

#[tokio::test]
async fn repos_truncated_at_300() {
    let full: Vec<Value> = (0..100)
        .map(|i| json!({"name": format!("r{i}"), "private": false}))
        .collect();
    let (base, log) = server(vec![
        ("/user", 200, json!({"login": "me"})),
        ("/user/repos", 200, Value::Array(full)),
    ])
    .await;
    let out = run(&base, "github.repos", json!({"owner": "me"}))
        .await
        .unwrap();
    assert_eq!(out.items.len(), 300);
    assert!(out.truncated);
    assert_eq!(paths(&log).len(), 4); // /user + 3쪽
}

#[tokio::test]
async fn branches_and_milestones_shape() {
    let (base, log) = server(vec![
        (
            "/repos/o/r/branches",
            200,
            json!([{"name": "main"}, {"name": "dev"}]),
        ),
        (
            "/repos/o/r/milestones",
            200,
            json!([{"number": 3, "title": "v1"}]),
        ),
    ])
    .await;
    let b = run(
        &base,
        "github.branches",
        json!({"owner": " o ", "repo": "r"}),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&b).unwrap(),
        json!({"items": [
            {"id": "main", "name": "main", "hint": null, "updatedAt": null},
            {"id": "dev", "name": "dev", "hint": null, "updatedAt": null},
        ], "truncated": false})
    );
    let m = run(
        &base,
        "github.milestones",
        json!({"owner": "o", "repo": "r"}),
    )
    .await
    .unwrap();
    assert_eq!(
        (m.items[0].id.as_str(), m.items[0].name.as_str()),
        ("3", "v1")
    );
    assert_eq!(
        paths(&log),
        [
            "/repos/o/r/branches?per_page=100&page=1",
            "/repos/o/r/milestones?state=open&sort=due_on&per_page=100&page=1"
        ]
    );
}

#[tokio::test]
async fn bad_inputs_send_nothing() {
    let (base, log) = server(vec![]).await;
    let token = CountingToken::default();
    let api = GithubApi::with_base(&base);
    for (src, v) in [
        ("github.repos", json!({"owner": "a b"})),
        ("github.repos", json!({"owner": "{{nodes.x.output.y}}"})),
        ("github.branches", json!({"owner": "o", "repo": ".."})),
        ("github.milestones", json!({"owner": "o"})),
    ] {
        let e = options(&api, &token, src, &inputs(v)).await.unwrap_err();
        assert_eq!(e.code.as_deref(), Some("GITHUB_INPUT"));
    }
    assert!(paths(&log).is_empty());
    assert_eq!(token.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_source_rejected() {
    let (base, log) = server(vec![]).await;
    let e = run(&base, "github.nope", json!({})).await.unwrap_err();
    assert_eq!(e.message, "지원하지 않는 목록이에요.");
    assert!(paths(&log).is_empty());
}

#[tokio::test]
async fn errors_keep_code() {
    let (base, _) = server(vec![("/user", 401, json!({"message": "secret"}))]).await;
    let e = run(&base, "github.owners", json!({})).await.unwrap_err();
    assert_eq!(e.code.as_deref(), Some("GITHUB_AUTH"));
    assert!(!e.message.contains("secret"));
}
