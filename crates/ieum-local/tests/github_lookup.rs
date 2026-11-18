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

#[tokio::test]
async fn find_branch_list_and_one() {
    let (base, log) = server(
        200,
        json!([
            {"name": "main", "commit": {"sha": "s1"}, "protected": true, "x": 1},
            {"name": "dev", "commit": {"sha": "s2"}}
        ]),
    )
    .await;
    let out = run(
        &base,
        "github.find_branch",
        json!({"owner": "a", "repo": "b", "branch": ""}),
    )
    .await
    .unwrap();
    assert_eq!(
        out["items"],
        json!([
            {"name": "main", "sha": "s1", "protected": true},
            {"name": "dev", "sha": "s2", "protected": false}
        ])
    );
    assert_eq!(req_paths(&log), ["/repos/a/b/branches?per_page=100"]);

    let (base, log) = server(
        200,
        json!({"name": "main", "commit": {"sha": "s1"}, "protected": false}),
    )
    .await;
    let out = run(
        &base,
        "github.find_branch",
        json!({"owner": "a", "repo": "b", "branch": " main "}),
    )
    .await
    .unwrap();
    assert_eq!(out["total"], json!(1));
    assert_eq!(req_paths(&log), ["/repos/a/b/branches/main"]);
}

#[tokio::test]
async fn find_branch_encodes_slash() {
    let (base, log) = server(200, json!({"name": "feat/x", "commit": {"sha": "s"}})).await;
    run(
        &base,
        "github.find_branch",
        json!({"owner": "a", "repo": "b", "branch": "feat/x"}),
    )
    .await
    .unwrap();
    assert_eq!(req_paths(&log), ["/repos/a/b/branches/feat%2Fx"]);
}

#[tokio::test]
async fn find_branch_rejects() {
    for bad in ["../x", "a//b", "a\\b", "a\nb"] {
        let (base, log) = server(200, json!({})).await;
        let tok = Arc::new(CountingToken::default());
        let r = run_full(
            &base,
            tok.clone(),
            "github.find_branch",
            json!({"owner": "a", "repo": "b", "branch": bad}),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(message(r), "브랜치 형식이 올바르지 않습니다.", "{bad:?}");
        assert_eq!(tok.0.load(Ordering::SeqCst), 0);
        assert!(log.lock().unwrap().is_empty());
    }
}

fn file_cfg(path: &str) -> Value {
    json!({"owner": "a", "repo": "b", "path": path})
}

#[tokio::test]
async fn get_file_text() {
    let (base, log) = server(
        200,
        json!({"name": "main.rs", "path": "src/main.rs", "type": "file", "sha": "s", "size": 5,
               "encoding": "base64", "content": "aGVs\nbG8=\n"}),
    )
    .await;
    let mut cfg = file_cfg("src/main.rs");
    cfg["ref"] = json!("dev");
    let out = run(&base, "github.get_file", cfg).await.unwrap();
    assert_eq!(
        out["items"],
        json!([{"name": "main.rs", "path": "src/main.rs", "type": "file", "sha": "s", "size": 5,
                "content": "hello", "truncated": false}])
    );
    assert_eq!(req_paths(&log), ["/repos/a/b/contents/src/main.rs?ref=dev"]);
}

async fn one_file(entry: Value) -> Value {
    let (base, _) = server(200, entry).await;
    let out = run(&base, "github.get_file", file_cfg("f")).await.unwrap();
    out["items"][0].clone()
}

fn b64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

#[tokio::test]
async fn get_file_content_limits() {
    let big = one_file(json!({"size": 2_000_000, "content": "aGk="})).await;
    assert_eq!(
        (big["content"].clone(), big["truncated"].clone()),
        (json!(""), json!(true))
    );
    let none = one_file(json!({"size": 5, "encoding": "none", "content": ""})).await;
    assert_eq!(
        (none["content"].clone(), none["truncated"].clone()),
        (json!(""), json!(true))
    );
    let bin = one_file(json!({"size": 2, "content": b64(&[0xff, 0xfe])})).await;
    assert_eq!(
        (bin["content"].clone(), bin["truncated"].clone()),
        (json!(""), json!(false))
    );
    let bad = one_file(json!({"size": 2, "content": "!!!!"})).await;
    assert_eq!(
        (bad["content"].clone(), bad["truncated"].clone()),
        (json!(""), json!(false))
    );
    let long = "가".repeat(100_001);
    let t = one_file(json!({"size": 300_003, "content": b64(long.as_bytes())})).await;
    assert_eq!(t["content"], json!("가".repeat(100_000)));
    assert_eq!(t["truncated"], json!(true));
}

#[tokio::test]
async fn get_file_dir() {
    let entries: Vec<Value> = (0..1001)
        .map(|i| json!({"name": format!("f{i}"), "path": format!("d/f{i}"), "type": "file", "sha": "s", "size": 1}))
        .collect();
    let (base, _) = server(200, Value::Array(entries)).await;
    let out = run(&base, "github.get_file", file_cfg("d")).await.unwrap();
    let items = out["items"].as_array().unwrap();
    assert_eq!(items.len(), 1000);
    assert_eq!(items[0]["content"], json!(""));
    assert_eq!(items[0]["truncated"], json!(false));
}

#[tokio::test]
async fn get_file_rejects() {
    let long = "a".repeat(1025);
    let cases = [
        (file_cfg("../a"), "path 형식이 올바르지 않습니다."),
        (file_cfg("a//b"), "path 형식이 올바르지 않습니다."),
        (file_cfg("a\\b"), "path 형식이 올바르지 않습니다."),
        (file_cfg("\na"), "path 형식이 올바르지 않습니다."),
        (file_cfg(&long), "path 형식이 올바르지 않습니다."),
        (
            json!({"owner": "a", "repo": "b", "path": "x", "ref": "u:main"}),
            "ref 형식이 올바르지 않습니다.",
        ),
    ];
    for (cfg, want) in cases {
        let (base, log) = server(200, json!({})).await;
        let tok = Arc::new(CountingToken::default());
        let r = run_full(
            &base,
            tok.clone(),
            "github.get_file",
            cfg,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(message(r), want);
        assert_eq!(tok.0.load(Ordering::SeqCst), 0);
        assert!(log.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn get_file_encodes_segments() {
    let (base, log) = server(404, json!({})).await;
    run(&base, "github.get_file", file_cfg("docs/a b.md"))
        .await
        .unwrap();
    assert_eq!(req_paths(&log), ["/repos/a/b/contents/docs/a%20b.md"]);
}
