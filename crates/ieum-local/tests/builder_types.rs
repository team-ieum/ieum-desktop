use ieum_local::builder::*;
use serde_json::json;

#[test]
fn op_wire_shape_roundtrips() {
    let op = Op::Add {
        event: "local.ai".into(),
        label: "분류".into(),
        after: Some("t".into()),
        branch: None,
    };
    let v = serde_json::to_value(&op).unwrap();
    assert_eq!(
        v,
        json!({"op": "add", "args": {"event": "local.ai", "label": "분류", "after": "t"}})
    );
    assert_eq!(Op::from_value(v).unwrap(), op);
    assert_eq!(
        Op::from_value(json!({"op": "pick-folder", "args": {"id": "t", "field": "folder"}}))
            .unwrap(),
        Op::PickFolder {
            id: "t".into(),
            field: "folder".into()
        }
    );
    // args 없음·빈 args 둘 다 받는다.
    assert_eq!(
        Op::from_value(json!({"op": "status"})).unwrap(),
        Op::Status {}
    );
    assert_eq!(
        Op::from_value(json!({"op": "finish", "args": {}})).unwrap(),
        Op::Finish {}
    );
    assert_eq!(
        serde_json::to_value(Op::Guide {}).unwrap(),
        json!({"op": "guide", "args": {}})
    );
}

#[test]
fn no_op_can_enable_or_confirm() {
    for op in ["confirm-finish", "enable", "run", "save"] {
        let r = Op::from_value(json!({"op": op, "args": {}})).unwrap_err();
        assert_eq!(r.code, Some(Code::BadRequest), "{op}");
    }
    assert!(Op::from_value(json!({"op": "set", "args": {"id": "a"}})).is_err()); // values 없음
}

#[test]
fn mutating_ops() {
    assert!(
        Op::Rename {
            id: "a".into(),
            label: "b".into()
        }
        .mutates()
    );
    assert!(
        Op::PickFolder {
            id: "a".into(),
            field: "folder".into()
        }
        .mutates()
    );
    for op in [
        Op::Status {},
        Op::Events {},
        Op::Finish {},
        Op::Guide {},
        Op::Fields { id: "a".into() },
        Op::Test {
            id: "a".into(),
            file: None,
        },
    ] {
        assert!(!op.mutates(), "{op:?}");
    }
}

#[test]
fn reply_shape_and_exit_codes() {
    assert_eq!(
        Reply::ok(json!({"id": "n1"})).to_line(),
        r#"{"ok":true,"data":{"id":"n1"}}"#
    );
    assert_eq!(Reply::ok(()).to_line(), r#"{"ok":true}"#);
    let e = Reply::err(Code::FileMissing);
    assert_eq!(
        e.to_line(),
        r#"{"ok":false,"code":"FILE_MISSING","message":"옮길 파일이 없음"}"#
    );
    assert_eq!((Reply::ok(()).exit_code(), e.exit_code()), (0, 1));
    assert_eq!(Reply::err(Code::FieldLocked).exit_code(), 2);
    assert_eq!(Reply::err(Code::NotReady).exit_code(), 2);
    assert_eq!(Reply::err(Code::BadRequest).exit_code(), 3);
    assert_eq!(Reply::err(Code::Unauthorized).exit_code(), 3);
    assert_eq!(
        serde_json::to_value(Code::DestOccupied).unwrap(),
        json!("DEST_OCCUPIED")
    );
    assert_eq!(
        serde_json::to_value(Code::AiFailed).unwrap(),
        json!("AI_FAILED")
    );
    assert_eq!(
        serde_json::to_value(Code::FolderNotAllowed).unwrap(),
        json!("FOLDER_NOT_ALLOWED")
    );
    let u = Reply::err(Code::UserEditing);
    assert_eq!(
        u.to_line(),
        r#"{"ok":false,"code":"USER_EDITING","message":"사용자가 캔버스에서 고치는 중입니다. 저장한 뒤 다시 시도하세요."}"#
    );
    assert_eq!(u.exit_code(), 2);
}

#[test]
fn chat_file_shape() {
    // 빈 파일·옛 파일도 읽힌다.
    let empty: ChatFile = serde_json::from_value(json!({})).unwrap();
    assert!(
        empty.messages.is_empty()
            && empty.state.baseline.is_none()
            && empty.state.pending_finish.is_none()
    );
    let v = json!({
        "messages": [{"role": "user", "text": "정리해 줘", "at": "2026-11-13T00:00:00.000Z"}],
        "state": {
            "outline": ["새 파일", "옮기기"],
            "baseline": {"t": "h0"},
            "tests": {"t": {"hash": "h0", "status": "success"}},
            "pendingFinish": {"hash": "w", "folder": "/h/in", "actions": [{"nodeId": "m", "event": "local.file_move", "label": "옮기기", "target": "/h/out"}]}
        },
        "usage": []
    });
    let f: ChatFile = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(&f).unwrap(), v);
}

