//! Builder fields·set — 칸 정의·잠금·값 맞추기·갈래 key.

mod builder_support;

use builder_support::MemHost;
use ieum_local::builder::*;
use serde_json::{Value, json};

const W: &str = "11111111-1111-4111-8111-111111111111";

async fn run(b: &Builder, op: Value) -> Reply {
    b.apply(W, Op::from_value(op).unwrap(), None).await
}

fn trig() -> Value {
    json!({"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/h/in"}})
}

/// t → a(출력 칸 folder) → m(path만 채움)
fn chain() -> Value {
    json!({
        "nodes": [
            trig(),
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p", "outputFields": [{"key": "folder", "type": "text"}]}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}"}}
        ],
        "edges": [{"source": "t", "target": "a"}, {"source": "a", "target": "m"}]
    })
}

fn set(id: &str, values: Value) -> Value {
    json!({"op": "set", "args": {"id": id, "values": values}})
}

#[tokio::test]
async fn fields_show_defs_values_locks_and_upstream_outputs() {
    let b = Builder::new(MemHost::with(W, chain())).unwrap();
    let r = run(&b, json!({"op": "fields", "args": {"id": "m"}})).await;
    let f = r.data["fields"].as_array().unwrap();
    assert_eq!(
        f.iter()
            .map(|x| x["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["path", "baseFolder", "destination"]
    );
    assert_eq!(f[0]["value"], "{{nodes.t.output.path}}");
    assert_eq!(
        (
            &f[1]["locked"],
            &f[1]["required"],
            &f[1]["ref"],
            &f[1]["value"]
        ),
        (&json!(true), &json!(true), &json!(false), &json!(null))
    );
    assert_eq!(
        r.data["upstream"],
        json!([
            {"id": "t", "label": null, "event": "local.folder_new_file", "outputs": ["extension", "name", "path", "size"]},
            {"id": "a", "label": null, "event": "local.ai", "outputs": ["folder"]}
        ])
    );
    let a = run(&b, json!({"op": "fields", "args": {"id": "a"}})).await;
    assert_eq!(a.data["fields"][0]["locked"], true); // provider
    assert!(
        a.data["fields"][0]["choices"]
            .as_array()
            .unwrap()
            .contains(&json!("cli:claude"))
    );
    assert_eq!(
        run(&b, json!({"op": "fields", "args": {"id": "zz"}}))
            .await
            .code,
        Some(Code::NodeNotFound)
    );
}

#[tokio::test]
async fn set_saves_and_returns_that_nodes_errors() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    let r = run(
        &b,
        set("m", json!({"destination": "{{nodes.a.output.nope}}"})),
    )
    .await;
    assert!(r.ok);
    let codes: Vec<_> = r.data["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["code"].as_str().unwrap(), e["field"].as_str().unwrap()))
        .collect();
    assert_eq!(
        codes,
        [
            ("REF_UNKNOWN_FIELD", "config.destination"),
            ("REQUIRED", "config.baseFolder")
        ]
    );
    assert_eq!(
        h.doc(W).workflow.nodes[2].config["destination"],
        "{{nodes.a.output.nope}}"
    );
    assert!(!h.doc(W).enabled);
}

#[tokio::test]
async fn set_rejects_all_when_one_locked() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    assert_eq!(
        run(
            &b,
            set("m", json!({"destination": "x", "baseFolder": "/h/out"}))
        )
        .await
        .code,
        Some(Code::FieldLocked)
    );
    assert_eq!(
        run(&b, set("t", json!({"folder": "/h/other"}))).await.code,
        Some(Code::FieldLocked)
    );
    // 키 앞에 `config.`을 붙여도 잠금은 그대로다(접두사를 뗀 뒤에 본다).
    for (id, k) in [
        ("t", "config.folder"),
        ("m", "config.baseFolder"),
        ("a", "config.provider"),
        ("a", "config.model"),
        ("a", "config.effort"),
    ] {
        assert_eq!(
            run(&b, set(id, json!({k: "/h/x"}))).await.code,
            Some(Code::FieldLocked),
            "{k}"
        );
    }
    for k in ["provider", "credential", "model", "effort"] {
        assert_eq!(
            run(&b, set("a", json!({"prompt": "새 지시", k: "x"})))
                .await
                .code,
            Some(Code::FieldLocked),
            "{k}"
        );
    }
    assert_eq!(
        run(&b, set("m", json!({"destination": "x", "nope": 1})))
            .await
            .code,
        Some(Code::FieldUnknown)
    );
    assert_eq!(h.saves(), 0); // 부분 저장 없음
    assert!(
        !h.doc(W).workflow.nodes[2]
            .config
            .contains_key("destination")
    );
    assert_eq!(h.doc(W).workflow.nodes[1].config["prompt"], "p");
}

#[tokio::test]
async fn set_coerces_cli_strings_and_guards_destination() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    assert!(
        run(
            &b,
            set(
                "a",
                json!({"sendContent": "true", "outputFields": "[{\"key\":\"kind\",\"type\":\"text\"}]"})
            )
        )
        .await
        .ok
    );
    let c = &h.doc(W).workflow.nodes[1].config;
    assert_eq!(
        (&c["sendContent"], &c["outputFields"][0]["key"]),
        (&json!(true), &json!("kind"))
    );
    assert_eq!(
        run(&b, set("a", json!({"outputFields": "oops"})))
            .await
            .code,
        Some(Code::FieldInvalid)
    );
    for bad in ["/abs/dir", "../up", "a/../../b"] {
        assert_eq!(
            run(&b, set("m", json!({"destination": bad}))).await.code,
            Some(Code::FolderNotAllowed),
            "{bad}"
        );
    }
    assert!(
        run(
            &b,
            set(
                "m",
                json!({"destination": "영수증/{{nodes.a.output.kind}}"})
            )
        )
        .await
        .ok
    );
}

#[tokio::test]
async fn set_branches_fills_missing_keys_and_resets_test_state() {
    let cond = json!({"id": "c", "type": "CONDITION", "event": "branch.if", "config": {}});
    let h = MemHost::with(
        W,
        json!({"nodes": [trig(), cond], "edges": [{"source": "t", "target": "c"}]}),
    );
    let b = Builder::new(h.clone()).unwrap();
    assert_eq!(
        run(&b, json!({"op": "status"})).await.data["nodes"][1]["state"],
        "passed"
    );
    let rule =
        json!([{"left": "{{nodes.t.output.extension}}", "operator": "equals", "right": "pdf"}]);
    let r = run(
        &b,
        set(
            "c",
            json!({"branches": [{"name": "PDF", "rules": rule}, {"key": "b_aaaaaaaa", "name": "그 밖", "rules": rule}]}),
        ),
    )
    .await;
    let keys = r.data["branches"].as_array().unwrap();
    let k0 = keys[0]["key"].as_str().unwrap();
    assert!(
        k0.len() == 10
            && k0.starts_with("b_")
            && k0[2..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{k0}"
    );
    assert_eq!(keys[1], json!({"key": "b_aaaaaaaa", "name": "그 밖"}));
    assert_eq!(r.data["errors"], json!([]));
    // config가 바뀌면 untested
    assert_eq!(
        run(&b, json!({"op": "status"})).await.data["nodes"][1]["state"],
        "untested"
    );
}
