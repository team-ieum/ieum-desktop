//! 포맷터 공용 입력 읽기·제한 — BE `FormatterInputs`. 글 길이는 Java처럼 UTF-16 단위로 센다.

use serde_json::{Map, Value};

use crate::condition::{bool_of, decimal, java_str, strip};

pub(crate) const MAX_TEXT: usize = 100_000;
pub(crate) const MAX_ITEMS: usize = 10_000;
pub(crate) const MAX_PATTERN: usize = 200;
pub(crate) const MAX_CUSTOM_PATTERN: usize = 100;
const SUMMARY_LENGTH: usize = 50;
static NULL: Value = Value::Null;

/// 포맷터 실패 — 이유(화면용 BE 문구)와 AI용 코드.
#[derive(Debug)]
pub(crate) struct Fail {
    pub reason: String,
    pub code: &'static str,
    pub field: Option<&'static str>,
}

pub(crate) type R<T> = Result<T, Fail>;

pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Fail {
    Fail {
        reason: reason.into(),
        code: "FIELD_INVALID",
        field: Some(field),
    }
}

pub(crate) fn too_large(reason: impl Into<String>) -> Fail {
    Fail {
        reason: reason.into(),
        code: "TOO_LARGE",
        field: None,
    }
}

fn empty(field: &'static str, reason: String) -> Fail {
    Fail {
        reason,
        code: "FIELD_EMPTY",
        field: Some(field),
    }
}

/// 문자열은 그대로, 그 밖은 JSON.
pub(crate) fn json(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 실패 메시지용 입력 요약 — 앞 50자 + `…`(서로게이트 쌍은 가르지 않는다).
pub(crate) fn summary(s: &str) -> String {
    if s.encode_utf16().count() <= SUMMARY_LENGTH {
        return s.to_string();
    }
    let mut n = 0;
    let cut: String = s
        .chars()
        .take_while(|c| {
            n += c.len_utf16();
            n <= SUMMARY_LENGTH
        })
        .collect();
    cut + "…"
}

/// JSON 글 읽기. f64 범위 밖 숫자 때문에 못 읽으면 BE(BigDecimal)와 다른 값을 내지 않게 실패,
/// 그 밖의 실패는 `None`(호출부가 BE처럼 다른 방법으로 읽는다).
/// serde_json은 이 오류를 Category::Syntax로만 묶어 공개하므로 문구로 가린다 —
/// 문구가 바뀌면 `out_of_range_message_is_pinned` 테스트가 깨진다.
pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(t: &str) -> R<Option<T>> {
    match serde_json::from_str(t) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_syntax() && e.to_string().starts_with("number out of range") => {
            Err(too_large("목록 속 숫자가 너무 커요"))
        }
        Err(_) => Ok(None),
    }
}

/// 런타임이 치환해 넘긴 설정.
pub(crate) struct In<'a>(pub &'a Map<String, Value>);

