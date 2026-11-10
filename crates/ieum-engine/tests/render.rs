use ieum_engine::{Outputs, referenced_node_ids, render_deep};
use serde_json::{Value, json};

fn outputs(pairs: &[(&str, Value)]) -> Outputs {
    pairs
        .iter()
        .map(|(id, v)| (id.to_string(), v.as_object().unwrap().clone()))
        .collect()
}

fn render(s: &str, o: &Outputs) -> Value {
    render_deep(&json!(s), o)
}

#[test]
fn renders_nested_and_index() {
    let o = outputs(&[(
        "list",
        json!({
            "issues": [{"title": "첫 이슈"}, {"title": "둘째 이슈"}],
            "empty": [],
            "title": "문자열",
            "data": {"name": "이음"}
        }),
    )]);
    assert_eq!(
        render("{{nodes.list.output.issues.0.title}}", &o),
        json!("첫 이슈")
    );
    assert_eq!(
        render("{{nodes.list.output.issues.1.title}}", &o),
        json!("둘째 이슈")
    );
    assert_eq!(
        render("이름={{nodes.list.output.data.name}}!", &o),
        json!("이름=이음!")
    );
    assert_eq!(
        render("{{nodes.list.output.issues.5.title}}", &o),
        json!("")
    );
    assert_eq!(render("{{nodes.list.output.empty.0}}", &o), json!(""));
    assert_eq!(
        render(
            "{{nodes.list.output.issues.99999999999999999999.title}}",
            &o
        ),
        json!("")
    );
    assert_eq!(
        render("{{nodes.list.output.issues.0.missing}}", &o),
        json!("")
    );
    // 경로 중간에 스칼라를 만나면 그 스칼라
    assert_eq!(render("{{nodes.list.output.title.0}}", &o), json!("문자열"));

    let nested = json!([{"config": {"id": "{{nodes.list.output.data.name}}"}}, 7]);
    assert_eq!(
        render_deep(&nested, &o),
        json!([{"config": {"id": "이음"}}, 7])
    );
}

#[test]
fn missing_is_empty_string() {
    let o = outputs(&[("a", json!({"x": null})), ("blank", json!({}))]);
    assert_eq!(render("[{{nodes.nope.output.x}}]", &o), json!("[]"));
    assert_eq!(render("[{{nodes.a.output.y}}]", &o), json!("[]"));
    assert_eq!(render("[{{nodes.a.output.x}}]", &o), json!("[]"));
    assert_eq!(render("[{{nodes.blank.output.x}}]", &o), json!("[]"));
}

#[test]
fn map_value_becomes_json() {
    let o = outputs(&[(
        "n",
        json!({
            "issue": {"number": 7, "title": "t"},
            "labels": ["a", "b"],
            "count": 7,
            "flag": true,
            "text": "text",
            "m": {"t": "a$1\\b"}
        }),
    )]);
    assert_eq!(
        render("{{nodes.n.output.issue}}", &o),
        json!(r#"{"number":7,"title":"t"}"#)
    );
    assert_eq!(
        render("{{nodes.n.output.labels}}", &o),
        json!(r#"["a","b"]"#)
    );
    // List에 숫자 아닌 세그먼트 → 그 List
    assert_eq!(
        render("{{nodes.n.output.labels.first}}", &o),
        json!(r#"["a","b"]"#)
    );
    // 문자열 전체가 참조식 하나여도 타입 유지 안 함(항상 문자열)
    assert_eq!(render("{{nodes.n.output.count}}", &o), json!("7"));
    assert_eq!(render("{{nodes.n.output.flag}}", &o), json!("true"));
    assert_eq!(render("{{nodes.n.output.text}}", &o), json!("text"));
    assert_eq!(
        render("{{nodes.n.output.m}}", &o),
        json!(r#"{"t":"a$1\\b"}"#)
    );
}

#[test]
fn map_json_keeps_insertion_order() {
    let o = outputs(&[("n", json!({"m": {"z": 1, "a": 2}}))]);
    assert_eq!(
        render("{{nodes.n.output.m}}", &o),
        json!(r#"{"z":1,"a":2}"#)
    );
}

#[test]
fn single_pass_does_not_rerender() {
    let o = outputs(&[
        (
            "issue",
            json!({"title": "제목 {{nodes.other.output.secret}}"}),
        ),
        ("other", json!({"secret": "SECRET-VALUE"})),
    ]);
    assert_eq!(
        render(
            "{{nodes.issue.output.title}} / {{nodes.other.output.secret}}",
            &o
        ),
        json!("제목 {{nodes.other.output.secret}} / SECRET-VALUE")
    );
}

#[test]
fn map_keys_untouched() {
    let o = outputs(&[("a", json!({"k": "v"}))]);
    let input =
        json!({"{{nodes.a.output.k}}": "{{nodes.a.output.k}}", "n": 1, "b": false, "z": null});
    assert_eq!(
        render_deep(&input, &o),
        json!({"{{nodes.a.output.k}}": "v", "n": 1, "b": false, "z": null})
    );
}

#[test]
fn referenced_node_ids_order_and_dedup() {
    let config = json!({
        "first": "{{nodes.a.output.x}} {{nodes.b.output.y}}",
        "list": ["{{nodes.a.output.z}}", {"deep": "{{nodes.c-1.output.w}}"}],
        "{{nodes.keyonly.output.k}}": 3
    });
    assert_eq!(referenced_node_ids(&config), vec!["a", "b", "c-1"]);
    // Map 값 순회도 삽입순(키 사전순이면 a가 먼저)
    let ordered = json!({"z": "{{nodes.zz.output.x}}", "a": "{{nodes.aa.output.x}}"});
    assert_eq!(referenced_node_ids(&ordered), vec!["zz", "aa"]);
    assert!(referenced_node_ids(&Value::Null).is_empty());
    assert!(referenced_node_ids(&json!({"a": "plain", "b": 3})).is_empty());
}

// ── 의도된 차이(conformance/GAPS.md) ─────────────────────

#[test]
fn gap_double_notation_differs_from_java() {
    // BE `String.valueOf(Double)`은 1.0E7·1.0E-4, 러스트는 소수 그대로.
    let o = outputs(&[("a", json!({"big": 1.0e7, "small": 1.0e-4}))]);
    assert_eq!(render("{{nodes.a.output.big}}", &o), json!("10000000.0"));
    assert_eq!(render("{{nodes.a.output.small}}", &o), json!("0.0001"));
}

#[test]
fn gap_integer_beyond_u64_becomes_f64() {
    // BE(Jackson)는 BigInteger로 읽어 자릿수 그대로, 러스트는 f64로 읽는다.
    let big: Value = serde_json::from_str(r#"{"n": 18446744073709551616}"#).unwrap();
    let o = outputs(&[("a", big)]);
    assert_eq!(
        render("{{nodes.a.output.n}}", &o),
        json!("1.8446744073709552e+19")
    );
}

#[test]
fn gap_json_control_escape_is_lowercase() {
    // Jackson은 `\u001F`(대문자), serde_json은 `\u001f`.
    let o = outputs(&[("a", json!({"m": {"k": "\u{1f}"}}))]);
    assert_eq!(
        render("{{nodes.a.output.m}}", &o),
        json!("{\"k\":\"\\u001f\"}")
    );
}
