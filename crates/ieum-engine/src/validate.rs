//! 정적 검증(1단계분) — ieum-agent `core/validators/static_validator.py`를 옮겼다(에러 순서·문구 동일).
//! `REQUIRED`·`TYPE`·`CHOICE`·`CRON`·`TOO_LARGE`는 1단계 범위 밖이라 옮기지 않았다.
//! 필수 칸·선택지는 채팅 빌더용 `validate_fields`가 따로 본다(`validate`의 결과는 그대로).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::{Catalog, EventDef, InputField, NodeType};
use crate::model::{Edge, Node, Workflow};
use crate::render::VARIABLE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    RefNotAllowed,
    RefFormat,
    RefUnknownNode,
    RefSelf,
    RefNotUpstream,
    RefUnknownField,
    TriggerCount,
    EdgeInvalid,
    OrphanNode,
    /// `validate_fields`만 낸다(`validate`는 내지 않는다).
    Required,
    Choice,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub node_id: Option<String>,
    pub field: Option<String>,
    pub code: ErrorCode,
    pub message: String,
}

use ErrorCode::*;

/// 참조식 본문 {{ 여기 }} — 중괄호 없는 본문만(닫히지 않은 `{{` 반복에서 느려지지 않게).
static REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([^{}]*)\}\}").unwrap());

const RIGHT_UNUSED_OPERATORS: [&str; 4] = ["isEmpty", "isNotEmpty", "isTrue", "isFalse"];
const LEGACY_CONDITION_PATHS: [&str; 3] = ["config.operator", "config.left", "config.right"];
const MAX_BRANCHES: usize = 10;
const MAX_RULES: usize = 10;
const MAX_LISTED_BRANCHES: usize = 10;
const MAX_ERRORS: usize = 200;
const MAX_ERROR_BYTES: usize = 200_000;
const MAX_VALUE_LEN: usize = 80;
const MAX_LOC: usize = 256;

/// 워크플로우 전체 검증. 에러가 없으면 빈 목록. 같은 에러는 처음 하나만, MAX_ERRORS건·MAX_ERROR_BYTES에서 자른다.
pub fn validate(wf: &Workflow, catalog: &Catalog) -> Vec<ValidationError> {
    let (nodes, edges) = (&wf.nodes, &wf.edges);
    let mut out = Collector::default();
    for e in graph_errors(nodes, edges, catalog) {
        if !out.push(e) {
            return out.errors;
        }
    }
    // 같은 id면 뒤 노드가 이긴다(Python dict 컴프리헨션과 같게)
    let by_id: HashMap<&str, &Node> = nodes
        .iter()
        .filter_map(|n| sid(&n.id).map(|id| (id, n)))
        .collect();
    let preds = predecessors(edges, &by_id);
    let mut outputs = HashMap::new();
    for node in nodes {
        for e in field_errors(node, catalog) {
            if !out.push(e) {
                return out.errors;
            }
        }
        if !ref_errors(node, catalog, &by_id, &preds, &mut outputs, &mut out) {
            return out.errors;
        }
    }
    out.errors
}

#[derive(Default)]
struct Collector {
    seen: HashSet<ValidationError>,
    errors: Vec<ValidationError>,
    size: usize,
}

impl Collector {
    /// false = 상한에 닿아 더 받지 않는다.
    fn push(&mut self, e: ValidationError) -> bool {
        if self.seen.contains(&e) {
            return true;
        }
        self.size += py_json_len(&e);
        if !self.errors.is_empty() && self.size > MAX_ERROR_BYTES {
            return false;
        }
        self.seen.insert(e.clone());
        self.errors.push(e);
        self.errors.len() < MAX_ERRORS
    }
}

/// Python `len(json.dumps(e))`(ensure_ascii, 기본 구분자)와 같은 길이.
fn py_json_len(e: &ValidationError) -> usize {
    fn s(v: Option<&str>) -> usize {
        v.map_or(4, |v| {
            2 + v
                .chars()
                .map(|c| match c {
                    '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
                    ' '..='~' => 1,
                    c if c as u32 > 0xFFFF => 12,
                    _ => 6,
                })
                .sum::<usize>()
        })
    }
    // `{"nodeId": , "field": , "code": , "message": }` = 46자
    46 + s(e.node_id.as_deref())
        + s(e.field.as_deref())
        + serde_json::to_string(&e.code).unwrap().len()
        + s(Some(&e.message))
}

