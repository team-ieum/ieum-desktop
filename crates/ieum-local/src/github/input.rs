//! 칸 값 해석 — agent `tools/github.py`의 도우미와 1:1. 참조식이 숫자·문자열 어느 쪽을 줘도 같은 뜻이 되게.

use ieum_engine::NodeError;
use serde_json::{Map, Value};

use super::api::input_error;

/// URL 경로 조각(owner·repo). `/`·`..`·공백이 들어가면 다른 GitHub API 경로로 샌다.
pub(crate) fn is_path_name(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
            && !s.trim_matches('.').is_empty()
    })
}

/// 이슈·마일스톤 번호. 사람이 친 `#12`·앞뒤 공백, 참조식이 준 정수를 받는다. bool·1 미만은 None.
pub(crate) fn issue_no(v: &Value) -> Option<u64> {
    let n = match v {
        Value::Number(n) => n.as_u64()?,
        Value::String(s) => {
            let s = s.trim();
            let s = s.strip_prefix('#').unwrap_or(s).trim();
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            s.parse().ok()?
        }
        _ => return None,
    };
    (n >= 1).then_some(n)
}

/// 쉼표 구분 → 순서 유지 중복 제거 목록.
pub(crate) fn csv(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in v.as_str().unwrap_or("").split(',').map(str::trim) {
        if !p.is_empty() && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    }
    out
}

pub(crate) fn text(v: &Value) -> String {
    v.as_str().map(|s| s.trim().to_string()).unwrap_or_default()
}

/// GitHub login(사용자·조직).
#[allow(dead_code)] // find_issue(다음 작업)가 쓴다.
pub(crate) fn is_login(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        (1..=39).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    })
}

/// 비어 있지 않은 라벨·담당자·마일스톤만. 라벨·담당자는 보내면 통째 교체라 빈 값은 넣지 않는다.
pub(crate) fn issue_fields(
    labels: &Value,
    assignees: &Value,
    milestone: &Value,
) -> Result<Map<String, Value>, NodeError> {
    let mut f = Map::new();
    let names = csv(labels);
    if !names.is_empty() {
        f.insert("labels".into(), names.into());
    }
    let logins = csv(assignees);
    if !logins.is_empty() {
        f.insert("assignees".into(), logins.into());
    }
    // agent `str(milestone).strip()`처럼 참조식이 준 숫자도 빈 값이 아니다.
    let empty = match milestone {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    };
    if !empty {
        let n =
            issue_no(milestone).ok_or_else(|| input_error("milestone은 양의 정수여야 합니다."))?;
        f.insert("milestone".into(), n.into());
    }
    Ok(f)
}

/// 보냈는데 응답에 없는 라벨·담당자·마일스톤(권한 없음·없는 이름은 GitHub가 조용히 버린다).
pub(crate) fn dropped(sent: &Map<String, Value>, issue: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    for (key, name, what) in [("labels", "name", "라벨"), ("assignees", "login", "담당자")] {
        let got: Vec<String> = issue[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| x[name].as_str().unwrap_or("").to_lowercase())
            .collect();
        let missing: Vec<&str> = sent
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|n| !got.contains(&n.to_lowercase()))
            .collect();
        if !missing.is_empty() {
            warnings.push(format!("반영되지 않은 {what}: {}", missing.join(", ")));
        }
    }
    if let Some(m) = sent.get("milestone")
        && issue["milestone"]["number"] != *m
    {
        warnings.push(format!("반영되지 않은 마일스톤: {m}"));
    }
    warnings
}
