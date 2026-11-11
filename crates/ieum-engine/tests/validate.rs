use std::path::Path;

use ieum_engine::{Catalog, ErrorCode, ValidationError, Workflow, validate};
use serde_json::{Value, json};

fn catalog() -> Catalog {
    Catalog::stage1(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn run(nodes: Value, edges: Value) -> Vec<ValidationError> {
    let wf: Workflow = serde_json::from_value(json!({"nodes": nodes, "edges": edges})).unwrap();
    validate(&wf, &catalog())
}

fn codes(errors: &[ValidationError]) -> Vec<(ErrorCode, Option<&str>, Option<&str>)> {
    errors
        .iter()
        .map(|e| (e.code, e.node_id.as_deref(), e.field.as_deref()))
        .collect()
}

fn trigger() -> Value {
    json!({"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/in"}})
}

/// 참조는 `data` 칸에 — `prompt`는 `ref: false`.
fn ai(id: &str, data: &str) -> Value {
    json!({"id": id, "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p", "data": data}})
}

fn cond(config: Value) -> Value {
    json!({"id": "c", "type": "CONDITION", "event": "branch.if", "config": config})
}

fn br(key: &str) -> Value {
    json!({"key": key, "name": "긴급", "rules": [{"left": "a", "operator": "equals", "right": "b"}]})
}

/// t → nodes[0] → nodes[1] …
fn chain(nodes: Vec<Value>) -> Vec<ValidationError> {
    let mut all = vec![trigger()];
    all.extend(nodes);
    let edges: Vec<Value> = all
        .windows(2)
        .map(|w| json!({"source": w[0]["id"], "target": w[1]["id"]}))
        .collect();
    run(Value::Array(all), Value::Array(edges))
}

use ErrorCode::*;

#[test]
fn valid_workflow_has_no_errors() {
    let errors = chain(vec![
        ai("a", "요약"),
        ai("b", "{{nodes.a.output.output}} {{nodes.t.output.path}}"),
    ]);
    assert_eq!(errors, vec![]);
}

#[test]
fn serializes_camel_case_and_screaming_codes() {
    let errors = run(json!([]), json!([]));
    assert_eq!(
        serde_json::to_value(&errors).unwrap(),
        json!([{"nodeId": null, "field": null, "code": "TRIGGER_COUNT", "message": "워크플로우에 노드가 없습니다."}])
    );
    let all = [
        (RefNotAllowed, "REF_NOT_ALLOWED"),
        (RefFormat, "REF_FORMAT"),
        (RefUnknownNode, "REF_UNKNOWN_NODE"),
        (RefSelf, "REF_SELF"),
        (RefNotUpstream, "REF_NOT_UPSTREAM"),
        (RefUnknownField, "REF_UNKNOWN_FIELD"),
        (TriggerCount, "TRIGGER_COUNT"),
        (EdgeInvalid, "EDGE_INVALID"),
        (OrphanNode, "ORPHAN_NODE"),
    ];
    for (code, s) in all {
        assert_eq!(serde_json::to_value(code).unwrap(), json!(s));
    }
}

#[test]
fn trigger_count_zero_or_two() {
    let errors = run(json!([ai("a", "x")]), json!([]));
    // 트리거가 1개가 아니면 ORPHAN_NODE는 내지 않는다
    assert_eq!(codes(&errors), vec![(TriggerCount, None, None)]);
    assert_eq!(errors[0].message, "트리거는 1개여야 합니다(현재 0개).");
    let mut t2 = trigger();
    t2["id"] = json!("t2");
    t2["type"] = json!("trigger"); // 소문자도 트리거
    let errors = run(
        json!([trigger(), t2]),
        json!([{"source": "t", "target": "t2"}]),
    );
    assert_eq!(codes(&errors), vec![(TriggerCount, None, None)]);
}

