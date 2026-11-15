//! BE `ListFormatsTest`의 목록 포맷터 3개 케이스 + 참조식으로 들어온 JSON 문자열(Review Focus 1).

use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::{Map, Value, json};

fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

fn cfg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn out(event: &str, input: Value) -> Value {
    let got = apply(event, &cfg(input.clone()), now())
        .unwrap_or_else(|e| panic!("{event} {input}: {}", e.message));
    got["output"].clone()
}

fn case(event: &str, input: Value, want: &str) {
    case_v(event, input, json!(want));
}

fn case_v(event: &str, input: Value, want: Value) {
    assert_eq!(out(event, input.clone()), want, "{event} {input}");
}

/// 실패 이유(메시지 안)와 코드.
fn fails(event: &str, input: Value, reason: &str, code: &str) {
    let e = apply(event, &cfg(input.clone()), now()).unwrap_err();
    assert!(e.message.contains(reason), "{event} {input}: {}", e.message);
    assert_eq!(e.code.as_deref(), Some(code), "{event} {input}");
}

const LOOKUP: &str = "formatter.lookup_table";
const PICK: &str = "formatter.list_pick";
const TO_TEXT: &str = "formatter.list_to_text";
const NOT_TABLE: &str = "'대응표' 칸이 대응표(키-값) 모양이 아니에요";
const MAX_TEXT: usize = 100_000;
const MAX_ITEMS: usize = 10_000;

fn table() -> Value {
    json!({"urgent": "긴급", "Low": "낮음"})
}

const ITEMS: &str = r#"[{"id":1,"title":"A"},{"id":2,"title":"B"}]"#;

#[test]
fn lookup_table() {
    case(LOOKUP, json!({"input":"urgent","table":table()}), "긴급");
    case(LOOKUP, json!({"input":" urgent ","table":table()}), "긴급");
    case(
        LOOKUP,
        json!({"input":"URGENT","table":table(),"fallback":"x"}),
        "x",
    );
    case(LOOKUP, json!({"input":"URGENT","table":table()}), "");
    case(
        LOOKUP,
        json!({"input":"","table":table(),"fallback":"x"}),
        "",
    );
    fails(
        LOOKUP,
        json!({"input":"urgent","table":"urgent=긴급"}),
        NOT_TABLE,
        "FIELD_INVALID table",
    );
    fails(
        LOOKUP,
        json!({"input":"urgent"}),
        NOT_TABLE,
        "FIELD_EMPTY table",
    );
    let big: Map<String, Value> = (0..=MAX_ITEMS)
        .map(|i| (format!("k{i}"), json!("v")))
        .collect();
    fails(
        LOOKUP,
        json!({"input":"k1","table":big}),
        "대응표가 1만 개를 넘어요",
        "TOO_LARGE",
    );
}

