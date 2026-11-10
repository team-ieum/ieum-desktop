use std::collections::BTreeSet;
use std::path::Path;

use ieum_engine::{Catalog, NodeType, Workflow};

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
fn workflow_roundtrip_keeps_unknown_keys() {
    let json = serde_json::json!({
        "nodes": [{
            "id": "n1",
            "event": "local.file_move",
            "type": "ACTION",
            "position": {"x": 1, "y": 2},
            "config": {"path": "{{nodes.t.output.path}}"},
            "foo": {"bar": [1, 2]}
        }],
        "edges": [{"id": "e1", "source": "t", "target": "n1", "conditionType": null}]
    });
    let wf: Workflow = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(wf.nodes[0].extra["foo"], json["nodes"][0]["foo"]);
    let mut expected = json;
    // conditionType: null 은 None 으로 읽혀 쓰기 때 빠진다.
    expected["edges"][0].as_object_mut().unwrap().remove("conditionType");
    assert_eq!(serde_json::to_value(&wf).unwrap(), expected);
}