fn node(event: &str, config: serde_json::Value) -> ieum_engine::Node {
    serde_json::from_value(json!({"id": "n", "event": event, "config": config})).unwrap()
}

#[test]
fn node_hash_is_key_order_independent_and_pinned() {
    let a = node(
        "local.file_move",
        json!({"path": "p", "baseFolder": "/h/out", "destination": "a"}),
    );
    let b = node(
        "local.file_move",
        json!({"destination": "a", "baseFolder": "/h/out", "path": "p"}),
    );
    assert_eq!(node_hash(&a), node_hash(&b));
    assert_eq!(
        node_hash(&a),
        "9043e85a8f4bda9f604e6e2b1b09f130c8bb7a69d41884295a8b477a1bc62a2e"
    );
    let t = node(
        "local.file_trash",
        json!({"path": "{{nodes.t.output.path}}"}),
    );
    assert_eq!(
        node_hash(&t),
        "20b7c2077e565426b681060a25ef2816654a410dff316ef4bd43f277c9af6175"
    );
    // label·position은 hash에 안 들어간다.
    let mut renamed = t.clone();
    renamed.label = Some("다른 이름".into());
    assert_eq!(node_hash(&renamed), node_hash(&t));
}

#[test]
fn node_state_rules() {
    let n = node("local.file_trash", json!({"path": "x"}));
    let h = node_hash(&n);
    let mut s = ChatState::default();
    assert_eq!(node_state(&n, &s), NodeState::Untested);
    s.baseline = Some([("n".to_string(), h.clone())].into());
    assert_eq!(node_state(&n, &s), NodeState::Passed); // 채팅 시작 때 있던 그대로
    s.tests.insert(
        "n".into(),
        TestEntry {
            hash: h.clone(),
            status: TestStatus::Failed,
        },
    );
    assert_eq!(node_state(&n, &s), NodeState::Failed); // 기록이 baseline보다 먼저
    s.tests.insert(
        "n".into(),
        TestEntry {
            hash: "old".into(),
            status: TestStatus::Success,
        },
    );
    assert_eq!(node_state(&n, &s), NodeState::Untested); // 기록이 있는데 hash가 다르면 baseline과 같아도 untested
    assert!(
        NodeState::Skipped.is_done()
            && NodeState::Passed.is_done()
            && !NodeState::Untested.is_done()
    );
}

#[test]
fn request_line() {
    let r = Request::parse(r#"{"token":"tk","op":"rename","args":{"id":"a","label":"새 이름"}}"#)
        .unwrap();
    assert_eq!(
        (r.token.as_str(), &r.op),
        (
            "tk",
            &Op::Rename {
                id: "a".into(),
                label: "새 이름".into()
            }
        )
    );
    assert_eq!(Request::parse(&r.to_line()).unwrap().op, r.op);
    assert!(!r.to_line().contains('\n'));
    assert_eq!(
        Request::parse(r#"{"op":"status"}"#).unwrap_err().code,
        Some(Code::Unauthorized)
    );
    assert_eq!(
        Request::parse("not json").unwrap_err().code,
        Some(Code::BadRequest)
    );
}
