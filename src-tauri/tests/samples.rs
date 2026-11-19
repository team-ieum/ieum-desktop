//! 노드 테스트 샘플 저장소. 모든 파일은 임시 폴더 안에서만 만든다.

use std::collections::BTreeMap;

use ieum_desktop_lib::store::{MAX_STORED_BODY_UNITS, Sample, SampleStatus, Store, StoredWorkflow};
use ieum_engine::Workflow;
use serde_json::{Value, json};

const ID: &str = "3fa85f64-5717-4562-b3fc-2c963f66afa6";

fn sample(event: &str, out: Value) -> Sample {
    Sample {
        status: SampleStatus::Succeeded,
        output: out.as_object().cloned(),
        error: None,
        error_code: None,
        tested_at: "2026-01-01T00:00:00.000Z".into(),
        node: json!({"event": event, "config": {}}),
    }
}

fn one(k: &str, s: Sample) -> BTreeMap<String, Sample> {
    BTreeMap::from([(k.to_string(), s)])
}

#[test]
fn put_then_get_overwrites_per_node() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    assert!(store.get_samples(ID).unwrap().is_empty());

    store
        .put_samples(ID, one("a", sample("x", json!({"v": 1}))))
        .unwrap();
    store
        .put_samples(ID, one("b", sample("x", json!({"v": 2}))))
        .unwrap();
    let mut failed = sample("x", json!({}));
    failed.status = SampleStatus::Failed;
    failed.output = None;
    failed.error = Some("깨짐".into());
    failed.error_code = Some("E".into());
    store.put_samples(ID, one("a", failed)).unwrap();

    let got = store.get_samples(ID).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got["a"].status, SampleStatus::Failed);
    assert_eq!(got["a"].error.as_deref(), Some("깨짐"));
    assert_eq!(got["b"].output.as_ref().unwrap()["v"], json!(2));
    let raw = serde_json::to_value(&got["a"]).unwrap();
    assert_eq!(raw["errorCode"], "E");
    assert_eq!(raw["testedAt"], "2026-01-01T00:00:00.000Z");
}

#[test]
fn delete_workflow_removes_samples() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let wf: Workflow = serde_json::from_value(json!({"nodes": [], "edges": []})).unwrap();
    let saved = store
        .save_workflow(StoredWorkflow {
            id: ID.into(),
            name: "n".into(),
            enabled: false,
            workflow: wf,
            updated_at: String::new(),
        })
        .unwrap();
    store
        .put_samples(&saved.id, one("a", sample("x", json!({}))))
        .unwrap();
    assert!(t.path().join("samples").join(format!("{ID}.json")).exists());
    store.delete_workflow(&saved.id).unwrap();
    assert!(!t.path().join("samples").join(format!("{ID}.json")).exists());
    assert!(store.get_samples(ID).unwrap().is_empty());
}

#[test]
fn bad_id_rejected() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    assert!(store.get_samples("../evil").is_err());
    assert!(store.put_samples("../evil", BTreeMap::new()).is_err());
    assert!(!t.path().join("samples").exists());
}

#[test]
fn http_body_clipped_in_sample() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let big = "가".repeat(MAX_STORED_BODY_UNITS + 10);
    store
        .put_samples(
            ID,
            one(
                "h",
                sample("http.request", json!({"body": big, "status": 200})),
            ),
        )
        .unwrap();
    store
        .put_samples(ID, one("o", sample("other.node", json!({"body": big}))))
        .unwrap();
    let got = store.get_samples(ID).unwrap();
    let body = got["h"].output.as_ref().unwrap()["body"].as_str().unwrap();
    assert!(body.contains("잘림"));
    assert!(body.encode_utf16().count() < MAX_STORED_BODY_UNITS + 100);
    assert_eq!(got["h"].output.as_ref().unwrap()["status"], json!(200));
    // 다른 노드의 출력은 건드리지 않는다.
    assert_eq!(
        got["o"].output.as_ref().unwrap()["body"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        MAX_STORED_BODY_UNITS + 10
    );
}
