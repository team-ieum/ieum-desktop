use ieum_engine::{
    BuiltinRunners, Node, NodeCtx, NodeRunner, Outputs, Runners, TransformMapRunner,
};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

async fn run_map(config: Value) -> Map<String, Value> {
    let node: Node = serde_json::from_value(
        json!({"id": "m", "event": "transform.map", "type": "TRANSFORM", "config": config}),
    )
    .unwrap();
    let outputs = Outputs::from([(
        "t".to_string(),
        json!({"x": "1"}).as_object().unwrap().clone(),
    )]);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    TransformMapRunner
        .run(NodeCtx {
            node: &node,
            // 치환된 사본은 쓰지 않는다 — 원본 mappings의 1단계만 치환해야 한다
            config: Map::new(),
            outputs: &outputs,
            events: &tx,
            cancel: &cancel,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn map_renders_only_first_level_strings() {
    let out = run_map(json!({"mappings": {
        "a": "{{nodes.t.output.x}}",
        "b": {"c": "{{nodes.t.output.x}}"},
        "n": 3
    }}))
    .await;
    assert_eq!(
        Value::Object(out),
        json!({"a": "1", "b": {"c": "{{nodes.t.output.x}}"}, "n": 3})
    );
}

#[tokio::test]
async fn map_without_mappings_is_empty() {
    assert!(run_map(json!({})).await.is_empty());
}

#[test]
fn with_builtin_registers_transform_map() {
    assert!(Runners::with_builtin().contains_key("transform.map"));
}
