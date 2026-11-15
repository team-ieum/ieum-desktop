//! HTTP 노드가 낀 test는 사람 확인 뒤에 돌거나 건너뛴다. [켜기] 카드에 HTTP 줄.

mod builder_support;

use std::collections::BTreeMap;
use std::sync::Arc;

use builder_support::MemHost;
use ieum_engine::{NodeRecord, NodeStatus, RunStatus};
use ieum_local::UndoReport;
use ieum_local::builder::*;
use serde_json::{Value, json};
use tempfile::TempDir;

const W: &str = "11111111-1111-4111-8111-111111111111";
const URL: &str = "https://x/{{nodes.t.output.name}}";

async fn run(b: &Builder, op: Value) -> Reply {
    b.apply(W, Op::from_value(op).unwrap(), None).await
}

fn test_op(id: &str) -> Value {
    json!({"op": "test", "args": {"id": id}})
}

/// t → h(GET) → f, t → p(POST). 채팅 시작 시점 노드라 전부 passed.
fn setup() -> (Arc<MemHost>, Builder, TempDir) {
    let t = tempfile::tempdir().unwrap();
    let watch = t.path().canonicalize().unwrap();
    std::fs::write(watch.join("a.txt"), "a").unwrap();
    let h = MemHost::with(
        W,
        json!({
            "nodes": [
                {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
                {"id": "h", "type": "HTTP", "event": "http.request", "label": "조회", "config": {"method": "GET", "url": URL}},
                {"id": "f", "type": "TRANSFORM", "event": "formatter.text_default", "config": {"input": "{{nodes.h.output.statusCode}}", "defaultValue": "없음"}},
                {"id": "p", "type": "HTTP", "event": "http.request", "label": "알림", "config": {"method": "POST", "url": URL}}
            ],
            "edges": [{"source": "t", "target": "h"}, {"source": "h", "target": "f"}, {"source": "t", "target": "p"}]
        }),
    );
    h.put_doc(W, |d| d.enabled = false);
    let b = Builder::new(h.clone()).unwrap();
    (h, b, t)
}

fn ok_run(ids: &[&str]) -> TestRun {
    let nodes: BTreeMap<String, NodeRecord> = ids
        .iter()
        .map(|id| {
            let rec = NodeRecord {
                status: NodeStatus::Succeeded,
                output: Some(Default::default()),
                error: None,
                usage: None,
                error_code: None,
            };
            (id.to_string(), rec)
        })
        .collect();
    TestRun {
        run_id: "run-1".into(),
        status: RunStatus::Succeeded,
        nodes,
        undo: UndoReport::default(),
    }
}

fn tests_of(h: &MemHost) -> BTreeMap<String, TestEntry> {
    h.load_chat(W).unwrap().state.tests
}

#[tokio::test]
async fn test_with_http_asks_first_even_for_get() {
    let (h, b, _t) = setup();
    run(&b, test_op("f")).await;
    assert_eq!(
        h.http_asked(),
        [vec![HttpLine {
            node_id: "h".into(),
            label: "조회".into(),
            method: "GET".into(),
            url: URL.into(),
        }]]
    );
    assert_eq!(
        serde_json::to_value(&h.http_asked()[0][0]).unwrap(),
        json!({"nodeId": "h", "label": "조회", "method": "GET", "url": URL})
    );
}

#[tokio::test]
async fn test_without_http_never_asks() {
    let (h, b, _t) = setup();
    h.push_test_run(Ok(ok_run(&["t"])));
    let r = run(&b, test_op("t")).await;
    assert!(r.ok, "{r:?}");
    assert!(h.http_asked().is_empty());
    assert_eq!(r.data.get("httpSent"), None);
}

#[tokio::test]
async fn skip_marks_http_and_downstream_and_finish_is_ready() {
    let (h, b, _t) = setup();
    // t·p는 테스트 기록으로, h·f는 건너뛰기로 채운다.
    h.update_chat(W, &mut |c| c.state.baseline = Some(Default::default()))
        .unwrap();
    h.set_http_answer(HttpConfirm::Skip);
    let r = run(&b, test_op("f")).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(
        r.data,
        json!({"status": "skipped_by_user", "nodes": ["h", "f"]})
    );
    assert!(h.tested().is_empty());
    let tests = tests_of(&h);
    assert_eq!(tests.keys().collect::<Vec<_>>(), ["f", "h"]);
    let doc = h.doc(W);
    for (i, id) in [(1, "h"), (2, "f")] {
        assert_eq!(
            tests[id],
            TestEntry {
                hash: node_hash(&doc.workflow.nodes[i]),
                status: TestStatus::Skipped
            }
        );
    }
    let r = run(&b, test_op("p")).await;
    assert!(r.ok, "{r:?}");
    h.push_test_run(Ok(ok_run(&["t"])));
    let r = run(&b, test_op("t")).await;
    assert!(r.ok, "{r:?}");
    let r = run(&b, json!({"op": "finish"})).await;
    assert!(r.ok, "{r:?}");
}

#[tokio::test]
async fn cancelled_is_test_declined() {
    let (h, b, _t) = setup();
    let r = run(&b, test_op("f")).await;
    assert_eq!(r.code, Some(Code::TestDeclined));
    assert!(tests_of(&h).is_empty());
    assert!(h.tested().is_empty());
}

#[tokio::test]
async fn send_after_edit_is_stale() {
    let (h, b, _t) = setup();
    h.set_http_answer(HttpConfirm::Send);
    h.on_http(|h| {
        h.put_doc(W, |d| {
            d.workflow.nodes[1]
                .config
                .insert("url".into(), "https://evil/".into());
        })
    });
    let r = run(&b, test_op("f")).await;
    assert_eq!(r.code, Some(Code::Stale));
    assert!(h.tested().is_empty());

    h.on_http(|h| h.set_editing(W, true));
    let r = run(&b, test_op("f")).await;
    assert_eq!(r.code, Some(Code::UserEditing));
    assert!(h.tested().is_empty());
}

#[tokio::test]
async fn send_runs_and_marks_http_sent() {
    let (h, b, _t) = setup();
    h.set_http_answer(HttpConfirm::Send);
    h.push_test_run(Ok(ok_run(&["t", "h", "f"])));
    let r = run(&b, test_op("f")).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.data["httpSent"], true);
    assert_eq!(r.data["runId"], "run-1");
    assert_eq!(h.tested().len(), 1);
}

#[tokio::test]
async fn finish_lists_http_action() {
    let (h, b, _t) = setup();
    let r = run(&b, json!({"op": "finish"})).await;
    assert!(r.ok, "{r:?}");
    let pending = b.pending(W).unwrap();
    let http: Vec<_> = pending
        .actions
        .iter()
        .filter(|a| a.event == "http.request")
        .map(|a| (a.node_id.as_str(), a.target.as_deref()))
        .collect();
    assert_eq!(
        http,
        [
            ("h", Some(format!("GET {URL}").as_str())),
            ("p", Some(format!("POST {URL}").as_str()))
        ]
    );
    assert!(h.http_asked().is_empty());
}
