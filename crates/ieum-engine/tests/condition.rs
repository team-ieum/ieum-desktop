use std::sync::Arc;

use chrono::{DateTime, Utc};
use ieum_engine::{
    BuiltinRunners, ConditionRunner, Evaluation, Node, NodeCtx, NodeError, NodeRunner, Runners,
    evaluate,
};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

/// BE 테스트 시계와 같다 — KST 2026-11-05 10:00.
fn now() -> DateTime<Utc> {
    "2026-11-05T01:00:00Z".parse().unwrap()
}

fn eval(config: Value) -> Result<Evaluation, NodeError> {
    evaluate(config.as_object().unwrap(), now())
}

fn output(config: Value) -> Map<String, Value> {
    eval(config).unwrap().output
}

fn rule(left: Value, op: &str, right: Value) -> Value {
    json!({"left": left, "operator": op, "right": right})
}

fn one(rules: Vec<Value>) -> Value {
    json!({"branches": [{"key": "b_7f3a9c21", "name": "긴급", "rules": rules}]})
}

fn check(op: &str, left: Value, right: Value, cs: bool) -> bool {
    let mut c = one(vec![rule(left, op, right)]);
    c["caseSensitive"] = json!(cs);
    output(c)["matched"] == json!(true)
}

fn two_true(pick: Value) -> Value {
    json!({"branches": [
        {"key": "b_aaaaaaa1", "name": "A", "rules": [rule(json!("x"), "isNotEmpty", Value::Null)]},
        {"key": "b_bbbbbbb2", "name": "B", "rules": [rule(json!("y"), "isNotEmpty", Value::Null)]}
    ], "pick": pick})
}

fn no_match(otherwise: Value) -> Value {
    json!({"branches": [{"key": "b_aaaaaaa1", "name": "A",
        "rules": [rule(json!(""), "isNotEmpty", Value::Null)]}], "otherwise": otherwise})
}

fn legacy(op: &str, left: Value, right: Value) -> Value {
    json!({"operator": op, "left": left, "right": right})
}

