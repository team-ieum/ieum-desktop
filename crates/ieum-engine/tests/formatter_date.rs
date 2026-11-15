//! BE `DateFormatsTest`의 날짜 포맷터 3개 케이스.

use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::{Map, Value, json};

/// 2026-11-07 13:30 KST 토요일.
fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

fn cfg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn run(event: &str, input: &Value) -> Map<String, Value> {
    apply(event, &cfg(input.clone()), now())
        .unwrap_or_else(|e| panic!("{event} {input}: {}", e.message))
}

fn case(event: &str, input: Value, want: &str) {
    assert_eq!(run(event, &input)["output"], json!(want), "{event} {input}");
}

fn case_v(event: &str, input: Value, want: Value) {
    assert_eq!(Value::Object(run(event, &input)), want, "{event} {input}");
}

/// 실패 이유(메시지 안)와 코드.
fn fails(event: &str, input: Value, reason: &str, code: &str) {
    let e = apply(event, &cfg(input.clone()), now()).unwrap_err();
    assert!(e.message.contains(reason), "{event} {input}: {}", e.message);
    assert_eq!(e.code.as_deref(), Some(code), "{event} {input}");
}

const FORMAT: &str = "formatter.date_format";
const ADD: &str = "formatter.date_add";
const COMPARE: &str = "formatter.date_compare";
const T: &str = "2026-11-07T04:30:00Z";
const T_KST: &str = "2026-11-07T13:30:00+09:00";
const UNREAD: &str = "날짜로 읽지 못했어요";
const BAD_PATTERN: &str = "'직접 정한 모양'이 올바른 날짜 패턴이 아니에요";
const BAD_EXPR: &str = "기간 식을 읽지 못했어요(예: +1 month -2 days)";
const ADD_RANGE: &str = "계산한 날짜가 표기할 수 있는 범위를 넘었어요";

fn fmt(input: Value) -> String {
    run(FORMAT, &input)["output"].as_str().unwrap().to_string()
}

#[test]
fn date_format() {
    case(FORMAT, json!({"input": T}), "2026년 11월 7일 (토)");
    case(
        FORMAT,
        json!({"input": T, "format": "korean_time"}),
        "2026년 11월 7일 (토) 13:30",
    );
    case(FORMAT, json!({"input": T, "format": "date"}), "2026-11-07");
    case(
        FORMAT,
        json!({"input": T, "format": "datetime"}),
        "2026-11-07 13:30",
    );
    case(FORMAT, json!({"input": T, "format": "iso"}), T_KST);
    case(
        FORMAT,
        json!({"input": T, "format": "custom", "customPattern": "yyyy/MM/dd E"}),
        "2026/11/07 토",
    );
    case(
        FORMAT,
        json!({"input": "지금", "format": "korean_time"}),
        "2026년 11월 7일 (토) 13:30",
    );
    case(
        FORMAT,
        json!({"input": "2026.11.07", "format": "datetime"}),
        "2026-11-07 00:00",
    );
    case(
        FORMAT,
        json!({"input": T, "format": "datetime", "toTimezone": "America/New_York"}),
        "2026-11-06 23:30",
    );
    case(
        FORMAT,
        json!({"input": "2026-11-07T04:30", "format": "datetime", "fromTimezone": "UTC"}),
        "2026-11-07 13:30",
    );
    fails(
        FORMAT,
        json!({"input": T, "format": "custom"}),
        "'직접 정한 모양' 칸이 비어 있어요",
        "FIELD_EMPTY customPattern",
    );
    let a87 = "a".repeat(87);
    case(
        FORMAT,
        json!({"input": T, "format": "custom", "customPattern": format!("yyyy-MM-dd '{a87}'")}),
        &format!("2026-11-07 {a87}"),
    );
    fails(
        FORMAT,
        json!({"input": T, "format": "custom", "customPattern": format!("yyyy-MM-dd '{a87}a'")}),
        "'직접 정한 모양' 칸은 100자까지예요",
        "TOO_LARGE",
    );
    fails(
        FORMAT,
        json!({"input": T, "format": "custom", "customPattern": "yyyy-MM-dd'"}),
        BAD_PATTERN,
        "FIELD_INVALID customPattern",
    );
    fails(
        FORMAT,
        json!({"input": T, "toTimezone": "Mars/Base"}),
        "알 수 없는 시간대예요: Mars/Base",
        "FIELD_INVALID toTimezone",
    );
    fails(
        FORMAT,
        json!({"input": T, "fromTimezone": "Mars/Base"}),
        "알 수 없는 시간대예요",
        "FIELD_INVALID fromTimezone",
    );
    let e = apply(FORMAT, &cfg(json!({"input": "다음주"})), now()).unwrap_err();
    assert_eq!(
        e.message,
        "날짜 표기 바꾸기: '다음주'를 날짜로 읽지 못했어요 (입력: '다음주')"
    );
    assert_eq!(e.code.as_deref(), Some("FIELD_INVALID input"));
}