#[test]
fn edge_invalid_dup_missing_into_trigger_no_type() {
    let errors = run(
        json!([trigger(), ai("a", "x"), ai("a", "y")]),
        json!([
            {"source": "t", "target": "a"},
            {"source": "a", "target": "zz"},
            {"source": "a", "target": "t"}
        ]),
    );
    assert_eq!(
        codes(&errors),
        vec![
            (EdgeInvalid, Some("a"), None),
            (EdgeInvalid, None, None),
            (EdgeInvalid, Some("t"), None),
        ]
    );
    assert_eq!(errors[0].message, "노드 id 'a'가 중복됩니다.");
    assert_eq!(
        errors[1].message,
        "연결(a → zz)이 없는 노드 'zz'를 가리킵니다."
    );
    assert_eq!(
        errors[2].message,
        "트리거로 들어오는 연결은 둘 수 없습니다."
    );

    let no_type = json!({"id": "x", "event": "local.file_trash", "config": {}});
    let empty_id = json!({"id": "", "type": "ACTION", "event": "local.file_trash"});
    let errors = run(
        json!([trigger(), no_type, empty_id]),
        json!([{"source": "t", "target": ""}]),
    );
    assert_eq!(
        codes(&errors),
        vec![
            (EdgeInvalid, Some("x"), None),
            (EdgeInvalid, None, None),
            (EdgeInvalid, None, None),
            (OrphanNode, Some("x"), None),
        ]
    );
    assert_eq!(errors[0].message, "노드 'x'에 type이 없습니다.");
    assert_eq!(errors[1].message, "3번째 노드에 id가 없습니다.");
    assert_eq!(
        errors[2].message,
        "1번째 연결에 출발 또는 도착 노드가 없습니다."
    );
}

#[test]
fn orphan_node() {
    let mut nodes = vec![trigger(), ai("a", "x"), ai("b", "y")];
    nodes.rotate_left(1); // 트리거가 첫 노드가 아니어도 된다
    let errors = run(Value::Array(nodes), json!([{"source": "t", "target": "a"}]));
    assert_eq!(codes(&errors), vec![(OrphanNode, Some("b"), None)]);
    assert_eq!(errors[0].message, "트리거에서 이어지지 않은 노드입니다.");
}

fn branch_edges(c: Value, out: &[(&str, Option<&str>)]) -> Vec<ValidationError> {
    let x =
        json!({"id": "x", "type": "ACTION", "event": "local.file_trash", "config": {"path": "p"}});
    let y =
        json!({"id": "y", "type": "ACTION", "event": "local.file_trash", "config": {"path": "p"}});
    let mut edges = vec![json!({"source": "t", "target": "c"})];
    for (tgt, ct) in out {
        edges.push(json!({"source": "c", "target": tgt, "conditionType": ct}));
    }
    run(json!([trigger(), c, x, y]), Value::Array(edges))
}

#[test]
fn branch_edge_condition_type() {
    let c = cond(json!({"branches": [br("b_aaaaaaaa")]}));
    assert_eq!(
        branch_edges(c.clone(), &[("x", Some("b_aaaaaaaa")), ("y", Some("else"))]),
        vec![]
    );
    let errors = branch_edges(c, &[("x", Some("true")), ("y", None)]);
    assert_eq!(
        codes(&errors),
        vec![
            (EdgeInvalid, Some("c"), None),
            (EdgeInvalid, Some("c"), None)
        ]
    );
    assert_eq!(
        errors[0].message,
        "분기 'c' → 'x' 연결의 갈래 값('true')이 맞지 않아요. 쓸 수 있는 값: 'b_aaaaaaaa'(긴급), 'else'(그 외)."
    );
    assert_eq!(
        errors[1].message,
        "분기 'c' → 'y' 연결의 갈래 값(없음)이 맞지 않아요. 쓸 수 있는 값: 'b_aaaaaaaa'(긴급), 'else'(그 외)."
    );
}

#[test]
fn stopped_branch_else_edge_is_invalid() {
    let c = cond(json!({"branches": [br("b_aaaaaaaa")], "otherwise": "stop"}));
    let errors = branch_edges(c, &[("x", Some("b_aaaaaaaa")), ("y", Some("else"))]);
    assert_eq!(codes(&errors), vec![(EdgeInvalid, Some("c"), None)]);
    assert_eq!(
        errors[0].message,
        "분기 'c' → 'y' 연결: 그 외는 '여기서 멈추기'라 'else' 연결은 실행되지 않아요. 쓸 수 있는 값: 'b_aaaaaaaa'(긴급)."
    );
}

#[test]
fn legacy_branch_edges_true_false_and_non_branch_ignored() {
    let c = cond(json!({"operator": "equals", "left": "a", "right": "b"}));
    assert_eq!(
        branch_edges(c.clone(), &[("x", Some("true")), ("y", Some("false"))]),
        vec![]
    );
    assert_eq!(
        codes(&branch_edges(
            c,
            &[("x", Some("else")), ("y", Some("false"))]
        )),
        vec![(EdgeInvalid, Some("c"), None)]
    );
    let x =
        json!({"id": "x", "type": "ACTION", "event": "local.file_trash", "config": {"path": "p"}});
    let errors = run(
        json!([trigger(), x]),
        json!([{"source": "t", "target": "x", "conditionType": "zzz"}]),
    );
    assert_eq!(errors, vec![]);
}

