use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::{
    Catalog, ExecEvent, NodeCtx, NodeError, NodeRunner, NodeStatus, RejectReason, RunResult,
    RunStatus, Runners, Workflow, run_workflow,
};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// config를 그대로 출력으로 돌려준다. `fail: true`면 실패, `wait: true`면 취소될 때까지 기다렸다 실패.
/// branch.if에 쓰면 config의 `_branchKeys`가 그대로 출력에 실려 가짜 CONDITION 러너가 된다.
struct Echo;

#[async_trait]
impl NodeRunner for Echo {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        if ctx.config.get("wait") == Some(&json!(true)) {
            ctx.cancel.cancelled().await;
            return Err(NodeError {
                message: "취소됨".into(),
            });
        }
        if ctx.config.get("fail") == Some(&json!(true)) {
            return Err(NodeError {
                message: "일부러 실패".into(),
            });
        }
        Ok(ctx.config)
    }
}

fn catalog() -> Catalog {
    Catalog::stage1(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn runners() -> Runners {
    let echo: Arc<dyn NodeRunner> = Arc::new(Echo);
    ["local.ai", "branch.if", "local.file_move"]
        .into_iter()
        .map(|e| (e.to_string(), echo.clone()))
        .collect()
}

fn trigger() -> Value {
    json!({"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/in"}})
}

fn ai(id: &str, config: Value) -> Value {
    let mut c = json!({"provider": "cli:claude", "prompt": "요약"});
    c.as_object_mut()
        .unwrap()
        .extend(config.as_object().unwrap().clone());
    json!({"id": id, "type": "AI", "event": "local.ai", "config": c})
}

fn cond(keys: &[&str]) -> Value {
    json!({"id": "c", "type": "CONDITION", "event": "branch.if",
           "config": {"left": "a", "operator": "isNotEmpty", "_branchKeys": keys}})
}

fn edge(s: &str, t: &str) -> Value {
    json!({"source": s, "target": t})
}

fn branch(s: &str, t: &str, ct: &str) -> Value {
    json!({"source": s, "target": t, "conditionType": ct})
}

async fn run_with(wf: Value, cancel: CancellationToken) -> (RunResult, Vec<ExecEvent>) {
    let wf: Workflow = serde_json::from_value(wf).unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let trigger_output = json!({"path": "/in/a.pdf"}).as_object().unwrap().clone();
    let result = run_workflow(&wf, &catalog(), &runners(), trigger_output, tx, cancel).await;
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (result, events)
}

async fn run(nodes: Value, edges: Value) -> (RunResult, Vec<ExecEvent>) {
    run_with(
        json!({"nodes": nodes, "edges": edges}),
        CancellationToken::new(),
    )
    .await
}

fn started(events: &[ExecEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            ExecEvent::NodeStarted { node_id } => Some(node_id.as_str()),
            _ => None,
        })
        .collect()
}

fn status(r: &RunResult, id: &str) -> NodeStatus {
    r.nodes[id].status
}