fn err(
    code: ErrorCode,
    message: String,
    node_id: Option<&str>,
    field: Option<String>,
) -> ValidationError {
    ValidationError {
        node_id: node_id.map(clip),
        field,
        code,
        message,
    }
}

/// 메시지에 싣는 사용자 값 — MAX_VALUE_LEN자를 넘으면 자르고 '…'.
fn clip(s: &str) -> String {
    match s.char_indices().nth(MAX_VALUE_LEN) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// 노드 id·엣지 끝점으로 쓸 수 있는 값(비어 있지 않은 문자열).
fn sid(s: &str) -> Option<&str> {
    (!s.is_empty()).then_some(s)
}

/// Python `_get_by_path(node, path)` — `config.` 아래만 읽는다. 숫자 세그먼트 = 목록 인덱스.
fn get_path<'a>(node: &'a Node, path: &str) -> Option<&'a Value> {
    let mut segs = path.strip_prefix("config.")?.split('.');
    let first = segs.next()?;
    if is_digits(first) {
        return None;
    }
    let mut cur = node.config.get(first)?;
    for seg in segs {
        cur = if is_digits(seg) {
            cur.as_array()?.get(seg.parse::<usize>().ok()?)?
        } else {
            cur.as_object()?.get(seg)?
        };
    }
    Some(cur)
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Python `value in (None, "", [])`.
fn is_empty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// Python `str(v or "")` — 스칼라만 Python 표기와 같다(목록·객체는 JSON 표기).
fn py_str_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
        Some(Value::Array(a)) if a.is_empty() => String::new(),
        Some(Value::Object(o)) if o.is_empty() => String::new(),
        Some(other) => other.to_string(),
    }
}

fn field_path(f: &InputField) -> String {
    f.path
        .clone()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| format!("config.{}", f.key))
}

/// `loc == p` 또는 `loc`이 `p.`로 시작.
fn under(loc: &str, p: &str) -> bool {
    loc.strip_prefix(p)
        .is_some_and(|r| r.is_empty() || r.starts_with('.'))
}

// ── 그래프 ─────────────────────────────────────────────

fn graph_errors(nodes: &[Node], edges: &[Edge], catalog: &Catalog) -> Vec<ValidationError> {
    if nodes.is_empty() {
        return vec![err(
            TriggerCount,
            "워크플로우에 노드가 없습니다.".into(),
            None,
            None,
        )];
    }
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    let mut first: HashMap<&str, &Node> = HashMap::new();
    let mut triggers = Vec::new();
    for (idx, node) in nodes.iter().enumerate() {
        let Some(nid) = sid(&node.id) else {
            errors.push(err(
                EdgeInvalid,
                format!("{}번째 노드에 id가 없습니다.", idx + 1),
                None,
                None,
            ));
            continue;
        };
        if !seen.insert(nid) {
            errors.push(err(
                EdgeInvalid,
                format!("노드 id '{}'가 중복됩니다.", clip(nid)),
                Some(nid),
                None,
            ));
        }
        first.entry(nid).or_insert(node);
        match node.r#type.as_deref().filter(|t| !t.is_empty()) {
            None => errors.push(err(
                EdgeInvalid,
                format!("노드 '{}'에 type이 없습니다.", clip(nid)),
                Some(nid),
                None,
            )),
            Some(t) if t.to_uppercase() == "TRIGGER" => triggers.push(nid),
            Some(_) => {}
        }
    }
    if triggers.len() != 1 {
        errors.push(err(
            TriggerCount,
            format!("트리거는 1개여야 합니다(현재 {}개).", triggers.len()),
            None,
            None,
        ));
    }
    let trigger_id = (triggers.len() == 1).then(|| triggers[0]);
    let mut allowed: HashMap<&str, Option<Allowed>> = HashMap::new();
    for (idx, edge) in edges.iter().enumerate() {
        let (Some(source), Some(target)) = (sid(&edge.source), sid(&edge.target)) else {
            errors.push(err(
                EdgeInvalid,
                format!("{}번째 연결에 출발 또는 도착 노드가 없습니다.", idx + 1),
                None,
                None,
            ));
            continue;
        };
        let known = seen.contains(source) && seen.contains(target);
        if !known {
            let missing = if seen.contains(source) {
                target
            } else {
                source
            };
            errors.push(err(
                EdgeInvalid,
                format!(
                    "연결({} → {})이 없는 노드 '{}'를 가리킵니다.",
                    clip(source),
                    clip(target),
                    clip(missing)
                ),
                None,
                None,
            ));
        } else if Some(target) == trigger_id {
            errors.push(err(
                EdgeInvalid,
                "트리거로 들어오는 연결은 둘 수 없습니다.".into(),
                trigger_id,
                None,
            ));
        }
        if known {
            let a = allowed
                .entry(source)
                .or_insert_with(|| branch_allowed(first[source], catalog));
            if let Some(a) = a {
                errors.extend(branch_edge_error(
                    source,
                    target,
                    edge.condition_type.as_deref(),
                    a,
                ));
            }
        }
    }
    if let Some(tid) = trigger_id {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in edges {
            if let (Some(s), Some(t)) = (sid(&edge.source), sid(&edge.target))
                && seen.contains(s)
                && seen.contains(t)
            {
                adj.entry(s).or_default().push(t);
            }
        }
        let (mut reached, mut stack) = (HashSet::new(), vec![tid]);
        while let Some(cur) = stack.pop() {
            if reached.insert(cur) {
                stack.extend(adj.get(cur).into_iter().flatten());
            }
        }
        for node in nodes {
            if let Some(nid) = sid(&node.id).filter(|id| !reached.contains(id)) {
                errors.push(err(
                    OrphanNode,
                    "트리거에서 이어지지 않은 노드입니다.".into(),
                    Some(nid),
                    None,
                ));
            }
        }
    }
    errors
}

