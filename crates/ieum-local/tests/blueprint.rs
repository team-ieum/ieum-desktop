use ieum_local::builder::*;
use serde_json::{Value, json};

fn create(components: Vec<Value>) -> Value {
    json!({"createSurface": {
        "surfaceId": "blueprint", "catalogId": "ieum.chat.blueprint/v1",
        "components": components,
        "dataModel": {"pick": ["date"], "fmt": "ymd", "pdf": true, "name": "날짜.pdf"}
    }})
}

fn gate(id: &str, event: &str, field: &str) -> Value {
    json!({"id": id, "component": "AppGate", "slot": id, "target": {"event": event, "field": field}})
}

/// 8종을 모두 담은 정상 설계서 — LLM 표기(`{"path"}`).
fn comps() -> Vec<Value> {
    vec![
        json!({"id": "root", "component": "Blueprint", "title": "영수증 정리",
            "children": ["flow", "pick", "fmt", "pdf", "name", "watch", "go"]}),
        json!({"id": "flow", "component": "FlowPreview",
            "steps": [{"label": "새 파일", "event": "local.folder_new_file"}, {"label": "옮기기"}]}),
        json!({"id": "pick", "component": "ChipSelect", "label": "뽑을 것", "multiple": true,
            "options": [{"value": "date", "label": "날짜"}, {"value": "shop", "label": "가게"}],
            "value": {"path": "/pick"}}),
        json!({"id": "fmt", "component": "Select", "label": "날짜 형식",
            "options": [{"value": "ymd", "label": "2026-11-24"}, {"value": "dmy", "label": "24.11.2026"}],
            "value": {"path": "/fmt"}}),
        json!({"id": "pdf", "component": "Checkbox", "label": "PDF만", "value": {"path": "/pdf"}}),
        json!({"id": "name", "component": "TextInput", "label": "파일 이름", "hint": "예: 날짜_가게.pdf",
            "value": {"path": "/name"}}),
        gate("watch", "local.folder_new_file", "folder"),
        json!({"id": "go", "component": "SubmitButton"}),
    ]
}

/// `comps()`에 `extra`를 더하고 root에서 닿게 한다.
fn plus(extra: Value) -> Value {
    let mut c = comps();
    c[0]["children"]
        .as_array_mut()
        .unwrap()
        .push(extra["id"].clone());
    c.push(extra);
    json!([create(c)])
}

fn ok(current: Option<&Blueprint>, msgs: Value) -> Blueprint {
    apply_ui(current, msgs.as_array().unwrap()).unwrap()
}

fn rule(current: Option<&Blueprint>, msgs: Value) -> &'static str {
    let e = apply_ui(current, msgs.as_array().unwrap()).unwrap_err();
    assert!(UI_RULES.contains(&e.rule), "UI_RULES 밖의 rule: {}", e.rule);
    e.rule
}

/// 키가 `path` 하나뿐인 객체(v1.0에서 바인딩이 아닌 맨 객체)가 어딘가에 있는가.
fn has_bare_path(v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            (o.len() == 1 && o.contains_key("path")) || o.values().any(has_bare_path)
        }
        Value::Array(a) => a.iter().any(has_bare_path),
        _ => false,
    }
}

fn base() -> Blueprint {
    ok(None, json!([create(comps())]))
}

/// watch(감시 폴더)·dest(기준 폴더) 두 칸을 가진 설계서.
fn two_gates() -> Blueprint {
    ok(None, plus(gate("dest", "local.file_move", "baseFolder")))
}

fn update_components(c: Vec<Value>) -> Value {
    json!([{"updateComponents": {"surfaceId": "blueprint", "components": c}}])
}