impl In<'_> {
    fn get(&self, key: &str) -> &Value {
        self.0.get(key).unwrap_or(&NULL)
    }

    /// null → `""`, Map·List는 JSON, 그 밖은 글자로. 10만 자 초과는 실패.
    pub fn text(&self, key: &'static str) -> R<String> {
        let s = match self.get(key) {
            Value::Null => String::new(),
            v => json(v),
        };
        if s.encode_utf16().count() > MAX_TEXT {
            return Err(too_large(format!("'{key}' 칸이 10만 자를 넘어요")));
        }
        Ok(s)
    }

    pub fn required(&self, key: &'static str, title: &str) -> R<String> {
        let s = self.text(key)?;
        if s.is_empty() {
            return Err(empty(key, format!("'{title}' 칸이 비어 있어요")));
        }
        Ok(s)
    }

    /// 없거나 빈 값이면 기본값(기본값이 없으면 필수 선택 칸이라 실패), 허용 목록 밖이면 실패.
    pub fn choice(
        &self,
        key: &'static str,
        default: Option<&'static str>,
        allowed: &[&'static str],
    ) -> R<&'static str> {
        let s = match self.get(key) {
            Value::Null => String::new(),
            v => java_str(v),
        };
        if s.is_empty() {
            return default.ok_or_else(|| empty(key, format!("'{key}' 칸을 골라 주세요")));
        }
        allowed
            .iter()
            .find(|a| **a == s)
            .copied()
            .ok_or_else(|| invalid(key, format!("'{key}' 값을 알 수 없어요: {}", summary(&s))))
    }

    pub fn flag(&self, key: &str) -> bool {
        bool_of(self.get(key)) == Some(true)
    }

    /// `new BigDecimal(strip).intValueExact()`가 1 이상이어야 한다.
    pub fn positive_int(&self, key: &'static str, title: &str) -> R<usize> {
        let text = self.text(key)?;
        let n = decimal(strip(&text)).and_then(|d| {
            let len = d.digits.len() as i64;
            if d.neg || len == 0 || len > d.pos || d.pos > 10 {
                return None;
            }
            let n = (0..d.pos as usize).fold(0i64, |n, i| {
                n * 10 + i64::from(d.digits.get(i).copied().unwrap_or(0))
            });
            (n <= i64::from(i32::MAX)).then_some(n as usize)
        });
        n.ok_or_else(|| invalid(key, format!("'{title}' 칸은 1 이상의 정수여야 해요")))
    }

    /// List는 그대로, `[`로 시작하는 JSON 배열은 그 배열, 아니면 쉼표로 나눠 앞뒤 공백 제거(빈 원소도 자리를 지킨다).
    /// null·공백뿐인 글 → 빈 목록.
    pub fn list(&self, key: &'static str) -> R<Vec<Value>> {
        let items = match self.get(key) {
            Value::Null => Vec::new(),
            Value::Array(a) => a.clone(),
            v => {
                let s = java_str(v);
                let t = strip(&s);
                if t.is_empty() {
                    Vec::new()
                } else if let Some(a) = t
                    .starts_with('[')
                    .then(|| parse_json::<Vec<Value>>(t))
                    .transpose()?
                    .flatten()
                {
                    a
                } else {
                    s.split(',').map(|p| strip(p).into()).collect()
                }
            }
        };
        if items.len() > MAX_ITEMS {
            return Err(too_large("목록이 1만 개를 넘어요"));
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn with(key: &str, v: Value) -> Map<String, Value> {
        Map::from_iter([(key.to_string(), v)])
    }

    #[test]
    fn out_of_range_message_is_pinned() {
        let e = serde_json::from_str::<Value>("[1e99999]").unwrap_err();
        assert!(e.is_syntax(), "{e}");
        assert!(e.to_string().starts_with("number out of range"), "{e}");
        assert!(parse_json::<Value>("[-1e400]").is_err());
        assert!(matches!(parse_json::<Value>("[1] x"), Ok(None)));
    }

    #[test]
    fn text_over_limit_counts_utf16() {
        let m = with("input", json!("😀".repeat(50_001)));
        let e = In(&m).text("input").unwrap_err();
        assert_eq!(e.code, "TOO_LARGE");
        assert_eq!(e.reason, "'input' 칸이 10만 자를 넘어요");
        let m = with("input", json!("😀".repeat(50_000)));
        assert!(In(&m).text("input").is_ok());
        let m = with("input", json!({"a": [1]}));
        assert_eq!(In(&m).text("input").unwrap(), r#"{"a":[1]}"#);
    }

    #[test]
    fn required_empty() {
        let m = Map::new();
        let e = In(&m).required("defaultValue", "기본값").unwrap_err();
        assert_eq!(
            (e.reason.as_str(), e.code, e.field),
            (
                "'기본값' 칸이 비어 있어요",
                "FIELD_EMPTY",
                Some("defaultValue")
            )
        );
    }

    #[test]
    fn positive_int_reads_big_decimal() {
        let n = |v: Value| In(&with("n", v)).positive_int("n", "개수");
        assert_eq!(n(json!("3.0")).unwrap(), 3);
        assert_eq!(n(json!(" 1e2 ")).unwrap(), 100);
        assert_eq!(n(json!(2)).unwrap(), 2);
        assert_eq!(n(json!("2147483647")).unwrap(), 2_147_483_647);
        for bad in ["3.5", "0", "-1", "", "abc", "2147483648"] {
            let e = n(json!(bad)).unwrap_err();
            assert_eq!(e.reason, "'개수' 칸은 1 이상의 정수여야 해요");
            assert_eq!((e.code, e.field), ("FIELD_INVALID", Some("n")));
        }
    }

    #[test]
    fn list_rules() {
        let l = |v: Value| In(&with("input", v)).list("input").unwrap();
        assert_eq!(l(json!("a,,b")), [json!("a"), json!(""), json!("b")]);
        assert_eq!(l(json!(" [1,2] x")), [json!("[1"), json!("2] x")]);
        assert_eq!(l(json!(" [1, {\"a\":1}] ")), [json!(1), json!({"a": 1})]);
        assert_eq!(l(json!([1, "x"])), [json!(1), json!("x")]);
        assert!(l(json!("")).is_empty());
        assert!(l(json!(" \t")).is_empty());
        assert!(l(Value::Null).is_empty());
        let many = vec![json!(1); MAX_ITEMS + 1];
        let e = In(&with("input", json!(many))).list("input").unwrap_err();
        assert_eq!(
            (e.reason.as_str(), e.code),
            ("목록이 1만 개를 넘어요", "TOO_LARGE")
        );
    }

    #[test]
    fn choice_rules() {
        let m = with("mode", json!(""));
        assert_eq!(In(&m).choice("mode", Some("a"), &["a", "b"]).unwrap(), "a");
        let e = In(&m).choice("mode", None, &["a", "b"]).unwrap_err();
        assert_eq!(e.reason, "'mode' 칸을 골라 주세요");
        assert_eq!(
            In(&with("mode", json!("b")))
                .choice("mode", None, &["a", "b"])
                .unwrap(),
            "b"
        );
        let e = In(&with("mode", json!("c")))
            .choice("mode", None, &["a", "b"])
            .unwrap_err();
        assert_eq!(
            (e.reason.as_str(), e.code),
            ("'mode' 값을 알 수 없어요: c", "FIELD_INVALID")
        );
    }

    #[test]
    fn flag_reads_korean() {
        assert!(In(&with("f", json!("예"))).flag("f"));
        assert!(In(&with("f", json!(true))).flag("f"));
        assert!(!In(&with("f", json!("아니오"))).flag("f"));
        assert!(!In(&Map::new()).flag("f"));
    }

    #[test]
    fn summary_keeps_surrogate_pairs() {
        assert_eq!(summary("abc"), "abc");
        let s = format!("{}😀z", "a".repeat(49));
        assert_eq!(summary(&s), format!("{}…", "a".repeat(49)));
    }
}
