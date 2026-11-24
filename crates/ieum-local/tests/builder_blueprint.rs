//! Builder 설계서 동작 — ui·bind-folder·설계서를 거치지 않은 변경의 취소·터미널 거절.

mod builder_support;

use builder_support::MemHost;
use ieum_local::builder::*;
use serde_json::{Value, json};

const W: &str = "11111111-1111-4111-8111-111111111111";

async fn run(b: &Builder, op: Value) -> Reply {
    b.apply(W, Op::from_value(op).unwrap(), None).await
}

/// t(새 파일, 폴더 비움) → m(옮기기)
fn wf() -> Value {
    json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {}}
        ],
        "edges": [{"source": "t", "target": "m"}]
    })
}

/// 감시 폴더 칸(watch) 하나와 제출 버튼뿐인 설계서.
fn ui() -> Value {
    json!({"op": "ui", "args": {"messages": [{"createSurface": {
        "surfaceId": "blueprint", "catalogId": "ieum.chat.blueprint/v1",
        "components": [
            {"id": "root", "component": "Blueprint", "title": "정리", "children": ["watch", "go"]},
            {"id": "watch", "component": "AppGate", "slot": "watch",
                "target": {"event": "local.folder_new_file", "field": "folder"}},
            {"id": "go", "component": "SubmitButton"}
        ],
        "dataModel": {}
    }}]}})
}

fn bind(id: &str, field: &str, slot: &str) -> Value {
    json!({"op": "bind-folder", "args": {"id": id, "field": field, "slot": slot}})
}

fn blueprint(h: &MemHost) -> Blueprint {
    h.load_chat(W).unwrap().state.blueprint.unwrap()
}

fn set_path(h: &MemHost, path: &str) {
    h.update_chat(W, &mut |c| {
        let bp = c.state.blueprint.as_mut().unwrap();
        bp.slots.get_mut("watch").unwrap().path = Some(path.into());
    })
    .unwrap();
}

/// 설계서를 띄우고 사람이 watch 칸에서 /h/in을 고른 뒤 제출한 상태.
async fn submitted(h: &MemHost, b: &Builder) {
    assert!(run(b, ui()).await.ok);
    set_path(h, "/h/in");
    h.update_chat(W, &mut |c| {
        c.state.blueprint.as_mut().unwrap().status = BlueprintStatus::Submitted;
    })
    .unwrap();
}

#[tokio::test]
async fn ui_stores_blueprint_and_returns_version() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    let r = run(&b, ui()).await;
    assert_eq!(r.data, json!({"status": "awaiting_user", "version": 1}));
    assert_eq!(blueprint(&h).status, BlueprintStatus::Active);
    assert_eq!(h.saves(), 0); // 워크플로우는 건드리지 않는다
    assert!(h.doc(W).enabled);
    assert_eq!(run(&b, ui()).await.data["version"], 2);
}

#[tokio::test]
async fn ui_invalid_reply_carries_rule() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    let mut op = ui();
    let c = &mut op["args"]["messages"][0]["createSurface"]["components"];
    c[0]["children"].as_array_mut().unwrap().push("img".into());
    c.as_array_mut()
        .unwrap()
        .push(json!({"id": "img", "component": "Image"}));
    let r = run(&b, op).await;
    assert_eq!(r.code, Some(Code::UiInvalid));
    assert_eq!(r.data["rule"], "component_type");
    assert!(h.load_chat(W).unwrap().state.blueprint.is_none());
}

#[tokio::test]
async fn bind_folder_requires_submitted() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::BlueprintNotSubmitted)); // 설계서 없음
    assert!(run(&b, ui()).await.ok);
    set_path(&h, "/h/in");
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::BlueprintNotSubmitted)); // 아직 Active
    assert_eq!(h.saves(), 0);
}

#[tokio::test]
async fn bind_folder_sets_field_without_leaking_path() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    submitted(&h, &b).await;
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(h.doc(W).workflow.nodes[0].config["folder"], "/h/in");
    assert!(!r.to_line().contains("/h/in"));
    assert!(!h.doc(W).enabled); // commit 경로 — 켜진 워크플로우는 꺼진다
    let s = &blueprint(&h).slots["watch"];
    assert_eq!(s.path, None);
    assert_eq!(s.bound_node.as_deref(), Some("t"));
    assert_eq!(blueprint(&h).status, BlueprintStatus::Submitted); // 취소하지 않는다
}

