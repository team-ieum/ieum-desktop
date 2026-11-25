//! 숫자 포맷터 4개 — BE `NumberFormats`. 결과는 전부 문자열(지수 표기 없음).

use std::sync::LazyLock;

use bigdecimal::num_bigint::BigInt;
use bigdecimal::{BigDecimal, Pow, RoundingMode, Signed, Zero};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{Map, Value};

use super::extract::AREA;
use super::inputs::{Fail, In, R, invalid, too_large};
use super::{Entry, out};
use crate::condition::{clean_number, decimal, strip};

pub(crate) const OPS: &[Entry] = &[
    ("formatter.number_math", "계산하기", math),
    ("formatter.number_format", "숫자 표기", format),
    ("formatter.number_currency", "금액 표기", currency),
    ("formatter.number_phone", "전화번호 표기", phone),
];

type Out = R<Map<String, Value>>;

const MAX_DIGITS: i64 = 1000;

static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^(?:0({AREA})([0-9]{{3,4}})([0-9]{{4}})|(1[5-9][0-9]{{2}})([0-9]{{4}}))$"
    ))
    .unwrap()
});

const SYMBOLS: &[(&str, &str)] = &[
    ("KRW", "₩"),
    ("USD", "$"),
    ("JPY", "¥"),
    ("EUR", "€"),
    ("CNY", "¥"),
    ("GBP", "£"),
];

fn empty_input(reason: &str) -> Fail {
    Fail {
        reason: reason.into(),
        code: "FIELD_EMPTY",
        field: Some("input"),
    }
}

/// Java `precision() - scale()`·`scale()`가 각 1000 이내 — 원본 표기 그대로(0도) 본다.
fn bounded(d: BigDecimal) -> R<BigDecimal> {
    let scale = d.as_bigint_and_exponent().1;
    if scale.abs() > MAX_DIGITS || d.digits() as i64 - scale > MAX_DIGITS {
        return Err(too_large("숫자가 너무 커요(정수·소수 각 1000자리까지)"));
    }
    Ok(d)
}

/// `ConditionValues.number` + 자릿수 검사. title `None` = 입력값.
fn number(i: &In, key: &'static str, title: Option<&str>) -> R<BigDecimal> {
    let text = match title {
        Some(t) => i.required(key, t)?,
        None => i.text(key)?,
    };
    let reason = match title {
        Some(t) => format!("'{t}' 칸을 숫자로 읽지 못했어요"),
        None => "숫자로 읽지 못했어요".into(),
    };
    if text.is_empty() {
        return Err(empty_input(&reason));
    }
    let s = clean_number(&Value::String(text))
        .filter(|s| decimal(s).is_some())
        .ok_or_else(|| invalid(key, reason))?;
    // 문법은 decimal()이 Java 기준으로 확인했다 — 자릿수를 세기 전에 거대 BigInt를 만들지 않게 직접 읽는다.
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(&s)),
    };
    let (mantissa, exp) = body.split_once(['e', 'E']).unwrap_or((body, "0"));
    let exp: i64 = exp.parse().unwrap_or(0);
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{int}{frac}");
    let unscaled: BigInt = digits.parse().unwrap_or_default();
    bounded(BigDecimal::new(
        if neg { -unscaled } else { unscaled },
        frac.len() as i64 - exp,
    ))
}

/// 뒤 0 제거한 일반 표기. `-0` → `"0"`.
fn plain(d: &BigDecimal) -> String {
    if d.is_zero() {
        "0".into()
    } else {
        d.normalized().to_plain_string()
    }
}

fn ten(n: i64) -> BigInt {
    BigInt::from(10).pow(n.unsigned_abs())
}

/// `a.divide(b, 10, HALF_UP)` — 11자리까지 잘라 나눈 뒤 반올림하면 정확한 HALF_UP과 같다.
fn divide(a: &BigDecimal, b: &BigDecimal) -> BigDecimal {
    let ((ua, sa), (ub, sb)) = (a.as_bigint_and_exponent(), b.as_bigint_and_exponent());
    let k = 11 - sa + sb;
    let q = if k >= 0 {
        ua * ten(k) / ub
    } else {
        ua / (ub * ten(k))
    };
    BigDecimal::new(q, 11).with_scale_round(10, RoundingMode::HalfUp)
}

