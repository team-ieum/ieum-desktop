//! 목록 포맷터 3개 — BE `ListFormats`.

use std::str::FromStr;

use bigdecimal::{BigDecimal, Signed, Zero};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::inputs::{In, MAX_ITEMS, MAX_TEXT, R, invalid, json, parse_json, too_large};
use super::{Entry, out};
use crate::condition::strip;

pub(crate) const OPS: &[Entry] = &[
    ("formatter.lookup_table", "대응표로 바꾸기", lookup_table),
    ("formatter.list_pick", "목록에서 고르기", pick),
    ("formatter.list_to_text", "목록을 글로", to_text),
];

type Out = R<Map<String, Value>>;

/// 읽힌 JSON 배열·객체 글의 1단계 원소 원문(객체는 `"키": 값` 통째).
fn top_level(t: &str) -> Vec<&str> {
    let (mut depth, mut in_str, mut esc, mut start) = (0, false, false, 1);
    let mut parts = Vec::new();
    for (k, c) in t.char_indices() {
        if in_str {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    parts.push(t[start..k].trim());
                }
            }
            ',' if depth == 1 => {
                parts.push(t[start..k].trim());
                start = k + 1;
            }
            _ => {}
        }
    }
    parts
}

/// 한 목록·표에서 펴는 숫자 글의 합계 상한 — 짧은 `1e-99999`를 1만 개 넣으면 1GB가 되지 않게.
const MAX_DECIMALS_TOTAL: u64 = MAX_TEXT as u64 * 10;

/// BE는 JSON 글 속 소수를 `BigDecimal`로 읽어 원문 자릿수 그대로 plain 글자로 쓴다(`1.50`, `1e5` → `100000`).
/// f64를 거치면 끝 0·정밀도가 사라지므로 원문에서 다시 읽는다. `total`은 지금까지 편 글자 수.
fn decimal(v: &Value, literal: &str, total: &mut u64) -> R<Value> {
    if !v.is_f64() {
        return Ok(v.clone());
    }
    let Ok(d) = BigDecimal::from_str(literal) else {
        return Ok(v.clone());
    };
    // Java처럼 음의 scale인 0(`0e5`)은 `0`.
    if d.is_zero() && d.fractional_digit_count() < 0 {
        return Ok("0".into());
    }
    // 펴기 전에 길이를 잰다 — `0e-999999999` 같은 표기가 수억 자 글이 되지 않게.
    let (n, scale) = (d.digits(), d.fractional_digit_count());
    let len = u64::from(d.is_negative())
        + match scale {
            ..=0 => n + scale.unsigned_abs(),
            s if n > s as u64 => n + 1,
            s => 2 + s as u64,
        };
    if len > MAX_TEXT as u64 {
        return Err(too_large("목록 속 숫자가 10만 자를 넘어요"));
    }
    *total += len;
    if *total > MAX_DECIMALS_TOTAL {
        return Err(too_large("목록 속 숫자가 모두 합해 100만 자를 넘어요"));
    }
    Ok(d.to_plain_string().into())
}

fn lookup_table(i: &In, _: DateTime<Utc>) -> Out {
    let not_table = "'대응표' 칸이 대응표(키-값) 모양이 아니에요";
    let raw = i.0.get("table").unwrap_or(&Value::Null);
    // 참조식으로 치환된 대응표는 JSON 객체 문자열로 온다.
    let parsed = match raw {
        Value::String(s) if strip(s).starts_with('{') => {
            let t = strip(s);
            if t.encode_utf16().count() > MAX_TEXT {
                return Err(too_large("'table' 칸이 10만 자를 넘어요"));
            }
            match parse_json::<Map<String, Value>>(t)? {
                Some(mut m) => {
                    if m.values().any(Value::is_f64) {
                        let mut total = 0;
                        // 뒤에서부터 — 겹친 키는 마지막 값이 남고, 바꾼 값은 더는 소수가 아니라 건너뛴다.
                        for entry in top_level(t).into_iter().rev() {
                            let mut de =
                                serde_json::Deserializer::from_str(entry).into_iter::<String>();
                            let Some(Ok(key)) = de.next() else { continue };
                            let literal = entry[de.byte_offset()..].trim_start();
                            let literal = literal.strip_prefix(':').unwrap_or(literal).trim();
                            if let Some(v) = m.get_mut::<String>(&key) {
                                *v = decimal(v, literal, &mut total)?;
                            }
                        }
                    }
                    Some(m)
                }
                None => None,
            }
        }
        Value::Object(m) => Some(m.clone()),
        _ => None,
    };
    let Some(table) = parsed else {
        let mut fail = invalid("table", not_table);
        if raw.is_null() || raw.as_str() == Some("") {
            fail.code = "FIELD_EMPTY";
        }
        return Err(fail);
    };
    if table.len() > MAX_ITEMS {
        return Err(too_large("대응표가 1만 개를 넘어요"));
    }
    let key = i.text("input")?;
    let key = strip(&key);
    if key.is_empty() {
        return Ok(out(""));
    }
    Ok(out(match table.get(key) {
        None => i.text("fallback")?,
        Some(Value::Null) => String::new(),
        Some(v) => json(v),
    }))
}

/// 문자열 입력은 10만 자 제한을 거치고, JSON 글에서 읽은 1단계 소수는 plain 글자가 된다.
fn items(i: &In) -> R<Vec<Value>> {
    let Some(Value::String(_)) = i.0.get("input") else {
        return i.list("input");
    };
    let s = i.text("input")?;
    let items = i.list("input")?;
    if !items.iter().any(Value::is_f64) {
        return Ok(items);
    }
    let mut total = 0;
    items
        .iter()
        .zip(top_level(strip(&s)))
        .map(|(v, literal)| decimal(v, literal, &mut total))
        .collect()
}

fn pick(i: &In, _: DateTime<Utc>) -> Out {
    let items = items(i)?;
    if items.is_empty() {
        return Err(invalid("input", "목록이 비어 있어요"));
    }
    let n = match i.choice("position", Some("first"), &["first", "last", "nth"])? {
        "first" => 0,
        "last" => items.len() - 1,
        _ => i.positive_int("index", "몇 번째")? - 1,
    };
    let Some(picked) = items.get(n) else {
        return Err(invalid(
            "index",
            format!("{}번째 항목이 없어요(목록 {}개)", n + 1, items.len()),
        ));
    };
    Ok(out(picked.clone()))
}

fn to_text(i: &In, _: DateTime<Utc>) -> Out {
    let sep = i.text("separator")?;
    let sep = if sep.is_empty() { ", " } else { sep.as_str() };
    let parts: Vec<String> = items(i)?
        .iter()
        .map(|v| if v.is_null() { String::new() } else { json(v) })
        .collect();
    // 이어 붙이기 전에 잰다 — 1만 자 구분 기호 × 1만 개면 1억 자가 생긴다.
    let total = sep.encode_utf16().count() * parts.len().saturating_sub(1)
        + parts
            .iter()
            .map(|p| p.encode_utf16().count())
            .sum::<usize>();
    if total > MAX_TEXT {
        return Err(too_large("결과가 10만 자를 넘어요"));
    }
    Ok(out(parts.join(sep)))
}
