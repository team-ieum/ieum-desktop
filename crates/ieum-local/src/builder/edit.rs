//! 편집 동작 — 저장본 사본(`Workflow`)만 고친다. 읽기·저장·잠금은 `core.rs`가 한다.

use ieum_engine::{
    Catalog, Edge, EventDef, Node, NodeType, ValidationError, Workflow, condition_branches,
    upstream_ids,
};
use serde_json::{Map, Value, json};

use super::{AiDefault, ChatState, Code, Doc, node_state, workflow_hash};

/// 사람이 고르는 폴더 칸 (event, 칸 key) — `paths.rs`의 `user_folders`와 같은 쌍.
pub const FOLDER_FIELDS: [(&str, &str); 2] = [
    ("local.folder_new_file", "folder"),
    ("local.file_move", "baseFolder"),
];
/// `local.ai`에서 앱만 채우는 칸.
pub const AI_LOCKED_FIELDS: [&str; 3] = ["provider", "credential", "model"];

pub(super) const AI_EVENT: &str = "local.ai";

pub(super) fn is_folder_field(event: &str, key: &str) -> bool {
    FOLDER_FIELDS.contains(&(event, key))
}

fn type_name(t: NodeType) -> &'static str {
    match t {
        NodeType::Trigger => "TRIGGER",
        NodeType::Ai => "AI",
        NodeType::Action => "ACTION",
        NodeType::Condition => "CONDITION",
    }
}

fn is_trigger(node: &Node, catalog: &Catalog) -> bool {
    catalog
        .get(&node.event)
        .is_some_and(|d| d.node_type == NodeType::Trigger)
        || node
            .r#type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("TRIGGER"))
}

pub(super) fn find<'a>(wf: &'a Workflow, id: &str) -> Result<&'a Node, Code> {
    wf.nodes
        .iter()
        .find(|n| n.id == id)
        .ok_or(Code::NodeNotFound)
}

pub(super) fn find_mut<'a>(wf: &'a mut Workflow, id: &str) -> Result<&'a mut Node, Code> {
    wf.nodes
        .iter_mut()
        .find(|n| n.id == id)
        .ok_or(Code::NodeNotFound)
}

/// 분기에서 나가는 연결은 아는 갈래 값이 있어야 하고, 그 밖의 노드에서는 갈래가 없어야 한다.
fn check_branch(catalog: &Catalog, from: &Node, branch: Option<&str>) -> Result<(), Code> {
    let condition = catalog
        .get(&from.event)
        .is_some_and(|d| d.node_type == NodeType::Condition);
    let ok = match (condition, branch) {
        (true, Some(b)) => condition_branches(from).iter().any(|(key, _)| key == b),
        (false, None) => true,
        _ => false,
    };
    if ok { Ok(()) } else { Err(Code::EdgeInvalid) }
}

fn edge(source: &str, target: &str, branch: Option<String>) -> Edge {
    Edge {
        id: Some(uuid::Uuid::new_v4().to_string()),
        source: source.to_string(),
        target: target.to_string(),
        condition_type: branch,
        extra: Map::new(),
    }
}

pub(super) fn status(doc: &Doc, state: &ChatState, errors: Vec<ValidationError>) -> Value {
    let nodes: Vec<Value> = doc
        .workflow
        .nodes
        .iter()
        .map(|n| json!({"id": n.id, "event": n.event, "label": n.label, "state": node_state(n, state)}))
        .collect();
    let edges: Vec<Value> = doc
        .workflow
        .edges
        .iter()
        .map(|e| {
            let mut m = Map::new();
            m.insert("from".into(), e.source.clone().into());
            m.insert("to".into(), e.target.clone().into());
            if let Some(b) = &e.condition_type {
                m.insert("branch".into(), b.clone().into());
            }
            Value::Object(m)
        })
        .collect();
    let pending = state
        .pending_finish
        .as_ref()
        .is_some_and(|p| p.hash == workflow_hash(&doc.workflow));
    json!({
        "name": doc.name,
        "enabled": doc.enabled,
        "outline": state.outline,
        "nodes": nodes,
        "edges": edges,
        "errors": errors,
        "pendingFinish": pending,
    })
}

pub(super) fn events(catalog: &Catalog) -> Value {
    let list: Vec<Value> = catalog
        .events()
        .into_iter()
        .map(|e| {
            json!({
                "event": e.event,
                "title": e.title,
                "description": e.description,
                "nodeType": type_name(e.node_type),
            })
        })
        .collect();
    json!({"events": list})
}

/// 칸 `default`가 있는 것만 채운 새 노드 config.
fn default_config(def: &EventDef) -> Map<String, Value> {
    def.input_fields
        .iter()
        .filter_map(|f| Some((f.key.clone(), f.default.clone()?)))
        .collect()
}

