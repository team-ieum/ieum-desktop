use std::collections::HashMap;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde_json::{Map, Value};

/// 노드 id → 그 노드의 출력.
pub type Outputs = HashMap<String, Map<String, Value>>;

pub(crate) static VARIABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{\{nodes\.([a-zA-Z0-9-]+)\.output\.([a-zA-Z0-9_.]+)\}\}").unwrap()
});

/// 문자열 값의 참조식을 단일 패스로 치환한 복사본. Map 키는 그대로, 결과는 항상 문자열(BE `renderDeep`과 동일).
pub fn render_deep(value: &Value, outputs: &Outputs) -> Value {
    match value {
        Value::String(s) => Value::String(
            VARIABLE
                .replace_all(s, |c: &Captures| resolve_field(outputs, &c[1], &c[2]))
                .into_owned(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), render_deep(v, outputs)))
                .collect(),
        ),
        Value::Array(list) => Value::Array(list.iter().map(|v| render_deep(v, outputs)).collect()),
        other => other.clone(),
    }
}

/// 문자열 값이 직접 참조하는 노드 id — 등장 순서, 중복 없음. Map 키는 훑지 않는다.
pub fn referenced_node_ids(value: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    collect(value, &mut ids);
    ids
}

fn collect(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            for c in VARIABLE.captures_iter(s) {
                if !ids.iter().any(|id| id == &c[1]) {
                    ids.push(c[1].to_string());
                }
            }
        }
        Value::Object(map) => map.values().for_each(|v| collect(v, ids)),
        Value::Array(list) => list.iter().for_each(|v| collect(v, ids)),
        _ => {}
    }
}

fn resolve_field(outputs: &Outputs, node_id: &str, field_path: &str) -> String {
    let Some(output) = outputs.get(node_id).filter(|o| !o.is_empty()) else {
        return String::new();
    };
    // Java `String.split`처럼 끝의 빈 세그먼트는 버린다.
    let mut keys: Vec<&str> = field_path.split('.').collect();
    while keys.last() == Some(&"") {
        keys.pop();
    }
    let mut current: Option<&Value> = None; // None = output 자체(Map)
    for key in keys {
        let next = match current {
            None => output.get(key),
            Some(Value::Object(m)) => m.get(key),
            Some(Value::Array(list)) if is_index(key) => {
                key.parse::<usize>().ok().and_then(|i| list.get(i))
            }
            Some(scalar) => return stringify(scalar),
        };
        match next {
            None | Some(Value::Null) => return String::new(),
            v => current = v,
        }
    }
    match current {
        None => serde_json::to_string(output).unwrap_or_default(),
        Some(v) => stringify(v),
    }
}

fn is_index(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit())
}

/// Map·List는 JSON 문자열, 문자열은 그대로, 그 외는 표기 그대로.
fn stringify(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