/// (operator, left, right, caseSensitive) → passed. 기대값은 행마다 Java `ConditionNodeExecutor`·`ConditionValues`를 읽어 정했다.
#[test]
fn operator_table() {
    let n = Value::Null;
    let long1 = "1".repeat(1000);
    let long2 = "1".repeat(1001);
    #[rustfmt::skip]
    let rows: Vec<(&str, Value, Value, bool, bool)> = vec![
        // BE ConditionNodeExecutorTest.operators (21행)
        ("contains", json!("[긴급] 서버 다운"), json!("긴급"), false, true),
        ("notContains", json!("[긴급] 서버 다운"), json!("문의"), false, true),
        ("startsWith", json!("[긴급] 서버 다운"), json!("[긴급]"), false, true),
        ("endsWith", json!("[긴급] 서버 다운"), json!("다운"), false, true),
        ("in", json!("문의"), json!("문의, 질문"), false, true),
        ("notIn", json!("기타"), json!("문의, 질문"), false, true),
        ("greaterThan", json!("120,000원"), json!("100000"), false, true),
        ("lessThan", json!("15%"), json!("20"), false, true),
        ("greaterThanOrEqual", json!("4"), json!("4.0"), false, true),
        ("lessThanOrEqual", json!("1,000원"), json!("999"), false, false),
        ("numberEquals", json!("4"), json!("4.0"), false, true),
        ("before", json!("2026-11-04"), json!("오늘"), false, true),
        ("after", json!("2026-11-05T09:00:00+09:00"), json!("2026-11-05"), false, false),
        ("after", json!("2026-11-06"), json!("2026-11-05"), false, true),
        ("sameDay", json!("2026-11-04T16:00:00Z"), json!("2026-11-05"), false, true),
        ("isTrue", json!("예"), n.clone(), false, true),
        ("isFalse", json!("no"), n.clone(), false, true),
        ("isEmpty", json!("  "), n.clone(), false, true),
        ("isNotEmpty", json!("x"), n.clone(), false, true),
        ("equals", json!(" Hello "), json!("hello"), false, true),
        ("contains", json!("[2026] 보고서"), json!("보고서"), false, true),
        // 계획: 사람이 쓴 숫자
        ("greaterThan", json!("1,000원"), json!("999"), false, true),
        // 대소문자·공백
        ("equals", json!("True"), json!(" true "), false, true),
        ("notEquals", json!("True"), json!("true"), false, false),
        ("equals", json!("True"), json!("true"), true, false),
        ("equals", json!(" Hello "), json!("Hello"), true, true),
        ("contains", json!("ABC"), json!("b"), true, false),
        ("contains", json!("ABC"), json!("b"), false, true),
        ("in", json!("A"), json!("a, b"), true, false),
        // null = 빈 텍스트, 숫자·불리언은 표기 그대로
        ("equals", n.clone(), json!(""), false, true),
        ("equals", n.clone(), n.clone(), false, true),
        ("notEquals", n.clone(), json!(""), false, false),
        ("equals", json!("true"), json!(true), false, true),
        ("notEquals", json!("true"), json!(true), false, false),
        ("equals", json!("4"), json!(4), false, true),
        ("equals", json!("4"), json!(4.0), false, false),
        ("equals", json!(4.0), json!("4.0"), false, true),
        ("equals", json!("a"), json!("b"), false, false),
        ("notEquals", json!("a"), json!("b"), false, true),
        // 목록
        ("contains", json!("[\"긴급\",\"장애\"]"), json!("장애"), false, true),
        ("contains", json!("[\"긴급\",\"장애\"]"), json!("애"), false, false),
        ("notContains", json!("[\"긴급\",\"장애\"]"), json!("애"), false, true),
        ("contains", json!(["긴급", "장애"]), json!("애"), false, false),
        ("contains", json!("[\"긴급\"] 서버"), json!("긴"), false, true),
        ("contains", json!("[1] 공지"), json!("1"), false, true),
        ("contains", json!("{\"a\":1}"), json!("a"), false, true),
        ("in", json!("긴급"), json!("[\"긴급\",\"일반\"]"), false, true),
        ("in", json!("긴급"), json!(["긴급", "일반"]), false, true),
        ("in", json!("질문"), json!("문의, 질문 ,, "), false, true),
        ("in", json!("b"), json!("[\"A\",\" b \",\"\"]"), false, true),
        ("in", json!("1"), json!(["X", 1]), false, true),
        ("in", json!("[not json"), json!("[not json"), false, true),
        ("in", json!("4.0"), json!([4.0]), false, true),
        // 기준값이 비면 거짓
        ("contains", json!("x"), json!(""), false, false),
        ("notContains", json!("x"), json!(""), false, false),
        ("startsWith", json!("x"), json!(""), false, false),
        ("endsWith", json!("x"), json!(""), false, false),
        ("in", json!("x"), json!(""), false, false),
        ("notIn", json!("x"), json!(" , "), false, false),
        // 숫자 읽기
        ("numberEquals", json!(4), json!("4.0"), false, true),
        ("numberEquals", json!(" 15 % "), json!(15), false, true),
        ("numberEquals", json!("-1,234.5"), json!("-1234.50"), false, true),
        ("numberEquals", json!("1e3"), json!(1000), false, true),
        ("numberEquals", json!("1\u{00A0}000"), json!(1000), false, true),
        ("numberEquals", json!("1\u{3000}000"), json!(1000), false, true),
        ("numberEquals", json!(format!("{long1},원 ")), json!(long1), false, true),
        ("numberEquals", json!(long2), json!(long2), false, false),
        ("greaterThan", json!("0.10000000000000001"), json!("0.1"), false, true),
        ("lessThan", json!("-5"), json!("-0.5"), false, true),
        ("numberEquals", json!("-0"), json!("0.00"), false, true),
        ("greaterThan", json!("약 1천"), json!("100"), false, false),
        ("lessThan", json!("약 1천"), json!("100"), false, false),
        ("numberEquals", json!("₩1,000"), json!("1000"), false, false),
        ("numberEquals", json!(""), json!(""), false, false),
        ("numberEquals", json!("  "), json!(0), false, false),
        ("numberEquals", json!(true), json!(1), false, false),
        ("numberEquals", json!("1e"), json!(1), false, false),
        ("numberEquals", json!("."), json!(0), false, false),
        // 날짜 읽기
        ("before", json!("2026-11-05T09:00:00+09:00"), json!("2026-11-05T00:00:01Z"), false, true),
        ("before", json!("2026-11-05T09:00:00+09:00"), json!("2026-11-05T00:00:00Z"), false, false),
        ("after", json!("2026-11-05 09:00:00+09:00"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("2026-11-05T00:00:00.123Z"), json!("2026-11-05T00:00:00.122Z"), false, true),
        ("before", json!("2026-11-05T09:00"), json!("2026-11-05T00:00:01Z"), false, true),
        ("after", json!("2026-11-05T09:00"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("2026-11-05 09:00:30"), json!("2026-11-05T00:00:29Z"), false, true),
        ("sameDay", json!("2026.11.05"), json!("2026-11-04T15:00:00Z"), false, true),
        ("sameDay", json!("2026/11/05"), json!("2026-11-05"), false, true),
        ("before", json!("Thu, 5 Nov 2026 09:00:00 +0900"), json!("2026-11-05T00:00:01Z"), false, true),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +0900"), json!("2026-11-04T23:59:59Z"), false, true),
        ("sameDay", json!("오늘"), json!("2026-11-05"), false, true),
        ("after", json!("지금"), json!("2026-11-05T00:59:59Z"), false, true),
        ("before", json!("지금"), json!("2026-11-05T01:00:01Z"), false, true),
        ("sameDay", json!("Wed, 5 Nov 2026 09:00:00 +0900"), json!("2026-11-05"), false, false),
        ("sameDay", json!("2026-13-01"), json!("2026-11-05"), false, false),
        ("sameDay", json!("내일"), json!("오늘"), false, false),
        ("sameDay", json!("2026년 11월 7일"), json!("2026-11-07"), false, true),
        ("sameDay", json!("2026년11월7일"), json!("2026-11-07"), false, true),
        ("sameDay", json!("2026년 13월 1일"), json!("2026-11-05"), false, false),
        ("sameDay", json!("2026년 11월 7일 13:30"), json!("2026-11-07"), false, false),
        ("sameDay", json!("1794027000"), json!("1794027000000"), false, true),
        ("before", json!("1794026999"), json!("1794027000000"), false, true),
        ("sameDay", json!(1794027000), json!("1794027000000"), false, true),
        ("sameDay", json!("12345"), json!("오늘"), false, false),
        ("sameDay", json!("17940270000"), json!("오늘"), false, false),
        ("sameDay", json!("-1794027000"), json!("오늘"), false, false),
        ("sameDay", json!("0101234567"), json!("오늘"), false, false),
        ("before", json!("2026-11-05T09:00:00+09:00"), json!("2026-11-05"), false, false),
        ("before", json!("2026-11-05T09:00:00+09:00"), json!("2026-11-05T10:00:00+09:00"), false, true),
        ("before", json!("언젠가"), json!("오늘"), false, false),
        ("after", json!("오늘"), json!("언젠가"), false, false),
        // 참거짓
        ("isTrue", json!(true), n.clone(), false, true),
        ("isTrue", json!("TRUE"), n.clone(), false, true),
        ("isTrue", json!(" yes "), n.clone(), false, true),
        ("isTrue", json!("y"), n.clone(), false, true),
        ("isTrue", json!("1"), n.clone(), false, true),
        ("isTrue", json!(1), n.clone(), false, true),
        ("isTrue", json!(1.0), n.clone(), false, false),
        ("isFalse", json!(false), n.clone(), false, true),
        ("isFalse", json!("No"), n.clone(), false, true),
        ("isFalse", json!("n"), n.clone(), false, true),
        ("isFalse", json!("아니오"), n.clone(), false, true),
        ("isFalse", json!("0"), n.clone(), false, true),
        ("isFalse", json!(0), n.clone(), false, true),
        ("isTrue", json!("maybe"), n.clone(), false, false),
        ("isFalse", json!("maybe"), n.clone(), false, false),
        ("isTrue", json!(""), n.clone(), false, false),
        ("isFalse", n.clone(), n.clone(), false, false),
        // 비어 있음
        ("isEmpty", n.clone(), n.clone(), false, true),
        ("isEmpty", json!(""), n.clone(), false, true),
        ("isEmpty", json!("[]"), n.clone(), false, true),
        ("isEmpty", json!(" {} "), n.clone(), false, true),
        ("isEmpty", json!([]), n.clone(), false, true),
        ("isEmpty", json!({}), n.clone(), false, true),
        ("isEmpty", json!("0"), n.clone(), false, false),
        ("isEmpty", json!("[1]"), n.clone(), false, false),
        ("isEmpty", json!([""]), n.clone(), false, false),
        ("isEmpty", json!(false), n.clone(), false, false),
        ("isNotEmpty", json!(""), n.clone(), false, false),
        // 오프셋 경계(Java 21 OffsetDateTime.parse 실측): Z·±HH·±HH:MM·±HH:MM:ss만
        ("before", json!("2026-11-05T09:00:00+09"), json!("2026-11-05T00:00:01Z"), false, true),
        ("after", json!("2026-11-05T09:00:00+09"), json!("2026-11-04T23:59:59Z"), false, true),
        ("before", json!("2026-11-05T09:00:00+09:00:00"), json!("2026-11-05T00:00:01Z"), false, true),
        ("after", json!("2026-11-05T09:00:00+09:00:00"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("2026-11-05T00:00:00z"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("2026-11-05T09:00:00-00:30"), json!("2026-11-05T09:29:59Z"), false, true),
        ("after", json!("2026-11-05T09:00:00+18:00"), json!("2000-01-01T00:00:00Z"), false, true),
        ("after", json!("2026-11-05T09:00:00+9"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("2026-11-05T09:00:00+0900"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("2026-11-05T09:00:00+090000"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("2026-11-05T09:00:00+09:0"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("2026-11-05T09:00:00+09:00:0"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("2026-11-05T09:00:00+18:01"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("2026-11-05T09:00:00+19"), json!("2000-01-01T00:00:00Z"), false, false),
        // RFC 1123 오프셋 경계(Java 21 실측): GMT·±HH·±HHMM·±HHMMss만, 콜론 없음
        ("before", json!("Thu, 5 Nov 2026 09:00:00 +0900"), json!("2026-11-05T00:00:01Z"), false, true),
        ("before", json!("Thu, 5 Nov 2026 09:00:00 +09"), json!("2026-11-05T00:00:01Z"), false, true),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +09"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +090000"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("Thu, 5 Nov 2026 00:00:00 gmt"), json!("2026-11-04T23:59:59Z"), false, true),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 -0030"), json!("2026-11-05T09:29:59Z"), false, true),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +09:00"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +9"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 Z"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 UT"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +1801"), json!("2000-01-01T00:00:00Z"), false, false),
        ("after", json!("Thu, 5 Nov 2026 09:00:00 +0960"), json!("2000-01-01T00:00:00Z"), false, false),
        // RFC 1123 SMART 해석(Java 21 실측)
        ("sameDay", json!("31 Nov 2026 9:0:0 GMT"), json!("2026-11-30"), false, true),
        ("sameDay", json!("5 Nov 26 09:00 GMT"), json!("0026-11-05"), false, true),
        ("after", json!("4 Nov 2026 24:00 GMT"), json!("2026-11-04T23:59:59Z"), false, true),
        ("before", json!("4 Nov 2026 24:00 GMT"), json!("2026-11-05T00:00:01Z"), false, true),
        // 같은 값 경계
        ("greaterThan", json!("5"), json!("5.0"), false, false),
        ("lessThan", json!("5"), json!("5.0"), false, false),
        ("lessThanOrEqual", json!("5"), json!("5.0"), false, true),
        ("greaterThanOrEqual", json!("5"), json!("5.0"), false, true),
        ("greaterThanOrEqual", json!("4.99"), json!("5"), false, false),
        ("lessThanOrEqual", json!("5.01"), json!("5"), false, false),
        ("numberEquals", json!("5"), json!("5.01"), false, false),
        // startsWith·endsWith 부정
        ("startsWith", json!("[긴급] 서버 다운"), json!("서버"), false, false),
        ("endsWith", json!("[긴급] 서버 다운"), json!("서버"), false, false),
        // Java strip: NBSP는 지우지 않고, em space·U+001F는 지운다
        ("equals", json!("\u{00A0}a"), json!("a"), false, false),
        ("equals", json!("a\u{202F}"), json!("a"), false, false),
        ("equals", json!("\u{2003}a"), json!("a"), false, true),
        ("equals", json!("\u{1F}a"), json!("a"), false, true),
    ];
    let mut wrong = Vec::new();
    for (i, (op, l, r, cs, expected)) in rows.iter().enumerate() {
        if check(op, l.clone(), r.clone(), *cs) != *expected {
            wrong.push(format!("#{i} {op} {l} {r} cs={cs} → {}", !expected));
        }
    }
    assert!(wrong.is_empty(), "{} rows: {wrong:#?}", rows.len());
}

#[test]
fn first_match_wins() {
    let out = output(json!({"branches": [
        {"key": "b_7f3a9c21", "name": "긴급", "match": "all", "rules": [
            rule(json!("[긴급] 서버 다운"), "contains", json!("긴급")),
            rule(json!("120,000원"), "greaterThan", json!("100000"))]},
        {"key": "b_0d44e8f3", "name": "일반 문의", "match": "any",
         "rules": [rule(json!("[긴급] 서버 다운"), "in", json!("문의, 질문"))]}
    ]}));
    assert_eq!(
        serde_json::to_string(&out).unwrap(),
        r#"{"branch":"긴급","branches":["긴급"],"matched":true,"result":true,"_branchKeys":["b_7f3a9c21"]}"#
    );

    let out = output(two_true(Value::Null));
    assert_eq!(out["branches"], json!(["A"]));
    assert_eq!(out["_branchKeys"], json!(["b_aaaaaaa1"]));
}

#[test]
fn pick_all_returns_all() {
    let out = output(two_true(json!("all")));
    assert_eq!(out["branch"], json!("A"));
    assert_eq!(out["branches"], json!(["A", "B"]));
    assert_eq!(out["_branchKeys"], json!(["b_aaaaaaa1", "b_bbbbbbb2"]));
}

#[test]
fn match_any_needs_one_all_needs_every() {
    for (m, expected) in [
        (json!("any"), true),
        (json!("all"), false),
        (Value::Null, false),
    ] {
        let c = json!({"branches": [{"key": "b_a", "name": "A", "match": m, "rules": [
            rule(json!("x"), "isNotEmpty", Value::Null),
            rule(json!(""), "isNotEmpty", Value::Null)]}]});
        assert_eq!(output(c)["matched"], json!(expected), "match={m}");
    }
}

#[test]
fn otherwise_else() {
    assert_eq!(
        serde_json::to_string(&output(no_match(Value::Null))).unwrap(),
        r#"{"branch":"그 외","branches":["그 외"],"matched":false,"result":false,"_branchKeys":["else"]}"#
    );
}

#[test]
fn otherwise_stop_empty_keys() {
    assert_eq!(
        serde_json::to_string(&output(no_match(json!("stop")))).unwrap(),
        r#"{"branch":null,"branches":[],"matched":false,"result":false,"_branchKeys":[]}"#
    );
}

#[test]
fn blank_choices_use_defaults() {
    let c = json!({"branches": [{"key": "b_a", "name": "A", "match": "", "rules": [
        rule(json!("x"), "isNotEmpty", Value::Null),
        rule(json!(""), "isNotEmpty", Value::Null)]}], "pick": "", "otherwise": null});
    assert_eq!(output(c)["_branchKeys"], json!(["else"]));
}

#[test]
fn missing_name_falls_back_to_key() {
    let c = json!({"branches": [{"key": "b_a", "rules": [rule(json!("x"), "isNotEmpty", Value::Null)]}]});
    assert_eq!(output(c)["branch"], json!("b_a"));
}

#[test]
fn legacy_true_false() {
    let mut c = legacy("greaterThan", json!("120,000원"), json!("100000"));
    assert_eq!(
        serde_json::to_string(&output(c.clone())).unwrap(),
        r#"{"branch":"참","branches":["참"],"matched":true,"result":true,"_branchKeys":["true"]}"#
    );
    c["right"] = json!("200000");
    assert_eq!(
        serde_json::to_string(&output(c)).unwrap(),
        r#"{"branch":"그 외","branches":["그 외"],"matched":false,"result":false,"_branchKeys":["false"]}"#
    );

    // leftValue·rightValue 대체
    let c = json!({"operator": "equals", "leftValue": "a", "rightValue": "A"});
    assert_eq!(output(c)["result"], json!(true));
    // caseSensitive: 불리언 true·문자열 "TRUE"
    let mut c = legacy("equals", json!("True"), json!("true"));
    c["caseSensitive"] = json!(true);
    assert_eq!(output(c.clone())["result"], json!(false));
    c["caseSensitive"] = json!("TRUE");
    assert_eq!(output(c)["result"], json!(false));
    // pick·otherwise 무시
    let mut c = legacy("equals", json!("a"), json!("b"));
    c["otherwise"] = json!("stop");
    assert_eq!(output(c)["_branchKeys"], json!(["false"]));
    // branches가 ""·null·빈 목록이면 레거시
    for blank in [json!(""), Value::Null, json!([])] {
        let mut c = legacy("equals", json!("a"), json!("A"));
        c["branches"] = blank.clone();
        assert_eq!(
            output(c)["_branchKeys"],
            json!(["true"]),
            "branches={blank}"
        );
    }
}

fn ok() -> Value {
    rule(json!("x"), "isNotEmpty", Value::Null)
}

fn shape(branches: Value) -> Value {
    json!({"branches": branches})
}

fn br(key: Value, rules: Vec<Value>) -> Value {
    json!({"key": key, "name": "A", "rules": rules})
}

#[test]
fn reserved_key_fails() {
    for key in ["else", "true", "false"] {
        let e = eval(shape(json!([br(json!(key), vec![ok()])]))).unwrap_err();
        assert_eq!(
            e.message,
            format!("갈래 key로 예약어를 쓸 수 없습니다: {key}")
        );
    }
    // 공백만·대소문자 다른 key는 허용
    for key in [" ", "Else"] {
        assert!(
            eval(shape(json!([br(json!(key), vec![ok()])]))).is_ok(),
            "{key}"
        );
    }
}

#[test]
fn too_many_branches_fails() {
    let eleven: Vec<Value> = (0..11)
        .map(|i| br(json!(format!("b_{i}")), vec![ok()]))
        .collect();
    assert_eq!(
        eval(shape(json!(eleven))).unwrap_err().message,
        "갈래는 최대 10개입니다."
    );
    let ten: Vec<Value> = (0..10)
        .map(|i| br(json!(format!("b_{i}")), vec![ok()]))
        .collect();
    assert!(eval(shape(json!(ten))).is_ok());

    let e = eval(shape(json!([br(json!("b_a"), vec![ok(); 11])]))).unwrap_err();
    assert_eq!(e.message, "갈래 'A'의 조건은 최대 10개입니다.");
    assert!(eval(shape(json!([br(json!("b_a"), vec![ok(); 10])]))).is_ok());
}

#[test]
fn config_errors_fail_node() {
    let a = || br(json!("b_a"), vec![ok()]);
    let with = |k: &str, v: Value| {
        let mut c = shape(json!([a()]));
        c[k] = v;
        c
    };
    let mut bad_match = a();
    bad_match["match"] = json!("most");
    let cases = [
        (shape(json!("not-a-list")), "branches는 목록이어야 합니다."),
        (shape(json!(["x"])), "갈래는 객체여야 합니다."),
        (
            shape(json!([br(json!(""), vec![ok()])])),
            "갈래 key가 비어 있습니다.",
        ),
        (
            shape(json!([br(Value::Null, vec![ok()])])),
            "갈래 key가 비어 있습니다.",
        ),
        (
            shape(json!([br(json!(123), vec![ok()])])),
            "갈래 key는 문자열이어야 합니다: 123",
        ),
        (shape(json!([a(), a()])), "갈래 key가 중복됩니다: b_a"),
        (
            shape(json!([br(json!("b_a"), vec![])])),
            "갈래 'A'에 조건 목록이 없습니다.",
        ),
        (
            shape(json!([br(json!("b_a"), vec![json!("x")])])),
            "갈래 'A'의 조건은 객체여야 합니다.",
        ),
        (
            shape(json!([br(
                json!("b_a"),
                vec![rule(json!("x"), "regex", json!("y"))]
            )])),
            "지원하지 않는 연산자: regex",
        ),
        (shape(json!([bad_match])), "지원하지 않는 match 값: most"),
        (with("pick", json!("some")), "지원하지 않는 pick 값: some"),
        (
            with("otherwise", json!("skip")),
            "지원하지 않는 otherwise 값: skip",
        ),
        (json!({"left": "a"}), "지원하지 않는 연산자: null"),
    ];
    for (c, message) in cases {
        assert_eq!(eval(c.clone()).unwrap_err().message, message, "{c}");
    }
}

#[test]
fn undecidable_is_false_with_warning() {
    let warnings = |rules: Vec<Value>, m: &str| {
        let mut c = one(rules);
        c["branches"][0]["match"] = json!(m);
        eval(c).unwrap().warnings
    };
    let e = eval(one(vec![rule(
        json!("약 1천"),
        "greaterThan",
        json!("100000"),
    )]))
    .unwrap();
    assert_eq!(e.output["matched"], json!(false));
    assert_eq!(
        e.warnings,
        ["갈래 '긴급' 1번째 조건: '약 1천'을(를) 숫자로 읽지 못해 거짓으로 봤어요."]
    );
    // 왼쪽을 읽으면 오른쪽 탓
    assert_eq!(
        warnings(vec![rule(json!("1"), "lessThan", json!("x"))], "all"),
        ["갈래 '긴급' 1번째 조건: 'x'을(를) 숫자로 읽지 못해 거짓으로 봤어요."]
    );
    assert_eq!(
        warnings(
            vec![ok(), rule(json!("언젠가"), "after", json!("오늘"))],
            "any"
        ),
        ["갈래 '긴급' 2번째 조건: '언젠가'을(를) 날짜로 읽지 못해 거짓으로 봤어요."]
    );
    for op in [
        "contains",
        "notContains",
        "startsWith",
        "endsWith",
        "in",
        "notIn",
    ] {
        let e = eval(one(vec![rule(json!("x"), op, json!(""))])).unwrap();
        assert_eq!(e.output["matched"], json!(false), "{op}");
        assert_eq!(
            e.warnings,
            ["갈래 '긴급' 1번째 조건: 기준값이 비어 있어 거짓으로 봤어요."],
            "{op}"
        );
    }
    // 참거짓으로 못 읽는 값은 경고 없이 거짓
    assert!(warnings(vec![rule(json!("maybe"), "isTrue", Value::Null)], "all").is_empty());
    // 긴 값은 200자 + …(서로게이트 쌍은 가르지 않는다)
    let cut = format!("{}…", "가".repeat(200));
    assert_eq!(
        warnings(
            vec![rule(json!("가".repeat(300)), "greaterThan", json!("1"))],
            "all"
        ),
        [format!(
            "갈래 '긴급' 1번째 조건: '{cut}'을(를) 숫자로 읽지 못해 거짓으로 봤어요."
        )]
    );
    let left = format!("{}😀{}", "a".repeat(199), "b".repeat(10));
    assert_eq!(
        warnings(vec![rule(json!(left), "greaterThan", json!("1"))], "all"),
        [format!(
            "갈래 '긴급' 1번째 조건: '{}…'을(를) 숫자로 읽지 못해 거짓으로 봤어요.",
            "a".repeat(199)
        )]
    );
    // null은 그대로
    assert_eq!(
        warnings(vec![rule(Value::Null, "before", json!("오늘"))], "all"),
        ["갈래 '긴급' 1번째 조건: 'null'을(를) 날짜로 읽지 못해 거짓으로 봤어요."]
    );
}

#[tokio::test]
async fn builtin_runner_registered() {
    let runners = Runners::with_builtin();
    let runner = runners.get("branch.if").expect("branch.if 러너");
    let node: Node =
        serde_json::from_value(json!({"id": "c", "event": "branch.if", "type": "CONDITION"}))
            .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let config = legacy("equals", json!("a"), json!("A"))
        .as_object()
        .unwrap()
        .clone();
    let out = runner
        .run(NodeCtx {
            node: &node,
            config,
            events: &tx,
            cancel: &cancel,
        })
        .await
        .unwrap();
    assert_eq!(out["_branchKeys"], json!(["true"]));

    // 주입한 시계를 쓴다
    let fixed: Arc<dyn NodeRunner> = Arc::new(ConditionRunner::at(now()));
    let config = legacy("sameDay", json!("오늘"), json!("2026-11-05"))
        .as_object()
        .unwrap()
        .clone();
    let out = fixed
        .run(NodeCtx {
            node: &node,
            config,
            events: &tx,
            cancel: &cancel,
        })
        .await
        .unwrap();
    assert_eq!(out["result"], json!(true));

    let config = json!({"left": "a"}).as_object().unwrap().clone();
    let err = fixed
        .run(NodeCtx {
            node: &node,
            config,
            events: &tx,
            cancel: &cancel,
        })
        .await
        .unwrap_err();
    assert_eq!(err.message, "지원하지 않는 연산자: null");
}

#[test]
fn far_future_rfc_date_does_not_panic() {
    // Java는 ±999999999년까지 읽는다. chrono 범위 안이면 읽고, 밖이면 판단 불가(거짓) — panic은 안 된다.
    assert!(check(
        "sameDay",
        json!("1 Dec 262142 00:00 GMT"),
        json!("1 Dec 262142 00:00 GMT"),
        false
    ));
    for far in [
        "31 Dec 262142 23:59 GMT",
        "31 Dec 262142 24:00 GMT",
        "31 Dec 262143 23:59 GMT",
        "1 Dec 262144 00:00 GMT",
        "1 Dec 999999999 00:00 GMT",
        "31 Dec 262142 23:59 -1800",
    ] {
        for op in ["before", "after", "sameDay"] {
            assert!(
                eval(one(vec![rule(json!(far), op, json!("오늘"))])).is_ok(),
                "{far} {op}"
            );
        }
    }
}
