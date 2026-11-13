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
        ..Settings::default()
    };
    store.set_settings(&s).unwrap();
    assert_eq!(store.get_settings().unwrap(), s);

    store.delete_workflow(&saved.id).unwrap();
    assert!(store.list_workflows().unwrap().is_empty());
}

#[test]
fn old_settings_get_onboarding_and_notify_defaults() {
    let t = tempfile::tempdir().unwrap();
    fs::write(
        t.path().join("settings.json"),
        r#"{"language":"system","autostart":true,"firstRunDone":true,"serverUrl":"https://green.kiacms.co.kr"}"#,
    )
    .unwrap();
    let s = Store::new(t.path().to_path_buf()).get_settings().unwrap();
    assert!(!s.onboarding_done);
    assert!(s.notify_manual_start && s.notify_manual_end && s.notify_watch_end);
    assert!(s.autostart);
    let d = Settings::default();
    assert!(d.notify_manual_start && d.notify_manual_end && d.notify_watch_end);
    let v = serde_json::to_value(&d).unwrap();
    for k in [
        "onboardingDone",
        "notifyManualStart",
        "notifyManualEnd",
        "notifyWatchEnd",
    ] {
        assert!(v.get(k).is_some(), "{k}");
    }
}

#[test]
fn old_run_without_trigger_reads_none() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let id = "00000000-0000-4000-8000-000000000001";
    fs::create_dir_all(t.path().join("runs")).unwrap();
    let old = json!({
        "run": {"runId": id, "status": "succeeded", "validationErrors": [], "rejectReason": null, "nodes": {}},
        "workflowId": "w", "startedAt": "2026-01-01T00:00:00Z", "finishedAt": "2026-01-01T00:00:01Z",
        "fileOps": [], "allowedRoots": [], "undo": null
    });
    fs::write(t.path().join(format!("runs/{id}.json")), old.to_string()).unwrap();

    assert_eq!(store.list_runs(None).unwrap()[0].trigger, None);
    assert_eq!(store.get_run(id).unwrap().trigger, None);
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

#[test]
fn account_suggest_defaults_on() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.notify_account_suggest);
    assert!(Settings::default().notify_account_suggest);
}