#[test]
fn ref_not_allowed() {
    let mv = json!({"id": "m", "type": "ACTION", "event": "local.file_move",
        "config": {"path": "{{nodes.t.output.path}}", "baseFolder": "{{nodes.t.output.path}}"}});
    let errors = chain(vec![mv]);
    assert_eq!(
        codes(&errors),
        vec![(RefNotAllowed, Some("m"), Some("config.baseFolder"))]
    );
    assert_eq!(
        errors[0].message,
        "'기준 폴더' 칸에는 앞 단계 값을 넣을 수 없어요."
    );

    let mut b = br("{{nodes.t.output.k}}");
    b["rules"][0]["operator"] = json!("{{nodes.t.output.op}}");
    let errors = chain(vec![cond(json!({"branches": [b]}))]);
    assert_eq!(
        codes(&errors)
            .into_iter()
            .filter(|c| c.0 == RefNotAllowed)
            .collect::<Vec<_>>(),
        vec![
            (RefNotAllowed, Some("c"), Some("config.branches.0.key")),
            (
                RefNotAllowed,
                Some("c"),
                Some("config.branches.0.rules.0.operator")
            ),
        ]
    );
}

/// 리뷰 B P1: 출력 칸(`ref: false` 목록)의 이름·설명에 참조를 넣으면 파일 이름이 구분선 밖 schema로 간다.
#[test]
fn ai_output_fields_refuse_refs() {
    let mut a = ai("a", "x");
    a["config"]["outputFields"] = json!([
        {"key": "folder", "type": "text", "description": "{{nodes.t.output.name}}"},
        {"key": "{{nodes.t.output.extension}}", "type": "text"},
    ]);
    assert_eq!(
        codes(&chain(vec![a]))
            .into_iter()
            .filter(|c| c.0 == RefNotAllowed)
            .collect::<Vec<_>>(),
        vec![
            (
                RefNotAllowed,
                Some("a"),
                Some("config.outputFields.0.description")
            ),
            (RefNotAllowed, Some("a"), Some("config.outputFields.1.key")),
        ]
    );
}

#[test]
fn ref_format() {
    for bad in [
        "{{nodes.a.output}}",
        "{{nodes.a_b.output.x}}",
        "{{nodes.a.output.my-field}}",
        "{{ nodes.a.output.output }}",
        "{{trigger.x}}",
    ] {
        let errors = chain(vec![ai("a", "x"), ai("b", &format!("정리: {bad}"))]);
        assert_eq!(
            codes(&errors),
            vec![(RefFormat, Some("b"), Some("config.data"))],
            "{bad}"
        );
    }
    let errors = chain(vec![ai("a", "x"), ai("b", "{{trigger.x}}")]);
    assert_eq!(
        errors[0].message,
        "참조식 '{{trigger.x}}'의 형식이 맞지 않아요. '{{nodes.<노드ID>.output.<칸>}}' 모양이어야 해요."
    );
    let errors = chain(vec![ai("a", "x"), ai("b", "정리: {{nodes.a.output.output")]);
    assert_eq!(
        codes(&errors),
        vec![(RefFormat, Some("b"), Some("config.data"))]
    );
    assert_eq!(errors[0].message, "참조식의 '{{'가 닫히지 않았어요.");
}

#[test]
fn ref_not_upstream_self_unknown_node_in_order() {
    let errors = chain(vec![
        ai(
            "a",
            "{{nodes.b.output.output}} {{nodes.zz.output.x}} {{nodes.a.output.output}}",
        ),
        ai("b", "x"),
    ]);
    assert_eq!(
        codes(&errors),
        vec![
            (RefNotUpstream, Some("a"), Some("config.data")),
            (RefUnknownNode, Some("a"), Some("config.data")),
            (RefSelf, Some("a"), Some("config.data")),
        ]
    );
    assert_eq!(errors[0].message, "앞 단계가 아닌 노드 'b'를 참조해요.");
    assert_eq!(errors[1].message, "없는 노드 'zz'를 참조해요.");
    assert_eq!(errors[2].message, "자기 자신의 결과는 참조할 수 없어요.");
}