#[tokio::test]
async fn bind_folder_rejects_mismatch_unknown_and_reuse() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    submitted(&h, &b).await;
    let r = run(&b, bind("m", "baseFolder", "watch")).await;
    assert_eq!(r.code, Some(Code::SlotMismatch));
    let r = run(&b, bind("t", "folder", "dest")).await;
    assert_eq!(r.code, Some(Code::SlotUnknown));
    assert_eq!(h.saves(), 0);
    assert!(run(&b, bind("t", "folder", "watch")).await.ok);
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::SlotEmpty)); // 한 번만 쓴다
}

#[tokio::test]
async fn bind_folder_rechecks_folder_with_code_only() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    submitted(&h, &b).await;
    h.bad_folder("/h/in");
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::FolderNotAllowed));
    assert_eq!(r.message, Some(Code::FolderNotAllowed.message().into()));
    assert!(!r.to_line().contains("/h/in"));
    assert_eq!(h.saves(), 0);
}

#[tokio::test]
async fn bind_folder_respects_user_editing_and_stale() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    submitted(&h, &b).await;
    h.set_editing(W, true);
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::UserEditing));
    h.set_editing(W, false);
    h.fail_next_save(SaveError::Stale);
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::Stale));
    assert_eq!(h.saves(), 0);
    // 저장하지 못했으면 슬롯은 쓰지 않은 그대로다.
    assert_eq!(blueprint(&h).slots["watch"].bound_node, None);
    assert!(run(&b, bind("t", "folder", "watch")).await.ok);
}

#[tokio::test]
async fn bind_folder_treats_tampered_blueprint_as_absent() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    submitted(&h, &b).await;
    h.update_chat(W, &mut |c| {
        let bp = c.state.blueprint.as_mut().unwrap();
        let s = bp.slots.get_mut("watch").unwrap();
        (s.event, s.field) = ("local.ai".into(), "prompt".into());
        for g in bp.components.iter_mut().filter(|g| g["id"] == "watch") {
            g["target"] = json!({"event": "local.ai", "field": "prompt"});
        }
    })
    .unwrap();
    assert!(!validate_stored(&blueprint(&h)));
    let r = run(&b, bind("t", "folder", "watch")).await;
    assert_eq!(r.code, Some(Code::BlueprintNotSubmitted));
    assert_eq!(h.doc(W).workflow.nodes[0].config.get("folder"), None);
    assert_eq!(h.saves(), 0);
}

#[tokio::test]
async fn changing_op_cancels_active_blueprint_but_outline_does_not() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    assert!(run(&b, ui()).await.ok);
    set_path(&h, "/h/in");
    let outline =
        json!({"op": "outline", "args": {"name": "새 이름", "steps": ["받기", "옮기기"]}});
    assert!(run(&b, outline).await.ok);
    assert_eq!(blueprint(&h).status, BlueprintStatus::Active);
    // 실패한 변경은 취소하지 않는다.
    let rename = json!({"op": "rename", "args": {"id": "없음", "label": "x"}});
    assert!(!run(&b, rename).await.ok);
    assert_eq!(blueprint(&h).status, BlueprintStatus::Active);
    let add = json!({"op": "add", "args": {"event": "local.file_trash", "label": "버리기"}});
    assert!(run(&b, add).await.ok);
    let bp = blueprint(&h);
    assert_eq!(bp.status, BlueprintStatus::Cancelled);
    assert!(bp.slots.values().all(|s| s.path.is_none()));
}

#[test]
fn terminal_rejects_ui_and_bind_folder() {
    for op in [
        json!({"op": "ui", "args": {"messages": []}}),
        bind("t", "folder", "watch"),
    ] {
        assert!(Op::from_value(op.clone()).is_ok(), "{op}"); // 채팅에서는 아는 동작
        let mut line = op.clone();
        line["token"] = "x".into();
        let r = Request::parse(&line.to_string()).unwrap_err();
        assert_eq!(r.code, Some(Code::BadRequest), "{op}");
    }
}

#[tokio::test]
async fn submit_requires_active_same_version_and_all_slots() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    let data = json!({"name": "정리"});
    let r = b.submit_blueprint(W, 1, data.clone()).await;
    assert_eq!(r.code, Some(Code::Stale)); // 설계서 없음
    assert!(run(&b, ui()).await.ok);
    let r = b.submit_blueprint(W, 2, data.clone()).await;
    assert_eq!(r.code, Some(Code::Stale)); // 사람이 본 판이 아니다
    let r = b.submit_blueprint(W, 1, data.clone()).await;
    assert_eq!(r.code, Some(Code::SlotEmpty));
    set_path(&h, "/h/in");
    let r = b.submit_blueprint(W, 1, data.clone()).await;
    assert!(r.ok, "{r:?}");
    let bp = blueprint(&h);
    assert_eq!(bp.status, BlueprintStatus::Submitted);
    assert!(bp.build_pending);
    assert_eq!(bp.build_attempts, 0);
    assert_eq!(bp.data_model, data);
    // 제출한 판은 다시 제출할 수 없다.
    let r = b.submit_blueprint(W, 1, data).await;
    assert_eq!(r.code, Some(Code::Stale));
}