#[test]
fn lookup_table_non_string_values() {
    let t = json!({"n": 3, "o": {"k": 1}, "z": null});
    case(LOOKUP, json!({"input":"n","table":t}), "3");
    case(LOOKUP, json!({"input":"o","table":t}), r#"{"k":1}"#);
    case(LOOKUP, json!({"input":"z","table":t,"fallback":"x"}), "");
}

#[test]
fn list_pick_keeps_map() {
    case_v(PICK, json!({"input":ITEMS}), json!({"id":1,"title":"A"}));
    case_v(
        PICK,
        json!({"input":ITEMS,"position":"last"}),
        json!({"id":2,"title":"B"}),
    );
    case_v(
        PICK,
        json!({"input":ITEMS,"position":"nth","index":2}),
        json!({"id":2,"title":"B"}),
    );
}

#[test]
fn list_pick() {
    case(PICK, json!({"input":"a,,b","position":"nth","index":2}), "");
    fails(
        PICK,
        json!({"input":"a,,b","position":"nth","index":4}),
        "4번째 항목이 없어요(목록 3개)",
        "FIELD_INVALID index",
    );
    fails(
        PICK,
        json!({"input":"a,,b","position":"nth"}),
        "'몇 번째' 칸은 1 이상의 정수여야 해요",
        "FIELD_INVALID index",
    );
    fails(
        PICK,
        json!({"input":""}),
        "목록이 비어 있어요",
        "FIELD_INVALID input",
    );
    case(PICK, json!({"input":["x","y"],"position":"last"}), "y");
}

#[test]
fn list_to_text() {
    case(
        TO_TEXT,
        json!({"input":r#"["a",{"k":1},3,true]"#}),
        r#"a, {"k":1}, 3, true"#,
    );
    case(TO_TEXT, json!({"input":"a,b","separator":" / "}), "a / b");
    case(TO_TEXT, json!({"input":"a,b"}), "a, b");
    case(TO_TEXT, json!({"input":""}), "");
    case(TO_TEXT, json!({"input":r#"["a",null,"b"]"#}), "a, , b");
    case(TO_TEXT, json!({"input":"a,b","separator":""}), "a, b");
}

#[test]
fn list_pick_search_items_round_trip() {
    let items = r#"[{"id":7,"meta":{"tags":["a","b"],"score":2.5,"note":null}},3]"#;
    let picked = out(PICK, json!({"input":items}));
    // 다음 노드 참조는 Map을 압축 JSON으로 쓴다.
    assert_eq!(
        picked.to_string(),
        r#"{"id":7,"meta":{"tags":["a","b"],"score":2.5,"note":null}}"#
    );
    case_v(PICK, json!({"input":items,"position":"last"}), json!(3));
}

#[test]
fn list_pick_null_and_blank() {
    let got = apply(PICK, &cfg(json!({"input":"[null,1]"})), now()).unwrap();
    assert_eq!(got.get("output"), Some(&Value::Null));
    fails(
        PICK,
        json!({"input":"   "}),
        "목록이 비어 있어요",
        "FIELD_INVALID input",
    );
}

#[test]
fn list_to_text_result_limit() {
    let many = ",".repeat(MAX_ITEMS - 1);
    let over = "결과가 10만 자를 넘어요";
    fails(
        TO_TEXT,
        json!({"input":many,"separator":"x".repeat(MAX_TEXT)}),
        over,
        "TOO_LARGE",
    );
    fails(
        TO_TEXT,
        json!({"input":many,"separator":"xxxxxxxxxxx"}),
        over,
        "TOO_LARGE",
    );
    fails(
        TO_TEXT,
        json!({"input":"a","separator":"x".repeat(MAX_TEXT + 1)}),
        "'separator' 칸이 10만 자를 넘어요",
        "TOO_LARGE",
    );
    fails(
        TO_TEXT,
        json!({"input":["y".repeat(60_000), "z".repeat(60_000)]}),
        over,
        "TOO_LARGE",
    );
    let got = out(TO_TEXT, json!({"input":many,"separator":"x".repeat(10)}));
    assert_eq!(got.as_str().unwrap().len(), 99_990);
}

#[test]
fn decimals_stay_plain() {
    case(
        TO_TEXT,
        json!({"input":"[12345678.9, 0.0001, 100000000000000000000.5]"}),
        "12345678.9, 0.0001, 100000000000000000000.5",
    );
    case(PICK, json!({"input":"[12345678.9]"}), "12345678.9");
    case(PICK, json!({"input":"[0.0001]"}), "0.0001");
    let picked = out(PICK, json!({"input":r#"[{"price":12345678.9}]"#}));
    assert_eq!(picked.to_string(), r#"{"price":12345678.9}"#);
}

/// BE `BigDecimal.toPlainString` — 원문 끝 0을 지키고 지수는 풀고 음수 0은 0.
#[test]
fn decimals_keep_source_digits() {
    case(
        TO_TEXT,
        json!({"input":r#"[1.50, 1e5, -0.0, 2.5E-3, "1.50", [1.50]]"#}),
        "1.50, 100000, 0.0, 0.0025, 1.50, [1.5]",
    );
    case(
        PICK,
        json!({"input":" [ \"a,]\" , 1.50 ] ","position":"last"}),
        "1.50",
    );
    case(
        LOOKUP,
        json!({"input":"b","table":r#"{"a:\"}": 1, "b": 2.50, "b": 3.10}"#}),
        "3.10",
    );
    fails(
        PICK,
        json!({"input":"[0e-999999999]"}),
        "목록 속 숫자가 10만 자를 넘어요",
        "TOO_LARGE",
    );
}

#[test]
fn lookup_table_json_string() {
    case(LOOKUP, json!({"table":r#"{"x":"y"}"#,"input":"x"}), "y");
    fails(
        LOOKUP,
        json!({"table":"[1]","input":"x"}),
        NOT_TABLE,
        "FIELD_INVALID table",
    );
    fails(
        LOOKUP,
        json!({"table":"{broken","input":"x"}),
        NOT_TABLE,
        "FIELD_INVALID table",
    );
}

/// Review Focus 1 — 참조식으로 들어온 목록·대응표는 JSON 문자열이다.
#[test]
fn reference_json_strings() {
    let l = "[1.5, {\"a\":1}, null]";
    case_v(
        PICK,
        json!({"input":l,"position":"nth","index":1}),
        json!("1.5"),
    );
    case_v(
        PICK,
        json!({"input":l,"position":"nth","index":2}),
        json!({"a":1}),
    );
    case(TO_TEXT, json!({"input":l}), "1.5, {\"a\":1}, ");
    case(LOOKUP, json!({"input":" a ","table":"{\"a\":1}"}), "1");
    // 빈 키는 fallback이 아니라 빈 값.
    case(
        LOOKUP,
        json!({"input":"  ","table":{"a":"x"},"fallback":"f"}),
        "",
    );
    fails(
        PICK,
        json!({"input":""}),
        "목록이 비어 있어요",
        "FIELD_INVALID input",
    );
    // 목록으로 들어온 소수는 BE도 숫자 그대로다(JSON 글에서 읽은 것만 글자로).
    case_v(PICK, json!({"input":[1.5]}), json!(1.5));
}

/// 0의 여러 표기 — BE(Java 21, Jackson BigDecimal → toPlainString) 실측 결과.
#[test]
fn zero_notations() {
    case(
        TO_TEXT,
        json!({"input":"[0e5, 0e-5, 0.0, -0.0, 0E+3]"}),
        "0, 0.00000, 0.0, 0.0, 0",
    );
    case(
        LOOKUP,
        json!({"input":"a","table":r#"{"a": 0e-5}"#}),
        "0.00000",
    );
}

/// 지수가 큰 원소는 펴기 전에 길이를 재고, 목록 전체 합계도 잰다.
/// `1e99999`는 f64 범위를 넘어 JSON 목록으로 읽히지 않으므로 같은 크기로 펴지는 음의 지수로 잰다.
#[test]
fn huge_exponents_fail_fast() {
    let entries = |lit: &str, n: usize| {
        let list = format!("[{}]", vec![lit; n].join(","));
        let table = format!(
            "{{{}}}",
            (0..n)
                .map(|k| format!("\"k{k}\": {lit}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        [
            (PICK, json!({"input":list})),
            (TO_TEXT, json!({"input":list.clone()})),
            (LOOKUP, json!({"input":"k0","table":table})),
        ]
    };
    // 원소 하나가 `0.` + 99999자리 = 10만1자.
    for (event, input) in entries("1e-99999", 20) {
        let t = std::time::Instant::now();
        fails(event, input, "목록 속 숫자가 10만 자를 넘어요", "TOO_LARGE");
        assert!(
            t.elapsed().as_secs_f64() < 1.0,
            "{event}: {:?}",
            t.elapsed()
        );
    }
    // 원소 하나는 10만 자 안(99999자)이지만 합계가 100만 자를 넘는다.
    for (event, input) in entries("1e-99997", 11) {
        let t = std::time::Instant::now();
        fails(
            event,
            input,
            "목록 속 숫자가 모두 합해 100만 자를 넘어요",
            "TOO_LARGE",
        );
        assert!(
            t.elapsed().as_secs_f64() < 1.0,
            "{event}: {:?}",
            t.elapsed()
        );
    }
    fails(
        PICK,
        json!({"input":"[-1e-99998]"}),
        "목록 속 숫자가 10만 자를 넘어요",
        "TOO_LARGE",
    );
    let fits = out(PICK, json!({"input":"[1e-99998]"}));
    assert_eq!(fits.as_str().unwrap().len(), MAX_TEXT);
    case(
        TO_TEXT,
        json!({"input":"[1e5, 1.50, 100000000000000000000.5]"}),
        "100000, 1.50, 100000000000000000000.5",
    );
}

#[test]
fn table_string_over_limit() {
    let t = format!("{{\"a\":\"{}\"}}", "x".repeat(MAX_TEXT));
    fails(
        LOOKUP,
        json!({"input":"a","table":t}),
        "'table' 칸이 10만 자를 넘어요",
        "TOO_LARGE",
    );
}