#[test]
fn unknown_event_skips_field_check_but_gets_ref_check() {
    let x = json!({"id": "x", "type": "ACTION", "event": "jira.create_issue",
        "config": {"summary": "{{nodes.zz.output.x}}"}});
    assert_eq!(
        codes(&chain(vec![x])),
        vec![(RefUnknownNode, Some("x"), None)]
    );
}

#[test]
fn ref_unknown_field() {
    let errors = chain(vec![ai("a", "x"), ai("b", "{{nodes.a.output.summary}}")]);
    assert_eq!(
        codes(&errors),
        vec![(RefUnknownField, Some("b"), Some("config.data"))]
    );
    assert_eq!(errors[0].message, "노드 'a'에는 'summary' 결과가 없어요.");

    // outputFields 목록이 정적 출력(output)을 대체한다
    let mut a = ai("a", "x");
    a["config"]["outputFields"] = json!([{"key": "title", "type": "text"}]);
    let errors = chain(vec![
        a.clone(),
        ai("b", "{{nodes.a.output.title}} {{nodes.a.output.output}}"),
    ]);
    assert_eq!(
        codes(&errors),
        vec![(RefUnknownField, Some("b"), Some("config.data"))]
    );
    // 빈 outputFields는 기본 output 유지, 중첩 경로는 첫 키만 본다
    a["config"]["outputFields"] = json!([]);
    assert_eq!(
        chain(vec![a, ai("b", "{{nodes.a.output.output.x.0}}")]),
        vec![]
    );
}

#[test]
fn branch_reserved_key_is_not_referenceable() {
    let c = cond(json!({"branches": [br("b_00000001")]}));
    let x = json!({"id": "x", "type": "ACTION", "event": "local.file_rename", "config": {"path": "p",
        "newName": "{{nodes.c.output._branchKeys}} {{nodes.c.output.branch}} {{nodes.c.output.matched}}"}});
    let errors = run(
        json!([trigger(), c, x]),
        json!([{"source": "t", "target": "c"}, {"source": "c", "target": "x", "conditionType": "b_00000001"}]),
    );
    assert_eq!(
        codes(&errors),
        vec![(RefUnknownField, Some("x"), Some("config.newName"))]
    );
}

#[test]
fn unused_condition_fields_are_skipped() {
    let gone = "{{nodes.gone.output.x}}";
    for extra in [json!({"right": {"a": [gone]}}), json!({"rightValue": gone})] {
        let mut config = json!({"operator": "isEmpty", "left": "{{nodes.t.output.path}}"});
        config
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(chain(vec![cond(config)]), vec![]);
    }
    // 새 모양 분기는 레거시 칸(operator)을 보지 않는다, 갈래 안 참조의 field는 config.branches
    let mut b = br("b_1");
    b["rules"][0]["left"] = json!("{{nodes.zz.output.a}}");
    let errors = chain(vec![cond(
        json!({"branches": [b], "operator": "{{nodes.zz.output.b}}"}),
    )]);
    assert_eq!(
        codes(&errors),
        vec![(RefUnknownNode, Some("c"), Some("config.branches"))]
    );
}

#[test]
fn dedup_keeps_first_and_caps_200() {
    let errors = chain(vec![ai(
        "a",
        "{{nodes.gone.output.x}} {{nodes.zz.output.x}} {{nodes.gone.output.x}}",
    )]);
    let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(
        messages,
        ["없는 노드 'gone'를 참조해요.", "없는 노드 'zz'를 참조해요."]
    );

    let prompt: Vec<String> = (0..300)
        .map(|i| format!("{{{{nodes.gone{i}.output.x}}}}"))
        .collect();
    let errors = chain(vec![ai("a", &prompt.join(" "))]);
    assert_eq!(errors.len(), 200);
    assert_eq!(errors[0].message, "없는 노드 'gone0'를 참조해요.");
}

#[test]
fn user_values_clipped_to_80_chars() {
    let long_id = "g".repeat(200);
    let errors = chain(vec![ai(
        "a",
        &format!(
            "{{{{{}}}}} {{{{nodes.{long_id}.output.x}}}}",
            "x".repeat(200)
        ),
    )]);
    assert!(
        errors[0]
            .message
            .contains(&format!("{{{{{}…}}}}", "x".repeat(80)))
    );
    assert!(
        errors[1]
            .message
            .contains(&format!("'{}…'", "g".repeat(80)))
    );
    let id = "q".repeat(200);
    let errors = run(json!([trigger(), ai(&id, "x")]), json!([]));
    assert_eq!(
        errors[0].node_id.as_deref(),
        Some(format!("{}…", "q".repeat(80)).as_str())
    );
}