#[test]
fn date_format_plan_cases() {
    case(
        FORMAT,
        json!({"input": "2026-11-07"}),
        "2026년 11월 7일 (토)",
    );
    case(
        FORMAT,
        json!({"input": T, "format": "iso", "toTimezone": "UTC"}),
        "2026-11-07T04:30:00Z",
    );
    case(
        FORMAT,
        json!({"input": "2026-11-07 13:30", "format": "custom", "customPattern": "yy/M/d a h:mm"}),
        "26/11/7 오후 1:30",
    );
    fails(
        FORMAT,
        json!({"input": "2026-11-07", "format": "custom", "customPattern": "yyyy-QQ"}),
        "지원하지 않는 날짜 패턴 문자예요: Q",
        "FIELD_INVALID customPattern",
    );
    fails(
        FORMAT,
        json!({"input": "2026-11-07", "toTimezone": "Mars/Base"}),
        "알 수 없는 시간대예요",
        "FIELD_INVALID toTimezone",
    );
    fails(
        FORMAT,
        json!({"input": T, "format": "week"}),
        "'format' 값을 알 수 없어요: week",
        "FIELD_INVALID format",
    );
}

#[test]
fn custom_pattern_letters() {
    let p = |pattern: &str| {
        fmt(
            json!({"input": "2026-11-07T09:05:03+09:00", "format": "custom", "customPattern": pattern}),
        )
    };
    assert_eq!(p("y yy yyy yyyy yyyyy"), "2026 26 2026 2026 02026");
    assert_eq!(p("M MM d dd"), "11 11 7 07");
    assert_eq!(p("E EEE EEEE EEEEE"), "토 토 토요일 토");
    assert_eq!(p("H HH h hh m mm s ss a"), "9 09 9 09 5 05 3 03 오전");
    assert_eq!(p("'at' HH'시' ''1'' 'it''s'"), "at 09시 '1' it's");
    assert_eq!(p("yyyy년 M월 d일, (E) / :-"), "2026년 11월 7일, (토) / :-");
    assert_eq!(
        fmt(
            json!({"input": "2026-11-07T00:30:00+09:00", "format": "custom", "customPattern": "a h"})
        ),
        "오전 12"
    );
    for (pattern, letter) in [("MMM", "M"), ("yyyy [MM]", "["), ("SSS", "S"), ("VV", "V")] {
        fails(
            FORMAT,
            json!({"input": T, "format": "custom", "customPattern": pattern}),
            &format!("지원하지 않는 날짜 패턴 문자예요: {letter}"),
            "FIELD_INVALID customPattern",
        );
    }
    for pattern in ["ddd", "aa", "HHH", "EEEEEE", "{", "#", "b"] {
        fails(
            FORMAT,
            json!({"input": T, "format": "custom", "customPattern": pattern}),
            BAD_PATTERN,
            "FIELD_INVALID customPattern",
        );
    }
}

#[test]
fn zone_forms() {
    let to = |zone: &str| fmt(json!({"input": T, "format": "iso", "toTimezone": zone}));
    assert_eq!(to("Z"), "2026-11-07T04:30:00Z");
    assert_eq!(to("+09:00"), T_KST);
    assert_eq!(to("+9"), T_KST);
    assert_eq!(to("-0330"), "2026-11-07T01:00:00-03:30");
    assert_eq!(to("UTC+05:30"), "2026-11-07T10:00:00+05:30");
    assert_eq!(to(" Asia/Kolkata "), "2026-11-07T10:00:00+05:30");
    fails(
        FORMAT,
        json!({"input": T, "toTimezone": "+19:00"}),
        "알 수 없는 시간대예요",
        "FIELD_INVALID toTimezone",
    );
}

