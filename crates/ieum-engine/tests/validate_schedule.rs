use std::path::Path;

use ieum_engine::{
    Catalog, ErrorCode, NodeType, ValidationError, Workflow, validate, validate_fields,
};
use serde_json::{Value, json};

const EVENTS: [&str; 6] = [
    "schedule.every",
    "schedule.hourly",
    "schedule.daily",
    "schedule.weekly",
    "schedule.monthly",
    "manual.run",
];

fn wf(event: &str, config: Value) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [{"id": "t", "type": "TRIGGER", "event": event, "config": config}],
        "edges": []
    }))
    .unwrap()
}

fn cron_errors(errors: &[ValidationError]) -> Vec<(&str, &str)> {
    errors
        .iter()
        .filter(|e| e.code == ErrorCode::Cron)
        .map(|e| (e.node_id.as_deref().unwrap(), e.field.as_deref().unwrap()))
        .collect()
}

#[test]
fn catalog_has_schedule_and_manual() {
    let stage1 = Catalog::stage1(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    for catalog in [Catalog::embedded().unwrap(), stage1] {
        for event in EVENTS {
            let def = catalog.get(event).unwrap_or_else(|| panic!("{event}"));
            assert_eq!(def.node_type, NodeType::Trigger, "{event}");
        }
    }
}

#[test]
fn cron_invalid() {
    let catalog = Catalog::embedded().unwrap();
    for cron in [
        json!("0 9 * *"),
        json!("0 9 1 * 1"),
        json!("abc def ghi jkl mno"),
        json!("0 9 * * +1"),
        json!("0 0 99 * * ?"),
        json!(42),
    ] {
        let errors = validate(&wf("schedule.every", json!({"cron": cron})), &catalog);
        assert_eq!(cron_errors(&errors), [("t", "config.cron")], "{cron}");
    }
    for cron in [json!("0 9 * * 1-5"), json!("0 0 9 ? * MON-FRI"), json!("")] {
        let errors = validate(&wf("schedule.every", json!({"cron": cron})), &catalog);
        assert!(errors.is_empty(), "{cron}: {errors:?}");
    }
    assert!(validate(&wf("schedule.every", json!({})), &catalog).is_empty());
}

#[test]
fn daily_hour_choice() {
    let catalog = Catalog::embedded().unwrap();
    let w = wf("schedule.daily", json!({"hour": "24"}));
    let fields = validate_fields(&w, &catalog);
    assert_eq!(fields.len(), 1, "{fields:?}");
    assert_eq!(fields[0].code, ErrorCode::Choice);
    assert_eq!(fields[0].field.as_deref(), Some("config.hour"));
    assert!(validate(&w, &catalog).is_empty());
}

#[test]
fn cron_code_serializes() {
    assert_eq!(
        serde_json::to_value(ErrorCode::Cron).unwrap(),
        json!("CRON")
    );
}