fn math(i: &In, _: DateTime<Utc>) -> Out {
    let op = i.choice(
        "operation",
        None,
        &["add", "subtract", "multiply", "divide", "negate"],
    )?;
    let a = number(i, "input", None)?;
    if op == "negate" {
        return Ok(out(plain(&-a)));
    }
    let b = number(i, "value", Some("계산할 값"))?;
    let r = match op {
        "add" => a + b,
        "subtract" => a - b,
        "multiply" => a * b,
        _ if b.is_zero() => return Err(invalid("value", "0으로 나눌 수 없어요")),
        _ => divide(&a, &b),
    };
    Ok(out(plain(&bounded(r)?)))
}

/// 정수부를 3자리마다 th로 묶고 소수점을 dec로. d의 scale은 0 이상이어야 한다.
fn grouped(d: &BigDecimal, th: &str, dec: &str) -> String {
    let s = d.abs().to_plain_string();
    let (ip, fp) = s
        .split_once('.')
        .map_or((s.as_str(), None), |(a, b)| (a, Some(b)));
    let mut b = String::from(if d.is_negative() { "-" } else { "" });
    for (n, c) in ip.chars().enumerate() {
        if n > 0 && (ip.len() - n) % 3 == 0 {
            b.push_str(th);
        }
        b.push(c);
    }
    if let Some(fp) = fp {
        b.push_str(dec);
        b.push_str(fp);
    }
    b
}

fn format(i: &In, _: DateTime<Utc>) -> Out {
    let th = match i.choice(
        "thousands",
        Some("comma"),
        &["comma", "period", "space", "none"],
    )? {
        "comma" => ",",
        "period" => ".",
        "space" => " ",
        _ => "",
    };
    let dec = match i.choice("decimal", Some("period"), &["period", "comma"])? {
        "comma" => ",",
        _ => ".",
    };
    if th == dec {
        return Err(invalid("decimal", "천 단위 구분과 소수점 기호가 같아요"));
    }
    let d = number(i, "input", None)?;
    let scale = d.as_bigint_and_exponent().1.max(0);
    Ok(out(grouped(&d.with_scale(scale), th, dec)))
}

fn currency(i: &In, _: DateTime<Utc>) -> Out {
    let currencies: Vec<&str> = SYMBOLS.iter().map(|s| s.0).collect();
    let cur = i.choice("currency", Some("KRW"), &currencies)?;
    let style = i.choice("style", Some("korean"), &["korean", "symbol"])?;
    let scale = if matches!(cur, "KRW" | "JPY") { 0 } else { 2 };
    let d = number(i, "input", None)?.with_scale_round(scale, RoundingMode::HalfUp);
    let body = grouped(&d.abs(), ",", ".");
    let sign = if d.is_negative() { "-" } else { "" };
    Ok(out(if cur == "KRW" && style == "korean" {
        format!("{sign}{body}원")
    } else {
        let symbol = SYMBOLS.iter().find(|s| s.0 == cur).map_or("", |s| s.1);
        format!("{sign}{symbol}{body}")
    }))
}

fn phone(i: &In, _: DateTime<Utc>) -> Out {
    const REASON: &str = "한국 전화번호로 읽지 못했어요";
    let format = i.choice("format", Some("dash"), &["dash", "international", "digits"])?;
    let text = i.text("input")?;
    let raw = strip(&text);
    if raw.is_empty() {
        return Err(empty_input(REASON));
    }
    let mut digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    if raw.starts_with("+82") {
        digits.drain(..2);
        if !digits.starts_with('0') {
            digits.insert(0, '0');
        }
    }
    let m = PHONE
        .captures(&digits)
        .ok_or_else(|| invalid("input", REASON))?;
    let area = m.get(1).is_some();
    let parts: Vec<&str> = (if area { 1..4 } else { 4..6 })
        .map(|g| m.get(g).map_or("", |x| x.as_str()))
        .collect();
    Ok(out(match format {
        "digits" => digits.clone(),
        "international" => format!("+82 {}", parts.join("-")),
        _ => format!("{}{}", if area { "0" } else { "" }, parts.join("-")),
    }))
}
