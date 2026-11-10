//! 앱 저장소. 모든 파일은 임시 폴더 안에서만 만든다.

use std::fs;

use ieum_desktop_lib::store::{Language, Settings, Store, StoredWorkflow};
use ieum_engine::Workflow;
use serde_json::json;

fn workflow() -> Workflow {
    serde_json::from_value(json!({
        "nodes": [{"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/tmp/x"}}],
        "edges": []
    }))
    .unwrap()
}

fn stored(id: &str) -> StoredWorkflow {
    StoredWorkflow {
        id: id.into(),
        name: "정리".into(),
        enabled: true,
        workflow: workflow(),
        updated_at: String::new(),
    }
}

#[test]
fn store_roundtrip_and_atomic_write() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());

    // 빈 id면 새 UUID를 붙이고 시각을 채운다.
    let saved = store.save_workflow(stored("")).unwrap();
    assert!(uuid::Uuid::parse_str(&saved.id).is_ok());
    assert!(!saved.updated_at.is_empty());

    let mut again = saved.clone();
    again.name = "바뀐 이름".into();
    store.save_workflow(again).unwrap();

    let got = store.get_workflow(&saved.id).unwrap();
    assert_eq!(got.name, "바뀐 이름");
    assert_eq!(got.workflow.nodes[0].id, "t");
    assert_eq!(store.list_workflows().unwrap().len(), 1);

    // 원자적 쓰기: 임시 파일이 남지 않는다.
    let names: Vec<String> = fs::read_dir(t.path().join("workflows"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![format!("{}.json", saved.id)]);

    // 설정: 없으면 기본값, 쓰고 읽으면 그대로.
    let s = store.get_settings().unwrap();
    assert_eq!(s.language, Language::System);
    assert!(!s.first_run_done);
    let s = Settings {
        language: Language::En,
        autostart: true,
        first_run_done: true,
    };
    store.set_settings(&s).unwrap();
    assert_eq!(store.get_settings().unwrap(), s);

    store.delete_workflow(&saved.id).unwrap();
    assert!(store.list_workflows().unwrap().is_empty());
}

#[test]
fn ids_must_be_uuid() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let store = Store::new(data.clone());
    // 앱 데이터 밖에 미끼 파일
    fs::write(t.path().join("x.json"), "{}").unwrap();

    for bad in ["../x", "x", "../../etc/passwd", "a/b", ""] {
        if !bad.is_empty() {
            assert!(store.save_workflow(stored(bad)).is_err(), "save {bad}");
        }
        assert!(store.get_workflow(bad).is_err(), "get {bad}");
        assert!(store.delete_workflow(bad).is_err(), "delete {bad}");
        assert!(store.get_run(bad).is_err(), "get_run {bad}");
    }
    assert!(t.path().join("x.json").exists());
    assert!(!t.path().join("workflows").exists());
}