/// 분기 노드에서 나가는 엣지가 쓸 수 있는 (conditionType 값, 이름). 새 모양 = 갈래 key들 + otherwise가 stop이
/// 아니면 else. 레거시(branches 빈 값)는 otherwise를 무시하고 true·false.
pub fn condition_branches(node: &Node) -> Vec<(String, String)> {
    let branches = get_path(node, "config.branches");
    if is_empty(branches) {
        return vec![
            ("true".into(), "참".into()),
            ("false".into(), "그 외".into()),
        ];
    }
    let mut out: Vec<(String, String)> = branches
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|br| {
            let key = br.get("key")?.as_str().filter(|k| !k.is_empty())?;
            Some((key.to_string(), py_str_or_empty(br.get("name"))))
        })
        .collect();
    if get_path(node, "config.otherwise").and_then(Value::as_str) != Some("stop") {
        out.push(("else".into(), "그 외".into()));
    }
    out
}

struct Allowed {
    values: HashSet<String>,
    listed: String,
    stop: bool,
}

/// CONDITION 노드면 허용값·메시지용 허용값 표기·otherwise == stop, 아니면 None.
fn branch_allowed(node: &Node, catalog: &Catalog) -> Option<Allowed> {
    if catalog.get(&node.event)?.node_type != NodeType::Condition {
        return None;
    }
    let branches = condition_branches(node);
    let mut listed = branches
        .iter()
        .take(MAX_LISTED_BRANCHES)
        .map(|(v, n)| format!("'{}'({})", clip(v), clip(n)))
        .collect::<Vec<_>>()
        .join(", ");
    if branches.len() > MAX_LISTED_BRANCHES {
        listed += &format!(" 외 {}개", branches.len() - MAX_LISTED_BRANCHES);
    }
    if listed.is_empty() {
        listed = "없음".into();
    }
    let stop = is_branched(node)
        && get_path(node, "config.otherwise").and_then(Value::as_str) == Some("stop");
    Some(Allowed {
        values: branches.into_iter().map(|(v, _)| v).collect(),
        listed,
        stop,
    })
}

/// 분기 엣지의 conditionType — 허용값 밖이면 실행기가 절대 따라가지 않는 연결이다.
fn branch_edge_error(
    source: &str,
    target: &str,
    ct: Option<&str>,
    a: &Allowed,
) -> Option<ValidationError> {
    if ct.is_some_and(|c| a.values.contains(c)) {
        return None;
    }
    let place = format!("분기 '{}' → '{}' 연결", clip(source), clip(target));
    let msg = if a.stop && ct == Some("else") {
        format!(
            "{place}: 그 외는 '여기서 멈추기'라 'else' 연결은 실행되지 않아요. 쓸 수 있는 값: {}.",
            a.listed
        )
    } else {
        let got = ct.map_or("없음".to_string(), |c| format!("'{}'", clip(c)));
        format!(
            "{place}의 갈래 값({got})이 맞지 않아요. 쓸 수 있는 값: {}.",
            a.listed
        )
    };
    Some(err(EdgeInvalid, msg, Some(source), None))
}

// ── 칸 검사(REF_NOT_ALLOWED만) ─────────────────────────────

