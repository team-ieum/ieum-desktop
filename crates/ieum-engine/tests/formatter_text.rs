//! BE `TextFormatsTest`의 글 포맷터 8개 케이스.

use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::{Map, Value, json};

fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

fn cfg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn case_v(event: &str, input: Value, want: Value) {
    let got = apply(event, &cfg(input.clone()), now())
        .unwrap_or_else(|e| panic!("{event} {input}: {}", e.message));
    assert_eq!(got["output"], want, "{event} {input}");
}

fn case(event: &str, input: Value, want: &str) {
    case_v(event, input, json!(want));
}

/// 실패 이유(메시지 안)와 코드.
fn fails(event: &str, input: Value, reason: &str, code: &str) {
    let e = apply(event, &cfg(input.clone()), now()).unwrap_err();
    assert!(e.message.contains(reason), "{event} {input}: {}", e.message);
    assert_eq!(e.code.as_deref(), Some(code), "{event} {input}");
}

#[test]
fn text_case() {
    let ev = "formatter.text_case";
    case(
        ev,
        json!({"input":"hello wORLD","mode":"title"}),
        "Hello World",
    );
    case(
        ev,
        json!({"input":"hello wORLD","mode":"capitalize"}),
        "Hello wORLD",
    );
    case(ev, json!({"input":"hello wORLD"}), "Hello World");
    case(ev, json!({"input":"한글 abc","mode":"upper"}), "한글 ABC");
    case(ev, json!({"input":"ABC","mode":"lower"}), "abc");
    case(ev, json!({"input":""}), "");
}

#[test]
fn text_trim() {
    let ev = "formatter.text_trim";
    case(ev, json!({"input":"  a  b \n"}), "a  b");
    case(ev, json!({"input":"\u{a0}a\u{1f} "}), "\u{a0}a");
    case(ev, json!({"input":""}), "");
}

#[test]
fn text_replace() {
    let ev = "formatter.text_replace";
    case(
        ev,
        json!({"input":"a b","find":"[:space:]","replace":"_"}),
        "a_b",
    );
    case(ev, json!({"input":"a b","find":"[:space:]"}), "ab");
    case(ev, json!({"input":"a.b","find":".","replace":"-"}), "a-b");
    case(
        ev,
        json!({"input":"a\nb\tc","find":"[:newline:]","replace":"[:tab:]"}),
        "a\tb\tc",
    );
    case(
        ev,
        json!({"input":"a b","find":"[:space:]","replace":"[:newline:]"}),
        "a\nb",
    );
    case(ev, json!({"input":"","find":"a"}), "");
    fails(
        ev,
        json!({"input":"a","find":""}),
        "찾아 바꾸기: '찾을 글자' 칸이 비어 있어요",
        "FIELD_EMPTY find",
    );
    fails(
        ev,
        json!({"input":"a".repeat(50_000),"find":"a","replace":"b".repeat(50_000)}),
        "바꾼 결과가 10만 자를 넘어요",
        "TOO_LARGE",
    );
}

#[test]
fn text_truncate() {
    let ev = "formatter.text_truncate";
    case(ev, json!({"input":"👨‍👩‍👧가나다","maxLength":2}), "👨‍👩‍👧가");
    case(
        ev,
        json!({"input":"가나다라","maxLength":3,"ellipsis":true}),
        "가나…",
    );
    case(
        ev,
        json!({"input":"가나","maxLength":3,"ellipsis":true}),
        "가나",
    );
    case(
        ev,
        json!({"input":"👨‍👩‍👧가나","maxLength":2,"ellipsis":true}),
        "👨‍👩‍👧…",
    );
    case(ev, json!({"input":"","maxLength":3}), "");
    for bad in [json!(0), json!("abc")] {
        fails(
            ev,
            json!({"input":"abc","maxLength":bad}),
            "'최대 글자 수' 칸은 1 이상의 정수여야 해요",
            "FIELD_INVALID maxLength",
        );
    }
}

#[test]
fn text_split() {
    let ev = "formatter.text_split";
    case(ev, json!({"input":"a, b ,c","part":"first"}), "a");
    case(ev, json!({"input":"a, b ,c","part":"second"}), "b");
    case(ev, json!({"input":"a, b ,c","part":"last"}), "c");
    case_v(
        ev,
        json!({"input":"a, b ,c","part":"all"}),
        json!(["a", "b", "c"]),
    );
    case(ev, json!({"input":"a","part":"second"}), "");
    case(ev, json!({"input":"a,b"}), "a");
    case(
        ev,
        json!({"input":"a|b","separator":"|","part":"last"}),
        "b",
    );
    case(ev, json!({"input":""}), "");
    case_v(ev, json!({"input":"","part":"all"}), json!([]));
    case_v(
        ev,
        json!({"input":"a,,b,","part":"all"}),
        json!(["a", "", "b", ""]),
    );
    fails(
        ev,
        json!({"input":",".repeat(10_000),"part":"all"}),
        "결과 목록이 1만 개를 넘어요",
        "TOO_LARGE",
    );
}

#[test]
fn text_default() {
    let ev = "formatter.text_default";
    case(ev, json!({"input":"","defaultValue":"없음"}), "없음");
    case(ev, json!({"input":"[]","defaultValue":"없음"}), "없음");
    case(ev, json!({"input":"x","defaultValue":"없음"}), "x");
    case(ev, json!({"input":" [] ","defaultValue":"x"}), "x");
    case(ev, json!({"input":"","defaultValue":"d"}), "d");
    fails(
        ev,
        json!({"input":"x","defaultValue":""}),
        "'기본값' 칸이 비어 있어요",
        "FIELD_EMPTY defaultValue",
    );
    fails(
        ev,
        json!({"input":"a"}),
        "'기본값' 칸이 비어 있어요",
        "FIELD_EMPTY defaultValue",
    );
}

#[test]
fn text_url_encode() {
    let ev = "formatter.text_url_encode";
    case(ev, json!({"input":"a b&가"}), "a%20b%26%EA%B0%80");
    case(
        ev,
        json!({"input":"a%20b%26%EA%B0%80","direction":"decode"}),
        "a b&가",
    );
    case(ev, json!({"input":"a b~*"}), "a%20b%7E*");
    case(ev, json!({"input":"a+b%2B","direction":"decode"}), "a b+");
    case(ev, json!({"input":""}), "");
    for bad in ["%E", "%zz", "a%"] {
        fails(
            ev,
            json!({"input":bad,"direction":"decode"}),
            "URL 인코딩을 풀지 못했어요",
            "FIELD_INVALID input",
        );
    }
}

#[test]
fn text_length() {
    let ev = "formatter.text_length";
    case_v(ev, json!({"input":"👍한글"}), json!(3));
    case_v(ev, json!({"input":" a  b\nc ","unit":"words"}), json!(3));
    case_v(ev, json!({"input":""}), json!(0));
    case_v(ev, json!({"input":"","unit":"words"}), json!(0));
    case_v(
        ev,
        json!({"input":"가 나\u{3000}다","unit":"words"}),
        json!(2),
    );
}
