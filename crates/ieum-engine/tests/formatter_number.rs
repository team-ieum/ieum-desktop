//! BE `NumberFormatsTest`의 숫자 포맷터 4개 케이스.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::{Map, Value, json};

fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

fn cfg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn case(event: &str, input: Value, want: &str) {
    let got = apply(event, &cfg(input.clone()), now())
        .unwrap_or_else(|e| panic!("{event} {input}: {}", e.message));
    assert_eq!(got["output"], json!(want), "{event} {input}");
}

/// 실패 이유(메시지 안)와 코드.
fn fails(event: &str, input: Value, reason: &str, code: &str) {
    let e = apply(event, &cfg(input.clone()), now()).unwrap_err();
    assert!(e.message.contains(reason), "{event} {input}: {}", e.message);
    assert_eq!(e.code.as_deref(), Some(code), "{event} {input}");
}

const MATH: &str = "formatter.number_math";
const FORMAT: &str = "formatter.number_format";
const CURRENCY: &str = "formatter.number_currency";
const PHONE: &str = "formatter.number_phone";
const BIG: &str = "숫자가 너무 커요(정수·소수 각 1000자리까지)";
const NOT_PHONE: &str = "한국 전화번호로 읽지 못했어요";

#[test]
fn number_math() {
    case(
        MATH,
        json!({"input":"1,000원","operation":"add","value":"2.5"}),
        "1002.5",
    );
    case(
        MATH,
        json!({"input":"10","operation":"subtract","value":"3"}),
        "7",
    );
    case(
        MATH,
        json!({"input":"1000","operation":"multiply","value":"1"}),
        "1000",
    );
    case(
        MATH,
        json!({"input":"10","operation":"divide","value":"3"}),
        "3.3333333333",
    );
    // HALF_UP·음수·뒤 0 제거·몫이 0으로 반올림되는 나눗셈
    for (a, b, want) in [
        ("-10", "3", "-3.3333333333"),
        ("2", "3", "0.6666666667"),
        ("-2", "3", "-0.6666666667"),
        ("1", "8", "0.125"),
        ("1", "2e3", "0.0005"),
        ("0.00000000000000000001", "1", "0"),
        ("1e20", "0.5", "200000000000000000000"),
    ] {
        case(
            MATH,
            json!({"input":a,"operation":"divide","value":b}),
            want,
        );
    }
    fails(
        MATH,
        json!({"input":"10","operation":"divide","value":"0"}),
        "0으로 나눌 수 없어요",
        "FIELD_INVALID value",
    );
    case(MATH, json!({"input":"5","operation":"negate"}), "-5");
    fails(
        MATH,
        json!({"input":"5","operation":"add"}),
        "'계산할 값' 칸이 비어 있어요",
        "FIELD_EMPTY value",
    );
    fails(
        MATH,
        json!({"input":"5","operation":"add","value":"약 1천"}),
        "'계산할 값' 칸을 숫자로 읽지 못했어요",
        "FIELD_INVALID value",
    );
    fails(
        MATH,
        json!({"input":"약 1천","operation":"add","value":"1"}),
        "계산하기: 숫자로 읽지 못했어요",
        "FIELD_INVALID input",
    );
    fails(
        MATH,
        json!({"input":"5"}),
        "'operation' 칸을 골라 주세요",
        "FIELD_EMPTY operation",
    );
}