fn field_errors(node: &Node, catalog: &Catalog) -> Vec<ValidationError> {
    let Some(entry) = catalog.get(&node.event) else {
        return Vec::new();
    };
    let nid = sid(&node.id);
    let mut errors = Vec::new();
    for f in &entry.input_fields {
        let path = field_path(f);
        let mut value = get_path(node, &path);
        if entry.node_type == NodeType::Condition
            && is_empty(value)
            && let Some(alias) = condition_alias(&path)
        {
            value = get_path(node, alias);
        }
        if unused_field(entry, node, &path) {
            continue;
        }
        // 상한 밖 갈래·조건은 TOO_LARGE 몫이라 내용을 보지 않는다
        let truncated;
        if entry.node_type == NodeType::Condition
            && path == "config.branches"
            && let Some(list) = value.and_then(Value::as_array)
        {
            truncated = Value::Array(
                list.iter()
                    .take(MAX_BRANCHES)
                    .map(|b| {
                        let mut b = b.clone();
                        if let Some(rules) = b.get_mut("rules").and_then(Value::as_array_mut) {
                            rules.truncate(MAX_RULES);
                        }
                        b
                    })
                    .collect(),
            );
            value = Some(&truncated);
        }
        check(f, value, &path, nid, false, &mut errors);
    }
    errors
}

/// `no_ref` = 부모 목록 칸이 `ref: false`(자식 칸도 참조 금지).
fn check(
    f: &InputField,
    value: Option<&Value>,
    path: &str,
    nid: Option<&str>,
    no_ref: bool,
    errors: &mut Vec<ValidationError>,
) {
    let no_ref = no_ref || f.ref_allowed == Some(false);
    let Some(value) = value.filter(|v| !is_empty(Some(v))) else {
        return; // REQUIRED는 범위 밖
    };
    if f.list {
        // 목록이 아니면 TYPE — 범위 밖
        for (i, item) in value.as_array().into_iter().flatten().enumerate() {
            if item.is_object() {
                for child in &f.children {
                    check(
                        child,
                        item.get(&child.key),
                        &format!("{path}.{i}.{}", child.key),
                        nid,
                        no_ref,
                        errors,
                    );
                }
            }
        }
        return;
    }
    if value.as_str().is_some_and(|s| s.contains("{{")) && no_ref {
        let title = f
            .title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or(&f.key);
        errors.push(err(
            RefNotAllowed,
            format!("'{title}' 칸에는 앞 단계 값을 넣을 수 없어요."),
            nid,
            Some(path.to_string()),
        ));
    }
}

/// 새 모양 분기 — config.branches가 비어 있지 않다.
fn is_branched(node: &Node) -> bool {
    !is_empty(get_path(node, "config.branches"))
}

/// BE ConditionNodeExecutor는 left/right가 없으면 옛 표기 leftValue/rightValue를 읽는다.
fn condition_alias(path: &str) -> Option<&'static str> {
    match path {
        "config.left" => Some("config.leftValue"),
        "config.right" => Some("config.rightValue"),
        _ => None,
    }
}

/// 실행기가 읽지 않는 칸 — 칸 검사와 참조 검사가 함께 건너뛴다. (HTTP 분기는 1단계 노드에 없다)
fn unused_field(entry: &EventDef, node: &Node, path: &str) -> bool {
    if entry.node_type != NodeType::Condition {
        return false;
    }
    if LEGACY_CONDITION_PATHS.contains(&path) && is_branched(node) {
        return true;
    }
    path == "config.right"
        && get_path(node, "config.operator")
            .and_then(Value::as_str)
            .is_some_and(|op| RIGHT_UNUSED_OPERATORS.contains(&op))
}

// ── 참조식 ─────────────────────────────────────────────

fn predecessors<'a>(
    edges: &'a [Edge],
    by_id: &HashMap<&'a str, &Node>,
) -> HashMap<&'a str, Vec<&'a str>> {
    let mut preds: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in edges {
        if let (Some(s), Some(t)) = (sid(&edge.source), sid(&edge.target))
            && by_id.contains_key(s)
            && by_id.contains_key(t)
        {
            preds.entry(t).or_default().push(s);
        }
    }
    preds
}

fn ancestors<'a>(nid: &str, preds: &HashMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut stack: Vec<&str> = preds.get(nid).cloned().unwrap_or_default();
    while let Some(cur) = stack.pop() {
        if seen.insert(cur) {
            stack.extend(preds.get(cur).into_iter().flatten());
        }
    }
    seen
}