// Review Focus 5 — 서머타임 틈·겹침(뉴욕 2026-03-08 02:30은 없는 시각, 2026-11-01 01:30은 두 번).
// Java LocalDateTime.atZone: 틈이면 틈 전 오프셋(EST, -5), 겹치면 이른 쪽(EDT, -4).
#[test]
fn dst_gap_and_overlap_do_not_panic() {
    assert_eq!(
        fmt(
            json!({"input":"2026-03-08 02:30","fromTimezone":"America/New_York","toTimezone":"UTC","format":"datetime"})
        ),
        "2026-03-08 07:30"
    );
    assert_eq!(
        fmt(
            json!({"input":"2026-11-01 01:30","fromTimezone":"America/New_York","toTimezone":"UTC","format":"datetime"})
        ),
        "2026-11-01 05:30"
    );
    // 날짜만인 값은 fromTimezone의 0시.
    assert_eq!(
        fmt(
            json!({"input":"2026-03-08","fromTimezone":"America/New_York","toTimezone":"America/New_York","format":"iso"})
        ),
        "2026-03-08T00:00:00-05:00"
    );
}

#[test]
fn date_add() {
    case(
        ADD,
        json!({"input": T_KST, "expression": "+3일 -2시간"}),
        "2026-11-10T11:30:00+09:00",
    );
    case(
        ADD,
        json!({"input": T_KST, "expression": "+1 month -2 days"}),
        "2026-12-05T13:30:00+09:00",
    );
    case(
        ADD,
        json!({"input": T_KST, "expression": "+2개월"}),
        "2027-01-07T13:30:00+09:00",
    );
    case(
        ADD,
        json!({"input": T_KST, "expression": "+1달"}),
        "2026-12-07T13:30:00+09:00",
    );
    case(
        ADD,
        json!({"input": T_KST, "expression": "+1 Year"}),
        "2027-11-07T13:30:00+09:00",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": "+3 parsecs"}),
        "알 수 없는 단위예요: parsecs",
        "FIELD_INVALID expression",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": "3일"}),
        BAD_EXPR,
        "FIELD_INVALID expression",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": ""}),
        "'더하거나 뺄 기간' 칸이 비어 있어요",
        "FIELD_EMPTY expression",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": "+1일 x"}),
        BAD_EXPR,
        "FIELD_INVALID expression",
    );
}

#[test]
fn date_add_plan_cases() {
    case(
        ADD,
        json!({"input": "2026-01-31", "expression": "+1달"}),
        "2026-02-28",
    );
    case(
        ADD,
        json!({"input": "2026-01-31", "expression": "+1일 +2시간"}),
        "2026-02-01T02:00:00+09:00",
    );
}

#[test]
fn date_add_month_end() {
    case(
        ADD,
        json!({"input": "2026-01-31", "expression": "+1 month"}),
        "2026-02-28",
    );
    // 항마다 말일로 맞춘다(Java LocalDate.plus 순서대로).
    case(
        ADD,
        json!({"input": "2026-01-31", "expression": "+1달 +1달"}),
        "2026-03-28",
    );
    case(
        ADD,
        json!({"input": "2028-02-29", "expression": "+1년"}),
        "2029-02-28",
    );
}

#[test]
fn date_add_date_only() {
    case(
        ADD,
        json!({"input": "2026-11-07", "expression": "+1주"}),
        "2026-11-14",
    );
    case(
        ADD,
        json!({"input": "2026-11-07", "expression": "+3시간"}),
        "2026-11-07T03:00:00+09:00",
    );
}

#[test]
fn date_add_bounds() {
    fails(
        ADD,
        json!({"input": T_KST, "expression": "+999999999999 years"}),
        "기간 숫자가 너무 커요(6자리까지)",
        "TOO_LARGE",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": "+999999 years ".repeat(1000)}),
        "기간 식은 20항까지예요",
        "TOO_LARGE",
    );
    case(
        ADD,
        json!({"input": "2026-11-07", "expression": "+999999일"}),
        "4764-10-03",
    );
    fails(
        ADD,
        json!({"input": "2026-11-07", "expression": "+1000000일"}),
        "기간 숫자가 너무 커요(6자리까지)",
        "TOO_LARGE",
    );
    case(
        ADD,
        json!({"input": "2026-11-07", "expression": "+1일 ".repeat(20)}),
        "2026-11-27",
    );
    fails(
        ADD,
        json!({"input": "2026-11-07", "expression": "+1일 ".repeat(21)}),
        "기간 식은 20항까지예요",
        "TOO_LARGE",
    );
}

