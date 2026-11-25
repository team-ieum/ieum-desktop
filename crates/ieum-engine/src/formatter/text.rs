//! 글 포맷터 8개 — BE `TextFormats`.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use unicode_segmentation::UnicodeSegmentation;

use super::inputs::{In, MAX_ITEMS, MAX_TEXT, R, invalid, too_large};
use super::{Entry, out};
use crate::condition::{is_empty, strip};

pub(crate) const OPS: &[Entry] = &[
    ("formatter.text_case", "대소문자 바꾸기", case),
    ("formatter.text_trim", "공백 정리", trim),
    ("formatter.text_replace", "찾아 바꾸기", replace),
    ("formatter.text_truncate", "길이 자르기", truncate),
    ("formatter.text_split", "텍스트 나누기", split),
    ("formatter.text_default", "기본값 넣기", default_value),
    ("formatter.text_url_encode", "URL 인코딩", url_encode),
    ("formatter.text_length", "글자 수 세기", length),
];

type Out = R<Map<String, Value>>;

fn utf16(s: &str) -> i64 {
    s.encode_utf16().count() as i64
}

/// Java `Character.isWhitespace`.
fn java_ws(c: char) -> bool {
    strip(c.encode_utf8(&mut [0; 4])).is_empty()
}

// ponytail: Java Character.toUpperCase/toLowerCase는 한 글자 대응이라 여러 글자로 바뀌는 경우(ß 등)는
// 대문자는 그대로 두고 소문자는 첫 글자만 쓴다 — 그리스 iota 하첨자 같은 드문 단순 대응은 다를 수 있다.
fn upper(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn case(i: &In, _: DateTime<Utc>) -> Out {
    let s = i.text("input")?;
    let mode = i.choice(
        "mode",
        Some("title"),
        &["upper", "lower", "capitalize", "title"],
    )?;
    Ok(out(match mode {
        "upper" => s.to_uppercase(),
        "lower" => s.to_lowercase(),
        "capitalize" => {
            let mut cs = s.chars();
            cs.next()
                .map_or(String::new(), |c| upper(c).to_string() + cs.as_str())
        }
        _ => {
            let mut start = true;
            s.chars()
                .map(|c| {
                    let r = if start { upper(c) } else { lower(c) };
                    start = java_ws(c);
                    r
                })
                .collect()
        }
    }))
}

fn trim(i: &In, _: DateTime<Utc>) -> Out {
    Ok(out(strip(&i.text("input")?)))
}

fn notation(s: &str) -> String {
    s.replace("[:space:]", " ")
        .replace("[:newline:]", "\n")
        .replace("[:tab:]", "\t")
}

fn replace(i: &In, _: DateTime<Utc>) -> Out {
    let find = notation(&i.required("find", "찾을 글자")?);
    let replacement = notation(&i.text("replace")?);
    let s = i.text("input")?;
    // 결과를 만들기 전에 길이를 잰다 — 만들고 나서 재면 거대한 문자열이 생긴다.
    let count = s.matches(find.as_str()).count() as i64;
    if utf16(&s) + count * (utf16(&replacement) - utf16(&find)) > MAX_TEXT as i64 {
        return Err(too_large("바꾼 결과가 10만 자를 넘어요"));
    }
    Ok(out(s.replace(&find, &replacement)))
}

/// grapheme 경계 바이트 오프셋(0과 끝 포함).
fn bounds(s: &str) -> Vec<usize> {
    s.grapheme_indices(true)
        .map(|(b, _)| b)
        .chain([s.len()])
        .collect()
}

fn truncate(i: &In, _: DateTime<Utc>) -> Out {
    let max = i.positive_int("maxLength", "최대 글자 수")?;
    let s = i.text("input")?;
    let b = bounds(&s);
    if b.len() - 1 <= max {
        return Ok(out(s));
    }
    Ok(out(if i.flag("ellipsis") {
        format!("{}…", &s[..b[max - 1]])
    } else {
        s[..b[max]].to_string()
    }))
}

fn split(i: &In, _: DateTime<Utc>) -> Out {
    let part = i.choice("part", Some("first"), &["first", "second", "last", "all"])?;
    let sep = i.text("separator")?;
    let sep = if sep.is_empty() { "," } else { sep.as_str() };
    let s = i.text("input")?;
    if part == "all" {
        if s.is_empty() {
            return Ok(out(Vec::<Value>::new()));
        }
        if s.matches(sep).count() + 1 > MAX_ITEMS {
            return Err(too_large("결과 목록이 1만 개를 넘어요"));
        }
    }
    let mut pieces = s.split(sep).map(strip);
    Ok(out(match part {
        "all" => Value::from(pieces.collect::<Vec<_>>()),
        "second" => pieces.nth(1).unwrap_or("").into(),
        "last" => pieces.last().unwrap_or("").into(),
        _ => pieces.next().unwrap_or("").into(),
    }))
}

fn default_value(i: &In, _: DateTime<Utc>) -> Out {
    let dflt = i.required("defaultValue", "기본값")?;
    let s = i.text("input")?;
    Ok(out(if is_empty(&Value::String(s.clone())) {
        dflt
    } else {
        s
    }))
}

fn url_encode(i: &In, _: DateTime<Utc>) -> Out {
    let s = i.text("input")?;
    if i.choice("direction", Some("encode"), &["encode", "decode"])? == "encode" {
        // Java URLEncoder 안전 문자, 공백은 %20.
        let enc = s
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                    (b as char).to_string()
                }
                b' ' => "%20".into(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>();
        return Ok(out(enc));
    }
    url_decode(&s)
        .map(out)
        .ok_or_else(|| invalid("input", "URL 인코딩을 풀지 못했어요"))
}

/// Java `URLDecoder.decode` — `+`는 공백, 이어진 `%XX`는 UTF-8로(깨진 바이트는 대체 문자).
fn url_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut res = Vec::with_capacity(b.len());
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'+' => {
                res.push(b' ');
                k += 1;
            }
            b'%' => {
                let mut run = Vec::new();
                while k < b.len() && b[k] == b'%' {
                    // Java Integer.parseInt처럼 부호 `+`도 받는다.
                    let hex = std::str::from_utf8(b.get(k + 1..k + 3)?).ok()?;
                    run.push(u8::from_str_radix(hex, 16).ok()?);
                    k += 3;
                }
                res.extend_from_slice(String::from_utf8_lossy(&run).as_bytes());
            }
            c => {
                res.push(c);
                k += 1;
            }
        }
    }
    String::from_utf8(res).ok()
}

fn length(i: &In, _: DateTime<Utc>) -> Out {
    let s = i.text("input")?;
    let n = if i.choice("unit", Some("chars"), &["chars", "words"])? == "words" {
        // Java 정규식 기본 `\s`(ASCII).
        strip(&s)
            .split([' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
            .filter(|w| !w.is_empty())
            .count()
    } else {
        bounds(&s).len() - 1
    };
    Ok(out(n as u64))
}