/// 엣지를 거슬러 닿는 노드 id(자기 제외).
pub fn upstream_ids(wf: &Workflow, id: &str) -> HashSet<String> {
    let by_id: HashMap<&str, &Node> = wf
        .nodes
        .iter()
        .filter_map(|n| sid(&n.id).map(|id| (id, n)))
        .collect();
    let mut up: HashSet<String> = ancestors(id, &predecessors(&wf.edges, &by_id))
        .into_iter()
        .map(String::from)
        .collect();
    up.remove(id); // 순환이면 자기 자신도 닿는다
    up
}

/// 대상 노드가 내는 최상위 출력 키. 알 수 없으면(카탈로그에 없음·outputDynamic) None = 검사 안 함.
pub fn declared_outputs(node: &Node, catalog: &Catalog) -> Option<BTreeSet<String>> {
    let entry = catalog.get(&node.event).filter(|e| !e.output_dynamic)?;
    if let Some(source) = entry.outputs_from.as_deref().filter(|s| !s.is_empty()) {
        match get_path(node, source) {
            Some(Value::Object(m)) if !m.is_empty() => return Some(m.keys().cloned().collect()),
            // 출력 칸 목록이 정적 출력 전체를 대체한다. 깨진 key는 버린다
            Some(Value::Array(l)) if !l.is_empty() => {
                return Some(
                    l.iter()
                        .filter_map(|f| f.get("key")?.as_str().map(String::from))
                        .collect(),
                );
            }
            _ => {}
        }
    }
    Some(entry.output_keys.clone().unwrap_or_default())
}