#[tokio::test]
async fn submit_with_no_app_gates_is_allowed() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    let mut op = ui();
    let c = &mut op["args"]["messages"][0]["createSurface"]["components"];
    c[0]["children"] = json!(["go"]);
    c.as_array_mut().unwrap().remove(1);
    assert!(run(&b, op).await.ok);
    let r = b.submit_blueprint(W, 1, json!({})).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(blueprint(&h).status, BlueprintStatus::Submitted);
}

#[tokio::test]
async fn submit_rejects_slot_path_in_data() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    assert!(run(&b, ui()).await.ok);
    set_path(&h, "/h/in");
    let r = b.submit_blueprint(W, 1, json!({"note": "/h/in/a"})).await;
    assert_eq!(r.code, Some(Code::UiInvalid));
    assert_eq!(r.data["rule"], "data_slot_path");
    assert!(!r.to_line().contains("/h/in"));
    let bp = blueprint(&h);
    assert_eq!(bp.status, BlueprintStatus::Active);
    assert_eq!(bp.data_model, json!({}));
}

#[tokio::test]
async fn pick_slot_saves_path_after_dialog() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    assert!(run(&b, ui()).await.ok);
    let r = b.pick_slot(W, "dest", 1).await;
    assert_eq!(r.code, Some(Code::SlotUnknown));
    assert!(h.pick_titles().is_empty()); // 창을 띄우기 전에 거절
    h.push_pick(Some("/h/in".into()));
    let r = b.pick_slot(W, "watch", 1).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.data["path"], "/h/in");
    let s = &blueprint(&h).slots["watch"];
    assert_eq!(s.path.as_deref(), Some("/h/in"));
    assert_eq!(s.picked_version, Some(1));
    assert!(h.pick_titles().last().unwrap().contains("고르기"));
    assert_eq!(h.saves(), 0); // 워크플로우는 건드리지 않는다
}

#[tokio::test]
async fn pick_slot_discards_when_blueprint_changed_during_dialog() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    assert!(run(&b, ui()).await.ok);
    h.push_pick(Some("/h/in".into()));
    h.on_pick(|host| {
        host.update_chat(W, &mut |c| {
            c.state.blueprint.as_mut().unwrap().version = 2;
        })
        .unwrap();
    });
    let r = b.pick_slot(W, "watch", 1).await;
    assert_eq!(r.code, Some(Code::Stale));
    assert!(!r.to_line().contains("/h/in"));
    assert_eq!(blueprint(&h).slots["watch"].path, None);
}

#[tokio::test]
async fn pick_slot_rejects_bad_folder_and_cancel() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    assert!(run(&b, ui()).await.ok);
    h.bad_folder("/h/in");
    h.push_pick(Some("/h/in".into()));
    let r = b.pick_slot(W, "watch", 1).await;
    assert_eq!(r.code, Some(Code::FolderNotAllowed));
    assert!(!r.to_line().contains("/h/in"));
    assert_eq!(blueprint(&h).slots["watch"].path, None);
    let r = b.pick_slot(W, "watch", 1).await; // 고를 것이 없음 = 취소
    assert_eq!(r.code, Some(Code::PickCancelled));
    assert_eq!(blueprint(&h).slots["watch"].path, None);
}

#[tokio::test]
async fn sync_data_only_while_active() {
    let h = MemHost::with(W, wf());
    let b = Builder::new(h.clone()).unwrap();
    assert!(b.sync_blueprint_data(W, json!({"a": 0})).await.ok); // 설계서 없음
    assert!(h.load_chat(W).unwrap().state.blueprint.is_none());
    assert!(run(&b, ui()).await.ok);
    assert!(b.sync_blueprint_data(W, json!({"a": 1})).await.ok);
    assert_eq!(blueprint(&h).data_model, json!({"a": 1}));
    h.update_chat(W, &mut |c| {
        c.state.blueprint.as_mut().unwrap().status = BlueprintStatus::Submitted;
    })
    .unwrap();
    let r = b.sync_blueprint_data(W, json!({"a": 2})).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(blueprint(&h).data_model, json!({"a": 1}));
}