#[test]
fn out_of_range() {
    // Java ±999999999년 끝값은 chrono 범위 밖이라 읽기에서 실패한다(GAPS chrono 범위 행).
    fails(
        ADD,
        json!({"input": "+999999999-01-01", "expression": "+1 year"}),
        UNREAD,
        "FIELD_INVALID input",
    );
    fails(
        ADD,
        json!({"input": "-999999999-01-01", "expression": "-1 day"}),
        UNREAD,
        "FIELD_INVALID input",
    );
    fails(
        ADD,
        json!({"input": "+999999999-12-31T14:59:59Z", "expression": "+1 hour"}),
        UNREAD,
        "FIELD_INVALID input",
    );
    fails(
        FORMAT,
        json!({"input": "-999999999-01-01", "fromTimezone": "UTC", "toTimezone": "America/New_York"}),
        UNREAD,
        "FIELD_INVALID input",
    );
    // chrono 범위 끝을 넘는 계산은 패닉 없이 실패.
    fails(
        ADD,
        json!({"input": "2026-11-07", "expression": "+999999 years"}),
        ADD_RANGE,
        "TOO_LARGE",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": "+999999 years"}),
        ADD_RANGE,
        "TOO_LARGE",
    );
    fails(
        ADD,
        json!({"input": T_KST, "expression": "-999999 weeks ".repeat(20)}),
        ADD_RANGE,
        "TOO_LARGE",
    );
}

#[test]
fn truncates_seconds_and_today() {
    let t = "2026-11-07T13:30:45.678+09:00";
    case(
        FORMAT,
        json!({"input": t, "format": "iso"}),
        "2026-11-07T13:30:45+09:00",
    );
    case(
        ADD,
        json!({"input": t, "expression": "+1분"}),
        "2026-11-07T13:31:45+09:00",
    );
    case(
        FORMAT,
        json!({"input": "오늘", "format": "date"}),
        "2026-11-07",
    );
}

#[test]
fn date_compare() {
    let a = "2026-11-07T09:00:00+09:00";
    let b = "2026-11-08T12:30:00+09:00";
    case_v(
        COMPARE,
        json!({"input": a, "other": b}),
        json!({"days": 1, "hours": 27, "minutes": 1650, "isBefore": true}),
    );
    case_v(
        COMPARE,
        json!({"input": b, "other": a}),
        json!({"days": -1, "hours": -27, "minutes": -1650, "isBefore": false}),
    );
    case_v(
        COMPARE,
        json!({"input": a, "other": a}),
        json!({"days": 0, "hours": 0, "minutes": 0, "isBefore": false}),
    );
    case_v(
        COMPARE,
        json!({"input": "2026-11-07", "other": "2026-11-09T01:00:00+09:00"}),
        json!({"days": 2, "hours": 48, "minutes": 2880, "isBefore": true}),
    );
    case_v(
        COMPARE,
        json!({"input": "2026-11-07", "other": "2026-11-09T23:00:00+09:00"}),
        json!({"days": 2, "hours": 48, "minutes": 2880, "isBefore": true}),
    );
    // Java Duration은 초를 내림으로 나눈다: -59.5초 → -1분.
    case_v(
        COMPARE,
        json!({"input": "2026-11-07T00:00:59.5Z", "other": "2026-11-07T00:00:00Z"}),
        json!({"days": 0, "hours": 0, "minutes": -1, "isBefore": false}),
    );
    fails(
        COMPARE,
        json!({"input": a, "other": ""}),
        "'비교할 날짜' 칸이 비어 있어요",
        "FIELD_EMPTY other",
    );
    fails(
        COMPARE,
        json!({"input": a}),
        "'비교할 날짜' 칸이 비어 있어요",
        "FIELD_EMPTY other",
    );
}

#[test]
fn empty_input() {
    fails(
        FORMAT,
        json!({"input": ""}),
        "'입력값' 칸이 비어 있어요",
        "FIELD_EMPTY input",
    );
    fails(
        ADD,
        json!({"input": "", "expression": "+1일"}),
        "'입력값' 칸이 비어 있어요",
        "FIELD_EMPTY input",
    );
    fails(
        COMPARE,
        json!({"input": "", "other": "2026-11-07"}),
        "'기준 날짜' 칸이 비어 있어요",
        "FIELD_EMPTY input",
    );
}

const EDGE: &str = "+999999999-12-31T23:59:59-18:00";

#[test]
fn read_out_of_range() {
    fails(
        FORMAT,
        json!({"input": EDGE}),
        UNREAD,
        "FIELD_INVALID input",
    );
    fails(
        ADD,
        json!({"input": EDGE, "expression": "+1일"}),
        UNREAD,
        "FIELD_INVALID input",
    );
    fails(
        COMPARE,
        json!({"input": "2026-11-07", "other": EDGE}),
        UNREAD,
        "FIELD_INVALID other",
    );
}