#[tokio::test]
async fn linear_chain_runs_in_order() {
    let (r, events) = run(
        json!([ai("b", json!({})), trigger(), ai("a", json!({}))]),
        json!([edge("t", "a"), edge("a", "b")]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Succeeded);
    assert_eq!(started(&events), vec!["t", "a", "b"]);
    assert_eq!(
        r.nodes["t"].output.as_ref().unwrap()["path"],
        json!("/in/a.pdf")
    );
    assert_eq!(
        r.nodes["b"].output.as_ref().unwrap()["prompt"],
        json!("요약")
    );
    assert!(matches!(events.first(), Some(ExecEvent::RunStarted { .. })));
    assert!(matches!(
        events.last(),
        Some(ExecEvent::RunFinished {
            status: RunStatus::Succeeded,
            ..
        })
    ));
    let ExecEvent::RunStarted { run_id } = &events[0] else {
        unreachable!()
    };
    assert_eq!(run_id, &r.run_id);
    assert_eq!(
        serde_json::to_value(&events[0]).unwrap(),
        json!({"kind": "runStarted", "runId": r.run_id})
    );
}

#[tokio::test]
async fn all_dead_inputs_skip_and_propagate() {
    let (r, events) = run(
        json!([
            trigger(),
            cond(&["true"]),
            ai("x", json!({})),
            ai("y", json!({})),
            ai("z", json!({}))
        ]),
        json!([
            edge("t", "c"),
            branch("c", "x", "true"),
            branch("c", "y", "false"),
            edge("y", "z")
        ]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Succeeded);
    assert_eq!(status(&r, "x"), NodeStatus::Succeeded);
    assert_eq!(status(&r, "y"), NodeStatus::Skipped);
    assert_eq!(status(&r, "z"), NodeStatus::Skipped);
    let skipped: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            ExecEvent::NodeSkipped { node_id } => Some(node_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(skipped, vec!["y", "z"]);
    assert_eq!(
        r.nodes["c"].output.as_ref().unwrap()["_branchKeys"],
        json!(["true"])
    );
}

#[tokio::test]
async fn merge_runs_when_one_parent_live() {
    let (r, events) = run(
        json!([
            trigger(),
            cond(&["false"]),
            ai("x", json!({})),
            ai("y", json!({})),
            ai("m", json!({}))
        ]),
        json!([
            edge("t", "c"),
            branch("c", "x", "true"),
            branch("c", "y", "false"),
            edge("x", "m"),
            edge("y", "m")
        ]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Succeeded);
    assert_eq!(status(&r, "x"), NodeStatus::Skipped);
    assert_eq!(status(&r, "m"), NodeStatus::Succeeded);
    assert_eq!(started(&events), vec!["t", "c", "y", "m"]);
}

#[tokio::test]
async fn failure_marks_rest_not_run() {
    let (r, events) = run(
        json!([
            trigger(),
            ai("a", json!({"fail": true})),
            ai("b", json!({}))
        ]),
        json!([edge("t", "a"), edge("a", "b")]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Failed);
    assert_eq!(status(&r, "a"), NodeStatus::Failed);
    assert_eq!(r.nodes["a"].error.as_deref(), Some("일부러 실패"));
    assert_eq!(status(&r, "b"), NodeStatus::NotRun);
    assert!(events.iter().any(|e| matches!(e,
        ExecEvent::NodeFailed { node_id, message } if node_id == "a" && message == "일부러 실패")));
    assert!(matches!(
        events.last(),
        Some(ExecEvent::RunFinished {
            status: RunStatus::Failed,
            ..
        })
    ));
}

#[tokio::test]
async fn invalid_workflow_is_rejected_without_running() {
    let (r, events) = run(
        json!([
            trigger(),
            ai("a", json!({"prompt": "{{nodes.nope.output.x}}"}))
        ]),
        json!([edge("t", "a")]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Rejected);
    assert!(!r.validation_errors.is_empty());
    assert!(started(&events).is_empty());
    assert_eq!(status(&r, "a"), NodeStatus::NotRun);

    // 러너 없는 event도 거절
    let (r, events) = run(
        json!([trigger(), {"id": "d", "type": "ACTION", "event": "local.file_trash", "config": {}}]),
        json!([edge("t", "d")]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Rejected);
    assert!(started(&events).is_empty());
}

#[tokio::test]
async fn cancel_stops_run() {
    let cancel = CancellationToken::new();
    let wf = json!({"nodes": [trigger(), ai("a", json!({"wait": true})), ai("b", json!({}))],
                    "edges": [edge("t", "a"), edge("a", "b")]});
    let canceller = cancel.clone();
    let ((r, events), _) = tokio::join!(run_with(wf, cancel), async move {
        tokio::task::yield_now().await;
        canceller.cancel();
    });
    assert_eq!(r.status, RunStatus::Cancelled);
    assert_eq!(status(&r, "b"), NodeStatus::NotRun);
    assert!(started(&events).contains(&"a"));
    assert!(matches!(
        events.last(),
        Some(ExecEvent::RunFinished {
            status: RunStatus::Cancelled,
            ..
        })
    ));
}

#[tokio::test]
async fn oversized_workflow_rejected_before_validation() {
    let nodes: Vec<Value> = (0..501).map(|i| ai(&format!("n{i}"), json!({}))).collect();
    let (r, events) = run(Value::Array(nodes), json!([])).await;
    assert_eq!(r.status, RunStatus::Rejected);
    assert!(r.validation_errors.is_empty());
    assert!(started(&events).is_empty());

    let edges: Vec<Value> = (0..2001).map(|_| edge("t", "t")).collect();
    let (r, _) = run(json!([trigger()]), Value::Array(edges)).await;
    assert_eq!(r.status, RunStatus::Rejected);
    assert!(r.validation_errors.is_empty());
}

#[tokio::test]
async fn cycle_rejected_without_running() {
    let (r, events) = run(
        json!([trigger(), ai("a", json!({})), ai("b", json!({}))]),
        json!([edge("t", "a"), edge("a", "b"), edge("b", "a")]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Rejected);
    assert_eq!(r.reject_reason, Some(RejectReason::Cycle));
    assert!(r.validation_errors.is_empty());
    assert!(started(&events).is_empty());
}

#[tokio::test]
async fn rejected_has_reason() {
    let nodes: Vec<Value> = (0..501).map(|i| ai(&format!("n{i}"), json!({}))).collect();
    let (r, _) = run(Value::Array(nodes), json!([])).await;
    assert_eq!(r.reject_reason, Some(RejectReason::TooLarge));
    assert_eq!(
        serde_json::to_value(&r.reject_reason).unwrap(),
        json!({"kind": "TOO_LARGE"})
    );

    let (r, _) = run(
        json!([trigger(), {"id": "d", "type": "ACTION", "event": "local.file_trash", "config": {}}]),
        json!([edge("t", "d")]),
    )
    .await;
    let reason = RejectReason::UnknownEvent {
        event: "local.file_trash".into(),
    };
    assert_eq!(
        serde_json::to_value(&reason).unwrap(),
        json!({"kind": "UNKNOWN_EVENT", "event": "local.file_trash"})
    );
    assert_eq!(r.reject_reason, Some(reason));
    assert!(r.validation_errors.is_empty());

    let (r, _) = run(
        json!([
            trigger(),
            ai("a", json!({"prompt": "{{nodes.nope.output.x}}"}))
        ]),
        json!([edge("t", "a")]),
    )
    .await;
    assert_eq!(r.reject_reason, Some(RejectReason::Invalid));
    assert!(!r.validation_errors.is_empty());
    assert_eq!(
        serde_json::to_value(RejectReason::Cycle).unwrap(),
        json!({"kind": "CYCLE"})
    );
    assert_eq!(
        serde_json::to_value(RejectReason::Invalid).unwrap(),
        json!({"kind": "INVALID"})
    );

    let (r, _) = run(json!([trigger()]), json!([])).await;
    assert_eq!(r.status, RunStatus::Succeeded);
    assert_eq!(r.reject_reason, None);
}

#[tokio::test]
async fn config_rendered_before_runner() {
    let (r, _) = run(
        json!([
            trigger(),
            ai("a", json!({"output": "보고서"})),
            ai(
                "b",
                json!({"prompt": "{{nodes.a.output.output}} / {{nodes.t.output.path}}"})
            )
        ]),
        json!([edge("t", "a"), edge("a", "b")]),
    )
    .await;
    assert_eq!(r.status, RunStatus::Succeeded);
    assert_eq!(
        r.nodes["b"].output.as_ref().unwrap()["prompt"],
        json!("보고서 / /in/a.pdf")
    );
}
