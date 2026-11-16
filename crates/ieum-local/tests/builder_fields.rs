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

#[tokio::test]
async fn set_branches_replaces_reserved_and_duplicate_keys() {
    let cond = json!({"id": "c", "type": "CONDITION", "event": "branch.if", "config": {}});
    let b = Builder::new(MemHost::with(
        W,
        json!({"nodes": [trig(), cond], "edges": [{"source": "t", "target": "c"}]}),
    ))
    .unwrap();
    let rule =
        json!([{"left": "{{nodes.t.output.extension}}", "operator": "equals", "right": "pdf"}]);
    let r = run(
        &b,
        set(
            "c",
            json!({"branches": [{"key": "else", "rules": rule}, {"key": "b_1234abcd", "rules": rule}, {"key": "b_1234abcd", "rules": rule}]}),
        ),
    )
    .await;
    let keys: Vec<&str> = r.data["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys[1], "b_1234abcd");
    assert!(
        keys[0].starts_with("b_") && keys[2].starts_with("b_"),
        "{keys:?}"
    );
    assert!(keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2]);
}

/// t → 자르기(tr)·대응표(lk)·HTTP(h)·AI(a)
fn transforms() -> Value {
    json!({
        "nodes": [
            trig(),
            {"id": "tr", "type": "TRANSFORM", "event": "formatter.text_truncate", "config": {"input": "x"}},
            {"id": "lk", "type": "TRANSFORM", "event": "formatter.lookup_table", "config": {"input": "x"}},
            {"id": "h", "type": "HTTP", "event": "http.request", "config": {"method": "POST", "url": "https://example.com"}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "outputFields": [{"key": "k", "type": "text"}]}}
        ],
        "edges": [
            {"source": "t", "target": "tr"}, {"source": "t", "target": "lk"},
            {"source": "t", "target": "h"}, {"source": "t", "target": "a"}
        ]
    })
}

#[tokio::test]
async fn set_coerces_integer_dict_and_http_body() {
    let h = MemHost::with(W, transforms());
    let b = Builder::new(h.clone()).unwrap();
    let cfg = |id: &str, key: &str| {
        let doc = h.doc(W);
        let node = doc.workflow.nodes.iter().find(|n| n.id == id).unwrap();
        node.config.get(key).cloned().unwrap_or(Value::Null)
    };
    let ok = |values: Value, id: &'static str| {
        let b = &b;
        async move { assert!(run(b, set(id, values)).await.ok) }
    };

    ok(json!({"maxLength": "5"}), "tr").await;
    assert_eq!(cfg("tr", "maxLength"), json!(5));
    ok(json!({"maxLength": 7}), "tr").await; // --json 경로는 그대로
    assert_eq!(cfg("tr", "maxLength"), json!(7));
    ok(json!({"maxLength": "{{nodes.t.output.size}}"}), "tr").await; // 참조식은 글 그대로
    assert_eq!(cfg("tr", "maxLength"), json!("{{nodes.t.output.size}}"));

    ok(json!({"table": "{\"a\":\"b\"}"}), "lk").await;
    assert_eq!(cfg("lk", "table"), json!({"a": "b"}));

    ok(json!({"body": "{\"a\":1}"}), "h").await;
    assert_eq!(cfg("h", "body"), json!({"a": 1}));
    ok(json!({"body": "[1,2]"}), "h").await;
    assert_eq!(cfg("h", "body"), json!([1, 2]));
    for s in ["hi", "123", "\"q\""] {
        ok(json!({"body": s}), "h").await;
        assert_eq!(cfg("h", "body"), json!(s), "{s}");
    }
    ok(json!({"headers": "{\"X-A\":\"1\"}"}), "h").await;
    assert_eq!(cfg("h", "headers"), json!({"X-A": "1"}));

    // 다른 text 칸은 JSON처럼 보여도 글 그대로
    ok(json!({"prompt": "{\"a\":1}"}), "a").await;
    assert_eq!(cfg("a", "prompt"), json!("{\"a\":1}"));

    for (id, values) in [
        ("tr", json!({"maxLength": "5.5"})),
        ("tr", json!({"maxLength": "다섯"})),
        ("lk", json!({"table": "[1]"})),
        ("lk", json!({"table": "a=b"})),
        ("h", json!({"headers": "oops"})),
    ] {
        assert_eq!(
            run(&b, set(id, values.clone())).await.code,
            Some(Code::FieldInvalid),
            "{values}"
        );
    }
}