#[test]
fn huge_exponent() {
    let start = Instant::now();
    fails(
        MATH,
        json!({"input":"1e999999999","operation":"multiply","value":"1"}),
        BIG,
        "TOO_LARGE",
    );
    fails(
        MATH,
        json!({"input":"1","operation":"add","value":"1e-999999999"}),
        BIG,
        "TOO_LARGE",
    );
    let nines = "9".repeat(900);
    fails(
        MATH,
        json!({"input":nines,"operation":"multiply","value":nines}),
        BIG,
        "TOO_LARGE",
    );
    fails(FORMAT, json!({"input":"1e999999999"}), BIG, "TOO_LARGE");
    fails(CURRENCY, json!({"input":"1e999999999"}), BIG, "TOO_LARGE");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn scale_bypass() {
    let start = Instant::now();
    let long = format!("1{}e-1900", "0".repeat(900));
    for s in ["0e-999999999", "0e-1000000", "1e2147483647", &long] {
        fails(
            MATH,
            json!({"input":s,"operation":"add","value":"1"}),
            BIG,
            "TOO_LARGE",
        );
        fails(
            MATH,
            json!({"input":"1","operation":"multiply","value":s}),
            BIG,
            "TOO_LARGE",
        );
        fails(FORMAT, json!({"input":s}), BIG, "TOO_LARGE");
        fails(CURRENCY, json!({"input":s}), BIG, "TOO_LARGE");
    }
    fails(
        MATH,
        json!({"input":"0e-999999999","operation":"negate"}),
        BIG,
        "TOO_LARGE",
    );
    let spaced = format!("{}1", " ".repeat(200_000));
    fails(
        FORMAT,
        json!({"input":spaced}),
        "'input' 칸이 10만 자를 넘어요",
        "TOO_LARGE",
    );
    fails(
        MATH,
        json!({"input":"1","operation":"add","value":spaced}),
        "'value' 칸이 10만 자를 넘어요",
        "TOO_LARGE",
    );
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn currency_rounding_and_zero() {
    case(
        CURRENCY,
        json!({"input":"12000.5","currency":"KRW"}),
        "12,001원",
    );
    case(CURRENCY, json!({"input":"1.005","currency":"USD"}), "$1.01");
    case(
        CURRENCY,
        json!({"input":"12000","currency":"CNY"}),
        "¥12,000.00",
    );
    case(
        CURRENCY,
        json!({"input":"12000","currency":"GBP"}),
        "£12,000.00",
    );
    case(MATH, json!({"input":"-0","operation":"negate"}), "0");
    case(
        MATH,
        json!({"input":"5","operation":"subtract","value":"5"}),
        "0",
    );
    case(FORMAT, json!({"input":"-0"}), "0");
    case(CURRENCY, json!({"input":"-0"}), "0원");
}

/// 0의 여러 표기 — BE(Java 21) 실측 결과.
#[test]
fn zero_notations() {
    for (v, formatted) in [
        ("0e5", "0"),
        ("0e-5", "0.00000"),
        ("0.0", "0.0"),
        ("-0.0", "0.0"),
        ("0E+3", "0"),
    ] {
        case(MATH, json!({"input":v,"operation":"negate"}), "0");
        case(MATH, json!({"input":v,"operation":"add","value":"0"}), "0");
        case(FORMAT, json!({"input":v}), formatted);
        case(CURRENCY, json!({"input":v,"currency":"USD"}), "$0.00");
        case(CURRENCY, json!({"input":v}), "0원");
    }
}

#[test]
fn phone_edges() {
    fails(
        PHONE,
        json!({"input":"010123456789"}),
        NOT_PHONE,
        "FIELD_INVALID input",
    );
    fails(
        PHONE,
        json!({"input":"01212345678"}),
        NOT_PHONE,
        "FIELD_INVALID input",
    );
    case(
        PHONE,
        json!({"input":"+82 010-1234-5678","format":"international"}),
        "+82 10-1234-5678",
    );
    case(
        PHONE,
        json!({"input":"+82 10-1234-5678","format":"dash"}),
        "010-1234-5678",
    );
}

#[test]
fn number_format() {
    case(FORMAT, json!({"input":"1234567.891"}), "1,234,567.891");
    case(FORMAT, json!({"input":"1234.50"}), "1,234.50");
    case(
        FORMAT,
        json!({"input":"1234567.891","thousands":"period","decimal":"comma"}),
        "1.234.567,891",
    );
    case(
        FORMAT,
        json!({"input":"1234567.891","thousands":"none"}),
        "1234567.891",
    );
    case(
        FORMAT,
        json!({"input":"-1234","thousands":"space"}),
        "-1 234",
    );
    case(FORMAT, json!({"input":"1.2e3"}), "1,200");
    fails(
        FORMAT,
        json!({"input":"1234567.891","thousands":"comma","decimal":"comma"}),
        "천 단위 구분과 소수점 기호가 같아요",
        "FIELD_INVALID decimal",
    );
    fails(
        FORMAT,
        json!({"input":"1","thousands":"comma","decimal":"comma"}),
        "천 단위 구분과 소수점 기호가 같아요",
        "FIELD_INVALID decimal",
    );
}

#[test]
fn number_currency() {
    case(CURRENCY, json!({"input":"12000"}), "12,000원");
    case(
        CURRENCY,
        json!({"input":"12000","style":"symbol"}),
        "₩12,000",
    );
    case(
        CURRENCY,
        json!({"input":"12000.6","currency":"KRW"}),
        "12,001원",
    );
    case(
        CURRENCY,
        json!({"input":"12000","currency":"USD","style":"korean"}),
        "$12,000.00",
    );
    case(
        CURRENCY,
        json!({"input":"12","currency":"USD","style":"korean"}),
        "$12.00",
    );
    case(
        CURRENCY,
        json!({"input":"1500.4","currency":"JPY"}),
        "¥1,500",
    );
    case(
        CURRENCY,
        json!({"input":"12000","currency":"EUR"}),
        "€12,000.00",
    );
    case(CURRENCY, json!({"input":"-500","currency":"KRW"}), "-500원");
    case(CURRENCY, json!({"input":"-12000"}), "-12,000원");
}

#[test]
fn number_phone() {
    case(PHONE, json!({"input":"01012345678"}), "010-1234-5678");
    case(
        PHONE,
        json!({"input":"+82 10 1234 5678","format":"international"}),
        "+82 10-1234-5678",
    );
    case(PHONE, json!({"input":"02-345-6789"}), "02-345-6789");
    case(PHONE, json!({"input":"0313456789"}), "031-345-6789");
    case(PHONE, json!({"input":"15881234"}), "1588-1234");
    case(
        PHONE,
        json!({"input":"010-1234-5678","format":"digits"}),
        "01012345678",
    );
    fails(
        PHONE,
        json!({"input":"12345"}),
        NOT_PHONE,
        "FIELD_INVALID input",
    );
    fails(
        PHONE,
        json!({"input":"123"}),
        NOT_PHONE,
        "FIELD_INVALID input",
    );
}

/// BE는 빈 입력도 못 읽은 값으로 실패시킨다 — AI용 코드는 빈 칸으로 알린다.
#[test]
fn empty_input() {
    fails(
        MATH,
        json!({"input":"","operation":"negate"}),
        "숫자로 읽지 못했어요",
        "FIELD_EMPTY input",
    );
    fails(
        FORMAT,
        json!({"input":""}),
        "숫자로 읽지 못했어요",
        "FIELD_EMPTY input",
    );
    fails(
        CURRENCY,
        json!({"input":""}),
        "숫자로 읽지 못했어요",
        "FIELD_EMPTY input",
    );
    fails(PHONE, json!({"input":""}), NOT_PHONE, "FIELD_EMPTY input");
}
