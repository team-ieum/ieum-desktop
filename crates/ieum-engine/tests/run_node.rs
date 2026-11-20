use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::{
    Catalog, ErrorCode, NodeCtx, NodeError, NodeRunner, Outputs, Runners, ValidationError,
    Workflow, node_errors, run_node,
};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// config를 그대로 출력으로 돌려준다. `panic: true`면 패닉.
struct Echo;

#[async_trait]
impl NodeRunner for Echo {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        if ctx.config.get("panic") == Some(&json!(true)) {
            panic!("러너 버그");
        }
        Ok(ctx.config)
    }
}

fn catalog() -> Catalog {
    Catalog::stage1(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn runners() -> Runners {
    let echo: Arc<dyn NodeRunner> = Arc::new(Echo);
    Runners::from([("local.ai".to_string(), echo)])
}

fn node(config: Value) -> ieum_engine::Node {
    serde_json::from_value(json!({"id": "b", "type": "AI", "event": "local.ai", "config": config}))
        .unwrap()
}

fn wf(nodes: Value, edges: Value) -> Workflow {
    serde_json::from_value(json!({"nodes": nodes, "edges": edges})).unwrap()
}

fn codes(errors: &[ValidationError]) -> Vec<(ErrorCode, Option<&str>, Option<&str>)> {
    errors
        .iter()
        .map(|e| (e.code, e.node_id.as_deref(), e.field.as_deref()))
        .collect()
}

#[tokio::test]
async fn run_node_renders_seeded_outputs() {
    let mut outputs = Outputs::new();
    outputs.insert("a".into(), json!({"n": 7}).as_object().unwrap().clone());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let n = node(json!({"data": "#{{nodes.a.output.n}}"}));
    let (result, usage) = run_node(&n, &runners(), &outputs, &tx, &CancellationToken::new()).await;
    assert_eq!(result.unwrap()["data"], json!("#7"));
    assert_eq!(usage, None);
    assert!(
        rx.try_recv().is_err(),
        "run_node는 실행 이벤트를 내지 않는다"
    );
}

#[tokio::test]
async fn run_node_panic_is_internal_error() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let n = node(json!({"panic": true}));
    let (result, _) = run_node(
        &n,
        &runners(),
        &Outputs::new(),
        &tx,
        &CancellationToken::new(),
    )
    .await;
    let e = result.unwrap_err();
    assert_eq!(e.message, "노드 실행 중 내부 오류가 났습니다.");
    assert_eq!(e.code, None);
}

#[tokio::test]
async fn run_node_without_runner_fails_instead_of_panicking() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let n: ieum_engine::Node = serde_json::from_value(
        json!({"id": "m", "type": "ACTION", "event": "local.file_move", "config": {}}),
    )
    .unwrap();
    let (result, _) = run_node(
        &n,
        &runners(),
        &Outputs::new(),
        &tx,
        &CancellationToken::new(),
    )
    .await;
    assert!(result.is_err());
}

#[test]
fn node_errors_ignores_graph_errors() {
    // 트리거가 없고 연결도 없다 — 그래프 오류(트리거 수·고아)는 빼고 대상 노드의 칸 오류만.
    let w = wf(
        json!([
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "nope"}},
            {"id": "b", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude"}},
        ]),
        json!([]),
    );
    let errors = node_errors(&w, &catalog(), "b");
    assert_eq!(
        codes(&errors),
        [(ErrorCode::Required, Some("b"), Some("config.prompt"))]
    );
    assert!(node_errors(&w, &catalog(), "없음").is_empty());
}

#[test]
fn node_errors_ref_not_upstream() {
    // t → a, t → b. b가 형제 a·없는 노드 z를 참조하고, 앞 노드 t의 없는 출력 칸을 참조한다.
    let w = wf(
        json!([
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/in"}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p", "data": "{{nodes.z.output.x}}"}},
            {"id": "b", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p",
                "data": "{{nodes.a.output.output}} {{nodes.z.output.x}} {{nodes.t.output.nope}}"}},
        ]),
        json!([{"source": "t", "target": "a"}, {"source": "t", "target": "b"}]),
    );
    let errors = node_errors(&w, &catalog(), "b");
    assert_eq!(
        codes(&errors),
        [
            (ErrorCode::RefNotUpstream, Some("b"), Some("config.data")),
            (ErrorCode::RefUnknownNode, Some("b"), Some("config.data")),
            (ErrorCode::RefUnknownField, Some("b"), Some("config.data")),
        ]
    );
}

#[test]
fn node_errors_unknown_event() {
    let w = wf(
        json!([{"id": "x", "type": "ACTION", "event": "nope.event", "config": {}}]),
        json!([]),
    );
    let errors = node_errors(&w, &catalog(), "x");
    assert_eq!(codes(&errors), [(ErrorCode::UnknownEvent, Some("x"), None)]);
}