#[tokio::test]
async fn webhook_credential_field_is_hidden_and_rejected() {
    let h = MemHost::with(W, transforms());
    let b = Builder::new(h.clone()).unwrap();
    let r = run(&b, json!({"op": "fields", "args": {"id": "h"}})).await;
    let keys: Vec<&str> = r.data["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["method", "url", "headers", "body"]);
    for key in ["webhookCredentialId", "config.webhookCredentialId"] {
        assert_eq!(
            run(&b, set("h", json!({key: "w1", "url": "https://a.com"})))
                .await
                .code,
            Some(Code::FieldUnknown),
            "{key}"
        );
    }
    assert_eq!(h.saves(), 0);
}

/// 사람이 편집기에서 넣은 머리말 값·주소 쿼리·본문은 AI에게 가지 않는다.
#[tokio::test]
async fn http_fields_hide_secrets() {
    let h = MemHost::with(
        W,
        json!({
            "nodes": [
                trig(),
                {"id": "h", "type": "HTTP", "event": "http.request", "config": {
                    "method": "POST",
                    "url": "https://u:SECRET123@hooks.example.com/{{nodes.t.output.name}}?token=SECRET123#SECRET123",
                    "headers": {"Authorization": "Bearer SECRET123", "X-Ref": "{{nodes.t.output.name}}"},
                    "body": {"k": "SECRET123"}
                }}
            ],
            "edges": [{"source": "t", "target": "h"}]
        }),
    );
    let b = Builder::new(h.clone()).unwrap();
    let r = run(&b, json!({"op": "fields", "args": {"id": "h"}})).await;
    assert!(r.ok, "{r:?}");
    let text = serde_json::to_string(&r.data).unwrap();
    assert!(!text.contains("SECRET123"), "{text}");
    let value = |key: &str| {
        r.data["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .unwrap()["value"]
            .clone()
    };
    assert_eq!(value("method"), json!("POST"));
    assert_eq!(
        value("url"),
        json!("https://(숨김)@hooks.example.com/(숨김)")
    );
    assert_eq!(
        value("headers"),
        json!({"Authorization": "(숨김)", "X-Ref": "(숨김)"})
    );
    assert_eq!(value("body"), json!("(숨김: object)"));

    // 본문 모양과 없음.
    for (body, want) in [
        (json!([1]), json!("(숨김: array)")),
        (json!("SECRET123"), json!("(숨김: string)")),
    ] {
        h.put_doc(W, |d| {
            d.workflow.nodes[1]
                .config
                .insert("body".into(), body.clone());
        });
        let r = run(&b, json!({"op": "fields", "args": {"id": "h"}})).await;
        let f = &r.data["fields"].as_array().unwrap()[3];
        assert_eq!((&f["key"], &f["value"]), (&json!("body"), &want));
    }
    h.put_doc(W, |d| {
        d.workflow.nodes[1].config.remove("body");
    });
    let r = run(&b, json!({"op": "fields", "args": {"id": "h"}})).await;
    assert_eq!(r.data["fields"][3]["value"], Value::Null);
}

fn http_node(url: &str) -> Value {
    json!({
        "nodes": [
            trig(),
            {"id": "h", "type": "HTTP", "event": "http.request", "config": {
                "method": "POST",
                "url": url,
                "headers": {"Authorization": "Bearer HSECRET"},
                "body": {"k": "BSECRET"}
            }}
        ],
        "edges": [{"source": "t", "target": "h"}]
    })
}

async fn shown_url(b: &Builder) -> Value {
    let r = run(b, json!({"op": "fields", "args": {"id": "h"}})).await;
    r.data["fields"][1]["value"].clone()
}

/// 웹훅처럼 경로 자체가 비밀인 주소 — 호스트(와 참조식 호스트)까지만 보인다.
#[tokio::test]
async fn http_fields_hide_url_path() {
    for (url, want) in [
        (
            "https://hooks.slack.com/services/T0AAAA/B0BBBB/PATHSECRET123",
            "https://hooks.slack.com/(숨김)",
        ),
        (
            "https://discord.com/api/webhooks/123/PATHSECRET123",
            "https://discord.com/(숨김)",
        ),
        (
            "https://api.telegram.org/botPATHSECRET123/sendMessage",
            "https://api.telegram.org/(숨김)",
        ),
        (
            "https:/user:PWSECRET@host.example/x",
            "https://(숨김)@host.example/(숨김)",
        ),
        ("https://user:PW/SECRET@host.example/x", "(숨김)"),
        ("http://host.example:8080", "http://host.example:8080"),
        (
            "{{nodes.t.output.name}}/PATHSECRET123",
            "{{nodes.t.output.name}}/(숨김)",
        ),
        ("not a url PATHSECRET123", "(숨김)"),
    ] {
        let b = Builder::new(MemHost::with(W, http_node(url))).unwrap();
        assert_eq!(shown_url(&b).await, json!(want), "{url}");
    }
}

/// AI가 fields에서 본 숨김 표시를 그대로 돌려줘도 사람이 넣은 값은 그대로다.
#[tokio::test]
async fn http_set_keeps_values_behind_hidden_marks() {
    let url = "https://api.example.com/x?token=QSECRET";
    let h = MemHost::with(W, http_node(url));
    let b = Builder::new(h.clone()).unwrap();
    let r = run(&b, json!({"op": "fields", "args": {"id": "h"}})).await;
    let shown = |i: usize| r.data["fields"][i]["value"].clone();
    let mut headers = shown(2);
    headers["Content-Type"] = json!("application/json");
    let r = run(
        &b,
        set(
            "h",
            json!({"headers": headers, "url": shown(1), "body": shown(3)}),
        ),
    )
    .await;
    assert!(r.ok, "{r:?}");
    let cfg = h.doc(W).workflow.nodes[1].config.clone();
    assert_eq!(
        cfg["headers"],
        json!({"Authorization": "Bearer HSECRET", "Content-Type": "application/json"})
    );
    assert_eq!(cfg["url"], json!(url));
    assert_eq!(cfg["body"], json!({"k": "BSECRET"}));

    // 새 값은 그대로 들어가고, 빠진 머리말은 지워진다.
    let r = run(
        &b,
        set(
            "h",
            json!({"headers": {"X-A": "1"}, "url": "https://b.example/y"}),
        ),
    )
    .await;
    assert!(r.ok, "{r:?}");
    let cfg = h.doc(W).workflow.nodes[1].config.clone();
    assert_eq!(cfg["headers"], json!({"X-A": "1"}));
    assert_eq!(cfg["url"], json!("https://b.example/y"));

    // 숨김 표시인데 지킬 기존 값이 없으면 거절.
    let saves = h.saves();
    h.put_doc(W, |d| {
        d.workflow.nodes[1].config.remove("url");
        d.workflow.nodes[1].config.remove("body");
    });
    for values in [
        json!({"headers": {"Authorization": "(숨김)"}}),
        json!({"url": "https://b.example/(숨김)"}),
        json!({"body": "(숨김: object)"}),
    ] {
        assert_eq!(
            run(&b, set("h", values.clone())).await.code,
            Some(Code::FieldInvalid),
            "{values}"
        );
    }
    assert_eq!(h.saves(), saves);
}

/// 숨김 표시를 섞은 새 값은 거절 — 조용히 옛 값을 지키거나 표시 글자를 저장하지 않는다.
#[tokio::test]
async fn http_set_rejects_values_mixing_hidden_marks() {
    let url = "http://api.example.com/v1/items?token=QSECRET";
    let h = MemHost::with(W, http_node(url));
    let b = Builder::new(h.clone()).unwrap();
    for values in [
        json!({"url": "https://staging.example.com/(숨김)"}),
        json!({"url": "http://api.example.com/(숨김)?page=2"}),
        json!({"url": "https://api.example.com/(숨김)"}),
        json!({"headers": {"Authorization": "Bearer (숨김)"}}),
        json!({"body": {"k": "(숨김)"}}),
        json!({"body": "{\"k\": [\"(숨김)\"]}"}),
        json!({"body": "앞 (숨김: object)"}),
    ] {
        assert_eq!(
            run(&b, set("h", values.clone())).await.code,
            Some(Code::FieldInvalid),
            "{values}"
        );
    }
    assert_eq!(h.saves(), 0);
    let r = run(
        &b,
        set("h", json!({"url": "http://api.example.com/(숨김)"})),
    )
    .await;
    assert!(r.ok, "{r:?}");
    assert_eq!(h.doc(W).workflow.nodes[1].config["url"], json!(url));
}

/// BE는 머리말을 `Map<String,String>`으로 읽는다 — 글이 아닌 값은 set에서 거절.
#[tokio::test]
async fn set_rejects_non_string_header_values() {
    let h = MemHost::with(W, transforms());
    let b = Builder::new(h.clone()).unwrap();
    for headers in [
        json!({"X-A": 1}),
        json!({"X-A": null}),
        json!({"X-A": {"b": "c"}}),
        json!({"X-A": ["x"]}),
        json!("{\"X-A\": 1}"),
    ] {
        assert_eq!(
            run(&b, set("h", json!({"headers": headers.clone()})))
                .await
                .code,
            Some(Code::FieldInvalid),
            "{headers}"
        );
    }
    assert_eq!(h.saves(), 0);
    let r = run(
        &b,
        set("h", json!({"headers": {"X-A": "{{nodes.t.output.name}}"}})),
    )
    .await;
    assert!(r.ok, "{r:?}");
}