/// 참조식 에러를 out에 넣는다. false = 상한에 닿았다.
fn ref_errors<'a>(
    node: &Node,
    catalog: &Catalog,
    by_id: &HashMap<&'a str, &'a Node>,
    preds: &HashMap<&'a str, Vec<&'a str>>,
    outputs: &mut HashMap<String, Option<BTreeSet<String>>>,
    out: &mut Collector,
) -> bool {
    let Some(nid) = sid(&node.id) else {
        return true;
    };
    let entry = catalog.get(&node.event);
    let fields: Vec<String> = entry
        .map(|e| e.input_fields.iter().map(field_path).collect())
        .unwrap_or_default();
    // ACTION 동적 칸(fieldsSource) 안은 <칸 경로>.<키>까지 짚는다
    let dynamic: HashSet<String> = entry
        .filter(|e| e.node_type == NodeType::Action)
        .map(|e| {
            e.input_fields
                .iter()
                .filter(|f| f.fields_source.as_deref().is_some_and(|s| !s.is_empty()))
                .map(field_path)
                .collect()
        })
        .unwrap_or_default();
    let mut unused: Vec<String> = entry
        .map(|e| {
            fields
                .iter()
                .filter(|p| unused_field(e, node, p))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let aliases: Vec<String> = unused
        .iter()
        .filter_map(|p| condition_alias(p))
        .map(String::from)
        .collect();
    unused.extend(aliases);

    let mut anc: Option<HashSet<&str>> = None;
    // (위치 경로, 값)을 전위 순서로 — 명시 스택
    let root: Vec<(String, &Value)> = node
        .config
        .iter()
        .rev()
        .map(|(k, v)| (cut_loc(format!("config.{k}")), v))
        .collect();
    let mut stack = root;
    while let Some((loc, cur)) = stack.pop() {
        let text = match cur {
            Value::String(s) => s,
            Value::Object(m) => {
                stack.extend(
                    m.iter()
                        .rev()
                        .map(|(k, v)| (cut_loc(format!("{loc}.{k}")), v)),
                );
                continue;
            }
            Value::Array(l) => {
                stack.extend(
                    l.iter()
                        .enumerate()
                        .rev()
                        .map(|(i, v)| (cut_loc(format!("{loc}.{i}")), v)),
                );
                continue;
            }
            _ => continue,
        };
        if !text.contains("{{") || unused.iter().any(|p| under(&loc, p)) {
            continue;
        }
        let mut field = fields.iter().find(|p| under(&loc, p)).cloned();
        if let Some(f) = field
            .as_deref()
            .filter(|f| dynamic.contains(*f) && loc != *f)
        {
            let key = loc[f.len() + 1..].split('.').next().unwrap_or_default();
            field = Some(format!("{f}.{key}"));
        }
        let pos = |p: Option<usize>| p.map_or(-1, |i| i as isize);
        if pos(text.rfind("{{")) > pos(text.rfind("}}"))
            && !out.push(err(
                RefFormat,
                "참조식의 '{{'가 닫히지 않았어요.".into(),
                Some(nid),
                field.clone(),
            ))
        {
            return false;
        }
        for m in REF.captures_iter(text) {
            // `{{본문}}` 안에는 중괄호가 없으니 VARIABLE이 맞으면 곧 전체 일치다
            let Some(c) = VARIABLE.captures(&m[0]) else {
                let msg = format!(
                    "참조식 '{{{{{}}}}}'의 형식이 맞지 않아요. '{{{{nodes.<노드ID>.output.<칸>}}}}' 모양이어야 해요.",
                    clip(&m[1])
                );
                if !out.push(err(RefFormat, msg, Some(nid), field.clone())) {
                    return false;
                }
                continue;
            };
            let (target, out_path) = (&c[1], &c[2]);
            let e = if !by_id.contains_key(target) {
                Some(err(
                    RefUnknownNode,
                    format!("없는 노드 '{}'를 참조해요.", clip(target)),
                    Some(nid),
                    field.clone(),
                ))
            } else if target == nid {
                Some(err(
                    RefSelf,
                    "자기 자신의 결과는 참조할 수 없어요.".into(),
                    Some(nid),
                    field.clone(),
                ))
            } else if !anc
                .get_or_insert_with(|| ancestors(nid, preds))
                .contains(target)
            {
                Some(err(
                    RefNotUpstream,
                    format!("앞 단계가 아닌 노드 '{}'를 참조해요.", clip(target)),
                    Some(nid),
                    field.clone(),
                ))
            } else {
                let declared = outputs
                    .entry(target.to_string())
                    .or_insert_with(|| declared_outputs(by_id[target], catalog));
                let head = out_path.split('.').next().unwrap_or_default();
                declared.as_ref().filter(|d| !d.contains(head)).map(|_| {
                    err(
                        RefUnknownField,
                        format!(
                            "노드 '{}'에는 '{}' 결과가 없어요.",
                            clip(target),
                            clip(head)
                        ),
                        Some(nid),
                        field.clone(),
                    )
                })
            };
            if let Some(e) = e
                && !out.push(e)
            {
                return false;
            }
        }
    }
    true
}

/// 위치 경로 상한 — 긴 키는 잘린다.
fn cut_loc(s: String) -> String {
    match s.char_indices().nth(MAX_LOC) {
        Some((i, _)) => s[..i].to_string(),
        None => s,
    }
}

// ── 필수 칸·선택지(채팅 빌더용) ─────────────────────────────

/// 필수 칸 채움·선택지 값. `validate`와 따로 부른다(`validate`·`run_workflow`의 동작은 그대로).
/// 노드 순서 → 칸은 카탈로그 순서 → 칸마다 REQUIRED 다음 CHOICE. 카탈로그에 없는 event는 건너뛴다.
pub fn validate_fields(wf: &Workflow, catalog: &Catalog) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for node in &wf.nodes {
        let Some(entry) = catalog.get(&node.event) else {
            continue;
        };
        for f in &entry.input_fields {
            let path = field_path(f);
            if unused_field(entry, node, &path) {
                continue;
            }
            let mut value = get_path(node, &path);
            if entry.node_type == NodeType::Condition
                && is_empty(value)
                && let Some(alias) = condition_alias(&path)
            {
                value = get_path(node, alias);
            }
            check_required_choice(f, value, &path, sid(&node.id), &mut errors);
        }
    }
    errors
}

fn check_required_choice(
    f: &InputField,
    value: Option<&Value>,
    path: &str,
    nid: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let title = f
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .unwrap_or(&f.key);
    let Some(value) = value.filter(|v| !is_empty(Some(v))) else {
        if f.required {
            errors.push(err(
                Required,
                format!("'{title}' 칸이 비어 있어요."),
                nid,
                Some(path.to_string()),
            ));
        }
        return;
    };
    if f.list {
        for (i, item) in value.as_array().into_iter().flatten().enumerate() {
            if item.is_object() {
                for child in &f.children {
                    let path = format!("{path}.{i}.{}", child.key);
                    check_required_choice(child, item.get(&child.key), &path, nid, errors);
                }
            }
        }
        return;
    }
    if !f.choices.is_empty()
        && let Some(s) = value.as_str()
        && !s.contains("{{")
        && !f.choices.iter().any(|c| c.id == s)
    {
        errors.push(err(
            Choice,
            format!("'{title}' 칸의 값이 선택지에 없어요."),
            nid,
            Some(path.to_string()),
        ));
    }
}
