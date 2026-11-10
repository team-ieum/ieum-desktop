use std::collections::BTreeSet;
use std::path::Path;

use ieum_engine::{Catalog, CatalogError, NodeType, Workflow};

#[test]
fn stage1_catalog_has_six_events() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = Catalog::stage1(&root).unwrap();
    for event in [
        "branch.if",
        "local.folder_new_file",
        "local.ai",
        "local.file_move",
        "local.file_rename",
        "local.file_trash",
    ] {
        assert!(catalog.get(event).is_some(), "{event} 없음");
    }
    let branch = catalog.get("branch.if").unwrap();
    assert_eq!(branch.node_type, NodeType::Condition);
    let keys: BTreeSet<String> = ["branch", "branches", "matched", "result"]
        .map(String::from)
        .into();
    assert_eq!(branch.output_keys.as_ref(), Some(&keys));
}

#[test]
fn config_null_and_missing_edges_parse() {
    let wf: Workflow = serde_json::from_value(serde_json::json!({
        "nodes": [
            {"id": "a", "event": "local.ai", "config": null},
            {"id": "b", "event": "local.file_trash"}
        ]
    }))
    .unwrap();
    assert!(wf.nodes[0].config.is_empty());
    assert!(wf.nodes[1].config.is_empty());
    assert!(wf.edges.is_empty());
}

#[test]
fn ai_event_gets_common_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = Catalog::stage1(&root).unwrap();
    let ai = catalog.get("local.ai").unwrap();
    assert_eq!(ai.node_type, NodeType::Ai);
    assert!(ai.output_keys.as_ref().unwrap().contains("output"));
    assert_eq!(ai.outputs_from.as_deref(), Some("config.outputFields"));
}

#[test]
fn load_errors_do_not_panic() {
    let dir = std::env::temp_dir().join(format!("ieum-catalog-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let unknown = dir.join("unknown.json");
    let broken = dir.join("broken.json");
    std::fs::write(&unknown, r#"{"node_type": "HTTP"}"#).unwrap();
    std::fs::write(&broken, "{ not json").unwrap();
    assert!(matches!(
        Catalog::load(&[("x.unknown".into(), unknown)]),
        Err(CatalogError::UnknownNodeType { .. })
    ));
    assert!(matches!(
        Catalog::load(&[("x.broken".into(), broken)]),
        Err(CatalogError::Json { .. })
    ));
    assert!(matches!(
        Catalog::load(&[("x.missing".into(), dir.join("missing.json"))]),
        Err(CatalogError::Io { .. })
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn workflow_roundtrip_keeps_unknown_keys() {
    let json = serde_json::json!({
        "nodes": [{
            "id": "n1",
            "event": "local.file_move",
            "type": "ACTION",
            "position": {"x": 1, "y": 2},
            "config": {"zeta": 1, "path": "{{nodes.t.output.path}}"},
            "foo": {"bar": [1, 2]}
        }],
        "edges": [{"id": "e1", "source": "t", "target": "n1", "conditionType": null}]
    });
    let wf: Workflow = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(wf.nodes[0].extra["foo"], json["nodes"][0]["foo"]);
    let mut expected = json;
    // conditionType: null 은 None 으로 읽혀 쓰기 때 빠진다.
    expected["edges"][0]
        .as_object_mut()
        .unwrap()
        .remove("conditionType");
    assert_eq!(serde_json::to_value(&wf).unwrap(), expected);
    // Value 비교는 키 순서를 안 보므로 문자열로 순서까지 확인.
    assert_eq!(serde_json::to_string(&wf).unwrap(), expected.to_string());
}