/// `ai` = `local.ai` 노드에 채울 연결(없으면 비워 둔다).
pub(super) fn add(
    catalog: &Catalog,
    wf: &mut Workflow,
    event: &str,
    label: &str,
    after: Option<&str>,
    branch: Option<String>,
    ai: Option<AiDefault>,
) -> Result<Value, Code> {
    let def = catalog.get(event).ok_or(Code::EventUnknown)?;
    // 첫 노드는 트리거뿐이고, 트리거는 하나뿐이다.
    if wf.nodes.is_empty() != (def.node_type == NodeType::Trigger) {
        return Err(Code::TriggerRule);
    }
    let after = match after {
        Some(id) => Some(find(wf, id)?),
        None => wf.nodes.last(),
    };
    match after {
        Some(from) => check_branch(catalog, from, branch.as_deref())?,
        None if branch.is_some() => return Err(Code::EdgeInvalid),
        None => {}
    }
    let id = uuid::Uuid::new_v4().to_string();
    let new_edge = after.map(|from| edge(&from.id, &id, branch));

    let mut config = default_config(def);
    let mut needs_ai = false;
    if event == AI_EVENT {
        let ai = ai.unwrap_or(AiDefault {
            provider: String::new(),
            credential: None,
            model: None,
        });
        let model = ai.model.filter(|m| !m.is_empty());
        // OpenAI는 기본 모델이 없다 — 모델이 비면 실행이 실패한다.
        needs_ai = ai.provider.is_empty() || (ai.provider == "api:openai" && model.is_none());
        if !ai.provider.is_empty() {
            config.insert("provider".into(), ai.provider.into());
        }
        if let Some(c) = ai.credential.filter(|c| !c.is_empty()) {
            config.insert("credential".into(), c.into());
        }
        if let Some(m) = model {
            config.insert("model".into(), m.into());
        }
    }
    wf.nodes.push(Node {
        id: id.clone(),
        event: event.to_string(),
        r#type: Some(type_name(def.node_type).to_string()),
        label: Some(label.to_string()),
        description: None,
        position: None, // 자리는 편집기가 잡는다
        config,
        extra: Map::new(),
    });
    wf.edges.extend(new_edge);
    Ok(json!({"id": id, "needsAi": needs_ai}))
}

/// Ok(false) = 이미 있는 연결이라 고친 것이 없다.
pub(super) fn connect(
    catalog: &Catalog,
    wf: &mut Workflow,
    from: &str,
    to: &str,
    branch: Option<String>,
) -> Result<bool, Code> {
    let source = find(wf, from)?;
    let target = find(wf, to)?;
    if from == to || is_trigger(target, catalog) || upstream_ids(wf, from).contains(to) {
        return Err(Code::EdgeInvalid);
    }
    check_branch(catalog, source, branch.as_deref())?;
    let exists = wf
        .edges
        .iter()
        .any(|e| e.source == from && e.target == to && e.condition_type == branch);
    if !exists {
        wf.edges.push(edge(from, to, branch));
    }
    Ok(!exists)
}

pub(super) fn disconnect(wf: &mut Workflow, from: &str, to: &str) -> Result<(), Code> {
    let before = wf.edges.len();
    wf.edges.retain(|e| !(e.source == from && e.target == to));
    if wf.edges.len() == before {
        return Err(Code::EdgeInvalid);
    }
    Ok(())
}

pub(super) fn remove(catalog: &Catalog, wf: &mut Workflow, id: &str) -> Result<(), Code> {
    let node = find(wf, id)?;
    if is_trigger(node, catalog) && wf.nodes.len() > 1 {
        return Err(Code::TriggerRule);
    }
    wf.nodes.retain(|n| n.id != id);
    wf.edges.retain(|e| e.source != id && e.target != id);
    Ok(())
}

pub(super) fn rename(wf: &mut Workflow, id: &str, label: &str) -> Result<(), Code> {
    let label = label.trim();
    if label.is_empty() {
        return Err(Code::BadRequest);
    }
    find_mut(wf, id)?.label = Some(label.to_string());
    Ok(())
}

/// 폴더 고르기 창 제목 — 무엇에 쓸 폴더인지 사람이 창에서 본다.
// ponytail: 한국어 고정(i18n 미적용) — 앱 언어 설정을 Host로 받게 되면 그때 나눈다
pub(super) fn pick_title(catalog: &Catalog, node: &Node, field: &str) -> String {
    let def = catalog.get(&node.event);
    let name = node
        .label
        .as_deref()
        .filter(|l| !l.is_empty())
        .or(def.map(|d| d.title.as_str()))
        .unwrap_or(&node.event);
    let field_title = def
        .and_then(|d| d.input_fields.iter().find(|f| f.key == field))
        .and_then(|f| f.title.as_deref())
        .unwrap_or(field);
    format!("IEUM — '{name}'의 {field_title} 고르기")
}