#[test]
fn create_surface_converts_path_bindings_to_v1() {
    let b = base();
    assert!(
        serde_json::to_string(&b.components)
            .unwrap()
            .contains(r#"{"@path":"/name"}"#)
    );
    assert!(!has_bare_path(&Value::Array(b.components.clone())));
    assert_eq!(b.version, 1);
    assert_eq!(b.status, BlueprintStatus::Active);
    assert_eq!(b.components.len(), 8);
    assert_eq!(
        b.slots["watch"],
        Slot {
            event: "local.folder_new_file".into(),
            field: "folder".into(),
            path: None,
            ..Default::default()
        }
    );
}

#[test]
fn accepts_optional_version_key() {
    let mut m = create(comps());
    m["version"] = json!("1.0");
    assert_eq!(ok(None, json!([m.clone()])).version, 1);
    m["version"] = json!("0.9");
    assert_eq!(rule(None, json!([m])), "message_kind");
    assert_eq!(rule(None, json!([{"version": "1.0"}])), "message_kind");
}

#[test]
fn rejects_types_outside_catalog() {
    let img = json!({"id": "img", "component": "Image", "url": "https://x"});
    assert_eq!(rule(None, plus(img)), "component_type");
}

#[test]
fn rejects_at_and_fullwidth_at_keys() {
    for v in [
        json!({"@call": "x"}),
        json!({"＠path": "/x"}),
        json!({"@path": "/x"}),
    ] {
        let c = json!({"id": "t", "component": "TextInput", "label": "칸", "value": v});
        assert_eq!(rule(None, plus(c)), "binding", "{v}");
    }
    // 바인딩 자리 밖의 `@` 키도 같은 rule이다(모르는 prop으로 뭉개지지 않게).
    let c = json!({"id": "t", "component": "TextInput", "label": "칸", "value": {"path": "/t"}, "＠call": "x"});
    assert_eq!(rule(None, plus(c)), "binding");
}

#[test]
fn rejects_second_appgate_with_same_target() {
    let again = gate("again", "local.folder_new_file", "folder");
    assert_eq!(rule(None, plus(again)), "slot_duplicate_target");
}

#[test]
fn rejects_target_outside_folder_fields() {
    assert_eq!(
        rule(None, plus(gate("p", "local.ai", "prompt"))),
        "slot_target"
    );
}

#[test]
fn requires_one_root_blueprint_and_one_submit() {
    let mut c = comps();
    c[0]["id"] = json!("main");
    assert_eq!(rule(None, json!([create(c)])), "root");
    let mut c = comps();
    c[0]["children"]
        .as_array_mut()
        .unwrap()
        .push(json!("ghost"));
    assert_eq!(rule(None, json!([create(c)])), "root");

    let mut c = comps();
    c.retain(|x| x["id"] != "go");
    c[0]["children"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x != "go");
    assert_eq!(rule(None, json!([create(c)])), "submit_count");
    let go2 = json!({"id": "go2", "component": "SubmitButton"});
    assert_eq!(rule(None, plus(go2)), "submit_count");
}

#[test]
fn truncates_long_text_and_strips_control_chars() {
    let mut c = comps();
    c[0]["title"] = json!("가".repeat(60));
    c[5]["label"] = json!("파일\u{7} 이름\n규칙");
    let b = ok(None, json!([create(c)]));
    assert_eq!(b.components[0]["title"], json!("가".repeat(40)));
    assert_eq!(b.components[5]["label"], "파일 이름 규칙");
}

#[test]
fn update_without_active_is_no_surface() {
    let msg =
        json!([{"updateDataModel": {"surfaceId": "blueprint", "path": "/name", "value": "a"}}]);
    let e = apply_ui(None, msg.as_array().unwrap()).unwrap_err();
    assert_eq!(e.rule, "no_surface");
    assert_eq!(e.reply().code, Some(Code::UiNoSurface));

    let mut done = base();
    done.status = BlueprintStatus::Submitted;
    assert_eq!(rule(Some(&done), msg), "no_surface");
}

#[test]
fn update_components_swapping_targets_clears_both_paths() {
    let mut b = two_gates();
    b.slots.get_mut("watch").unwrap().path = Some("/h/in".into());
    b.slots.get_mut("dest").unwrap().path = Some("/h/out".into());

    // 같은 target이면 경로가 남는다.
    let same = ok(
        Some(&b),
        update_components(vec![gate("watch", "local.folder_new_file", "folder")]),
    );
    assert_eq!(same.slots["watch"].path.as_deref(), Some("/h/in"));
    assert_eq!(same.version, 2);

    let swapped = ok(
        Some(&b),
        update_components(vec![
            gate("watch", "local.file_move", "baseFolder"),
            gate("dest", "local.folder_new_file", "folder"),
        ]),
    );
    assert_eq!(swapped.slots["watch"].path, None);
    assert_eq!(swapped.slots["dest"].path, None);
    assert_eq!(swapped.slots["watch"].event, "local.file_move");
}

#[test]
fn update_data_model_sets_and_removes_by_pointer() {
    let b = base();
    let set = ok(
        Some(&b),
        json!([{"updateDataModel": {"surfaceId": "blueprint", "path": "/name", "value": "a"}}]),
    );
    assert_eq!(set.data_model["name"], "a");
    assert_eq!(set.version, 2);
    let removed = ok(
        Some(&set),
        json!([{"updateDataModel": {"surfaceId": "blueprint", "path": "/name", "value": null}}]),
    );
    assert!(removed.data_model.get("name").is_none());
    assert_eq!(removed.data_model["fmt"], "ymd");
}

#[test]
fn orphaned_components_are_pruned_with_their_slots() {
    let b = two_gates();
    assert!(b.slots.contains_key("dest"));
    let mut root = comps().remove(0);
    root["children"]
        .as_array_mut()
        .unwrap()
        .retain(|x| x != "dest");
    let after = ok(Some(&b), update_components(vec![root]));
    assert!(after.components.iter().all(|c| c["id"] != "dest"));
    assert!(!after.slots.contains_key("dest"));
    assert!(after.slots.contains_key("watch"));

    let mut c = comps();
    c.push(json!({"id": "lost", "component": "TextInput", "label": "고아", "value": {"path": "/lost"}}));
    let b = ok(None, json!([create(c)]));
    assert!(b.components.iter().all(|c| c["id"] != "lost"));
}

#[test]
fn validate_stored_rejects_orphans() {
    let mut b = base();
    assert!(validate_stored(&b));
    b.components.push(json!({"id": "lost", "component": "TextInput", "label": "고아", "value": {"@path": "/lost"}}));
    assert!(!validate_stored(&b));
}

#[test]
fn validate_stored_rejects_duplicate_ids() {
    // 변조본: root·자식 id를 다시 쓴 컴포넌트는 고아 정리를 빠져나간다.
    let extra =
        json!({"id": "x", "component": "TextInput", "label": "몰래", "value": {"@path": "/x"}});
    for id in ["root", "name"] {
        let mut b = base();
        let mut dup = extra.clone();
        dup["id"] = json!(id);
        b.components.push(dup);
        assert!(!validate_stored(&b), "{id}");
    }
    // AppGate를 slots와 맞춰 몰래 더한 경우도.
    let mut b = base();
    b.components
        .push(gate("go", "local.file_move", "baseFolder"));
    b.slots.insert(
        "go".into(),
        Slot {
            event: "local.file_move".into(),
            field: "baseFolder".into(),
            ..Default::default()
        },
    );
    assert!(!validate_stored(&b));
}

#[test]
fn create_while_submitted_replaces_with_fresh_active() {
    let mut old = base();
    old.status = BlueprintStatus::Submitted;
    old.version = 3;
    old.build_pending = true;
    old.slots.get_mut("watch").unwrap().path = Some("/h/in".into());
    let b = ok(Some(&old), json!([create(comps())]));
    assert_eq!(b.status, BlueprintStatus::Active);
    assert_eq!(b.version, 4);
    assert!(!b.build_pending);
    assert!(b.slots.values().all(|s| s.path.is_none()));
}

#[test]
fn check_data_rejects_slot_path_and_oversize() {
    let mut b = base();
    b.slots.get_mut("watch").unwrap().path = Some("/h/in".into());
    let e = check_data(&b, &json!({"x": "앞/h/in/뒤"})).unwrap_err();
    assert_eq!(e.rule, "data_slot_path");
    assert!(!e.reply().to_line().contains("/h/in"));

    let fits = json!({"x": "a".repeat(4088)});
    assert_eq!(serde_json::to_vec(&fits).unwrap().len(), 4096);
    assert!(check_data(&b, &fits).is_ok());
    let over = json!({"x": "a".repeat(4089)});
    assert_eq!(serde_json::to_vec(&over).unwrap().len(), 4097);
    assert_eq!(check_data(&b, &over).unwrap_err().rule, "data_model");
    assert_eq!(check_data(&b, &json!([])).unwrap_err().rule, "data_model");
}

#[test]
fn validate_stored_rejects_tampered_target() {
    let b = base();
    assert!(validate_stored(&b));

    let mut slot_only = b.clone();
    let s = slot_only.slots.get_mut("watch").unwrap();
    s.event = "local.ai".into();
    s.field = "prompt".into();
    assert!(!validate_stored(&slot_only));

    let mut both = slot_only.clone();
    both.components[6]["target"] = json!({"event": "local.ai", "field": "prompt"});
    assert!(!validate_stored(&both));

    // 저장본은 v1.0 표기만 — 맨 `{"path"}`는 거부한다.
    let mut bare = b.clone();
    bare.components[5]["value"] = json!({"path": "/name"});
    assert!(!validate_stored(&bare));
}

#[test]
fn prompt_summary_has_no_paths_and_uses_dialect() {
    let mut b = base();
    b.slots.get_mut("watch").unwrap().path = Some("/h/in".into());
    let s = prompt_summary(&b);
    assert!(s.starts_with("상태: active\n버전: 1\n컴포넌트:\n"), "{s}");
    assert!(s.contains(r#""path":"/name""#), "{s}");
    assert!(!s.contains("@path"));
    assert!(!s.contains("/h/in"));
    assert!(s.contains("watch local.folder_new_file.folder 고름"), "{s}");
}

#[test]
fn chat_file_with_broken_blueprint_still_loads() {
    let f: ChatFile = serde_json::from_value(json!({
        "messages": [{"role": "user", "text": "정리해 줘"}],
        "state": {"outline": ["a"], "blueprint": {"status": 5}}
    }))
    .unwrap();
    assert_eq!(f.state.blueprint, None);
    assert_eq!(f.state.outline, vec!["a".to_string()]);
}

#[test]
fn chat_file_without_blueprint_has_no_key() {
    let v = serde_json::to_value(ChatFile::default()).unwrap();
    assert!(v["state"].get("blueprint").is_none());

    // 정상 설계서는 그대로 왕복한다.
    let mut f = ChatFile::default();
    f.state.blueprint = Some(base());
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["state"]["blueprint"]["status"], "active");
    assert_eq!(serde_json::from_value::<ChatFile>(v).unwrap(), f);
}

#[test]
fn blueprint_codes_are_known() {
    for c in [
        Code::UiInvalid,
        Code::UiNoSurface,
        Code::SlotUnknown,
        Code::SlotEmpty,
        Code::SlotMismatch,
        Code::BlueprintNotSubmitted,
    ] {
        assert!(!c.message().is_empty());
        assert_eq!(c.exit_code(), 2);
    }
    assert_eq!(
        serde_json::to_value(Code::UiInvalid).unwrap(),
        json!("UI_INVALID")
    );
}

#[test]
fn rejects_root_path_binding() {
    // "/"에 묶이면 입력 하나가 데이터 모델 전체를 덮어쓴다.
    let c = json!({"id": "t", "component": "TextInput", "label": "칸", "value": {"path": "/"}});
    assert_eq!(rule(None, plus(c)), "binding");
    let mut b = base();
    b.components[5]["value"] = json!({"@path": "/"});
    assert!(!validate_stored(&b));
}

#[test]
fn update_components_requires_components() {
    let b = base();
    let msg = json!([{"updateComponents": {"surfaceId": "blueprint"}}]);
    assert_eq!(rule(Some(&b), msg), "message_kind");
    // createSurface는 components 없이 열고 같은 묶음의 updateComponents로 채워도 된다(A2UI v1.0 순서).
    let open = json!({"createSurface": {"surfaceId": "blueprint", "catalogId": "ieum.chat.blueprint/v1", "dataModel": {}}});
    let fill = json!({"updateComponents": {"surfaceId": "blueprint", "components": comps()}});
    assert_eq!(ok(None, json!([open, fill])).components.len(), 8);
}

#[test]
fn object_in_item_text_is_binding() {
    let opts = json!([{"value": "a", "label": {"path": "/x"}}, {"value": "b", "label": "나"}]);
    let c = json!({"id": "s", "component": "Select", "label": "칸", "options": opts, "value": {"path": "/s"}});
    assert_eq!(rule(None, plus(c)), "binding");
    let steps = json!([{"label": {"path": "/x"}}]);
    let f = json!({"id": "f2", "component": "FlowPreview", "steps": steps});
    assert_eq!(rule(None, plus(f)), "binding");
    // 객체가 아닌 틀린 모양은 그대로 unknown_prop.
    let opts = json!([{"value": "a", "label": 3}, {"value": "b", "label": "나"}]);
    let c = json!({"id": "s", "component": "Select", "label": "칸", "options": opts, "value": {"path": "/s"}});
    assert_eq!(rule(None, plus(c)), "unknown_prop");
}
