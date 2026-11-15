use std::collections::BTreeSet;
use std::path::Path;

use ieum_engine::{Catalog, CatalogError, NodeType, Workflow};

#[test]
fn stage1_catalog_has_thirty_events() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = Catalog::stage1(&root).unwrap();
    assert_eq!(catalog.events().len(), 30);
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
fn embedded_catalog_matches_disk() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let disk = Catalog::stage1(&root).unwrap();
    let embedded = Catalog::embedded().unwrap();
    // 디스크에 정의 파일이 늘면 내장 목록도 늘려야 한다
    let mut events = vec!["branch.if".to_string()];
    for entry in std::fs::read_dir(root.join("crates/ieum-local/defs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            events.push(path.file_stem().unwrap().to_string_lossy().into_owned());
        }
    }
    for dir in ["transform", "formatter", "http"] {
        for entry in std::fs::read_dir(root.join("catalog/events").join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                events.push(format!("{dir}.{stem}"));
            }
        }
    }
    assert_eq!(events.len(), 30);
    assert_eq!(disk.events().len(), 30);
    for event in &events {
        let (d, e) = (disk.get(event).unwrap(), embedded.get(event));
        let e = e.unwrap_or_else(|| panic!("{event} 내장 안 됨"));
        assert_eq!(format!("{d:?}"), format!("{e:?}"), "{event}");
    }
}

#[test]
fn embedded_has_builtin_24() {
    let c = Catalog::embedded().unwrap();
    assert_eq!(c.events().len(), 30);
    assert_eq!(
        c.get("transform.map").unwrap().node_type,
        NodeType::Transform
    );
    assert_eq!(
        c.get("formatter.date_format").unwrap().node_type,
        NodeType::Transform
    );
    assert_eq!(c.get("http.request").unwrap().node_type, NodeType::Http);
    assert_eq!(
        c.events()
            .iter()
            .filter(|e| e.event.starts_with("formatter."))
            .count(),
        22
    );
    assert_eq!(
        c.get("transform.map").unwrap().outputs_from.as_deref(),
        Some("config.mappings")
    );
}

#[test]
fn formatter_catalog_and_runners_match() {
    let c = Catalog::embedded().unwrap();
    let cat: BTreeSet<_> = c
        .events()
        .iter()
        .filter(|e| e.event.starts_with("formatter."))
        .map(|e| e.event.clone())
        .collect();
    let run: BTreeSet<_> = ieum_engine::formatter::events()
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(cat, run);
    assert_eq!(run.len(), 22);
    // title은 실패 메시지 머리로 확인 — 10만 자 넘는 input은 모든 포맷터가 실패시킨다.
    let cfg = serde_json::json!({"input": "x".repeat(100_001)});
    for event in &run {
        let e = ieum_engine::formatter::apply(
            event,
            cfg.as_object().unwrap(),
            "2026-11-07T04:30:00Z".parse().unwrap(),
        )
        .unwrap_err();
        let title = &c.get(event).unwrap().title;
        assert!(
            e.message.starts_with(&format!("{title}: ")),
            "{event}: {}",
            e.message
        );
    }
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
    std::fs::write(&unknown, r#"{"node_type": "DELAY"}"#).unwrap();
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

#[test]
fn events_listed_trigger_first_with_titles() {
    let c = Catalog::embedded().unwrap();
    let ids: Vec<&str> = c.events().iter().map(|e| e.event.as_str()).collect();
    assert_eq!(ids.len(), 30);
    assert_eq!(ids[0], "local.folder_new_file");
    assert!(ids[1..].is_sorted(), "{ids:?}");
    assert_eq!(
        ids[1..8],
        [
            "branch.if",
            "formatter.date_add",
            "formatter.date_compare",
            "formatter.date_format",
            "formatter.list_pick",
            "formatter.list_to_text",
            "formatter.lookup_table",
        ]
    );
    assert_eq!(
        ids[23..],
        [
            "formatter.text_url_encode",
            "http.request",
            "local.ai",
            "local.file_move",
            "local.file_rename",
            "local.file_trash",
            "transform.map",
        ]
    );
    assert_eq!(c.events()[0].title, "폴더에 새 파일");
    assert_eq!(
        c.get("local.file_trash").unwrap().description,
        "파일을 휴지통으로 옮겨요. 영구 삭제하지 않아요."
    );
}
