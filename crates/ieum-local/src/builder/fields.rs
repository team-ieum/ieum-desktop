//! fields·set — 칸 정의 보여 주기와 칸 채우기(잠긴 칸·값 맞추기·갈래 key).

use std::path::{Component, Path};

use ieum_engine::{
    Catalog, InputField, ValidationError, Workflow, declared_outputs, upstream_ids, validate,
    validate_fields,
};
use serde_json::{Map, Value, json};

use super::Code;
use super::edit::{AI_EVENT, AI_LOCKED_FIELDS, find, find_mut, is_folder_field};

/// 정적 검증 + 필수 칸·선택지. Builder는 늘 둘을 합쳐 쓴다.
pub(crate) fn all_errors(wf: &Workflow, catalog: &Catalog) -> Vec<ValidationError> {
    let mut errors = validate(wf, catalog);
    errors.extend(validate_fields(wf, catalog));
    errors
}

/// `set`으로 못 채우는 칸 — 폴더 칸은 사람이 고르고, AI 연결 칸은 앱이 채운다.
fn locked(event: &str, key: &str) -> bool {
    is_folder_field(event, key) || (event == AI_EVENT && AI_LOCKED_FIELDS.contains(&key))
}

fn field_json(event: &str, f: &InputField, path: String, value: Option<&Value>) -> Value {
    let children: Vec<Value> = f
        .children
        .iter()
        .map(|c| field_json("", c, format!("{path}.<n>.{}", c.key), None))
        .collect();
    json!({
        "key": f.key,
        "path": path,
        "title": f.title,
        "type": f.kind,
        "required": f.required,
        "ref": f.ref_allowed != Some(false),
        "locked": locked(event, &f.key),
        "choices": f.choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        "value": value,
        "children": children,
    })
}

pub(super) fn fields(catalog: &Catalog, wf: &Workflow, id: &str) -> Result<Value, Code> {
    let node = find(wf, id)?;
    let defs = catalog.get(&node.event).map(|d| d.input_fields.as_slice());
    let fields: Vec<Value> = defs
        .unwrap_or_default()
        .iter()
        .map(|f| {
            let path = f
                .path
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| format!("config.{}", f.key));
            field_json(&node.event, f, path, node.config.get(&f.key))
        })
        .collect();
    let up = upstream_ids(wf, id);
    let upstream: Vec<Value> = wf
        .nodes
        .iter()
        .filter(|n| up.contains(&n.id))
        .map(|n| {
            json!({
                "id": n.id,
                "label": n.label,
                "event": n.event,
                "outputs": declared_outputs(n, catalog).unwrap_or_default(),
            })
        })
        .collect();
    Ok(json!({"fields": fields, "upstream": upstream}))
}

/// CLI는 값을 글자로만 준다 — 불리언·목록 칸은 칸 타입에 맞춘다.
fn coerce(f: &InputField, value: Value) -> Result<Value, Code> {
    if f.list {
        return match value {
            Value::Array(_) | Value::Null => Ok(value),
            Value::String(s) => match serde_json::from_str(&s) {
                Ok(list @ Value::Array(_)) => Ok(list),
                _ => Err(Code::FieldInvalid),
            },
            _ => Err(Code::FieldInvalid),
        };
    }
    if f.kind.as_deref() == Some("boolean") {
        return match value {
            Value::Bool(_) | Value::Null => Ok(value),
            Value::String(s) if s == "true" => Ok(Value::Bool(true)),
            Value::String(s) if s == "false" => Ok(Value::Bool(false)),
            _ => Err(Code::FieldInvalid),
        };
    }
    Ok(value)
}

/// 실행 때 `resolve_in_base`가 거부하는 목적지(절대 경로·`..`)를 미리 거부한다.
fn destination_ok(value: &Value) -> bool {
    let Some(s) = value.as_str().filter(|s| !s.contains("{{")) else {
        return true;
    };
    let path = Path::new(s);
    !path.is_absolute() && !path.components().any(|c| c == Component::ParentDir)
}

/// 빈 갈래 key를 채우고(프런트 `newBranchKey`와 같은 모양) `[{key, name}]`을 돌려준다.
fn fill_branch_keys(value: &mut Value) -> Value {
    let list = value
        .as_array_mut()
        .map(Vec::as_mut_slice)
        .unwrap_or_default();
    for item in list.iter_mut() {
        let Some(branch) = item.as_object_mut() else {
            continue;
        };
        let has_key = branch
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(|k| !k.is_empty());
        if !has_key {
            let hex = uuid::Uuid::new_v4().simple().to_string();
            branch.insert("key".into(), format!("b_{}", &hex[..8]).into());
        }
    }
    list.iter()
        .map(|b| json!({"key": b.get("key"), "name": b.get("name")}))
        .collect()
}

/// 키 하나라도 거부되면 아무것도 고치지 않는다. `branch.if`의 `branches`를 채웠으면 `[{key, name}]`.
pub(super) fn set(
    catalog: &Catalog,
    wf: &mut Workflow,
    id: &str,
    values: Map<String, Value>,
) -> Result<Option<Value>, Code> {
    let node = find_mut(wf, id)?;
    let defs = catalog.get(&node.event).map(|d| d.input_fields.as_slice());
    let mut branches = None;
    let mut accepted = Vec::new();
    for (key, value) in values {
        let key = key.strip_prefix("config.").unwrap_or(&key).to_string();
        let f = defs
            .unwrap_or_default()
            .iter()
            .find(|f| f.key == key)
            .ok_or(Code::FieldUnknown)?;
        if locked(&node.event, &key) {
            return Err(Code::FieldLocked);
        }
        let mut value = coerce(f, value)?;
        match (node.event.as_str(), key.as_str()) {
            ("local.file_move", "destination") if !destination_ok(&value) => {
                return Err(Code::FolderNotAllowed);
            }
            ("branch.if", "branches") => branches = Some(fill_branch_keys(&mut value)),
            _ => {}
        }
        accepted.push((key, value));
    }
    for (key, value) in accepted {
        if value.is_null() {
            node.config.remove(&key);
        } else {
            node.config.insert(key, value);
        }
    }
    Ok(branches)
}
