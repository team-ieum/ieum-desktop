//! 채팅 설계서 — A2UI v1.0 표면 하나. LLM이 `ui` 동작으로 낸 메시지를 허용 목록으로 검증하고
//! 바인딩을 v1.0 `{"@path"}`로 바꿔 `ChatState.blueprint`에 둔다. 상태의 주인은 Rust다: 화면은 이
//! 스냅샷을 통째로 다시 그리고, 저장 파일은 믿지 않고 `validate_stored`로 다시 본다.
//! 슬롯(앱 전용 폴더 칸) 경로는 화면에만 간다 — 요약·에러·`at`에 싣지 않는다.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::core::lock_of;
use super::edit;
use super::{Builder, ChatState, Code, Doc, FOLDER_FIELDS, Reply};

pub const CATALOG_ID: &str = "ieum.chat.blueprint/v1";
pub const SURFACE_ID: &str = "blueprint";

const MAX_COMPONENTS: usize = 20;
const MAX_CHILDREN: usize = 12;
const MAX_STEPS: usize = 8;
const MAX_DATA_BYTES: usize = 4096;
const MAX_TEXT_VALUE: usize = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlueprintStatus {
    #[default]
    Active,
    Submitted,
    Cancelled,
}

/// `AppGate` 하나의 앱 쪽 상태. 경로는 target에 묶인다 — target이 바뀌면 새 슬롯이 된다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub event: String,
    pub field: String,
    pub path: Option<String>,
    pub picked_version: Option<u32>,
    pub bound_node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    pub status: BlueprintStatus,
    /// `ui` 동작이 성공할 때마다 +1. 제출·슬롯 고르기가 사람이 본 판과 같은지 이것으로 본다.
    pub version: u32,
    /// 검증·v1.0 변환을 마친 평평한 컴포넌트들.
    pub components: Vec<Value>,
    pub data_model: Value,
    pub slots: BTreeMap<String, Slot>,
    #[serde(default)]
    pub build_pending: bool,
    #[serde(default)]
    pub build_attempts: u32,
}

/// `ui` 실패 rule 이름 전부 — 실측 기록은 이 목록 안의 것만 쓴다.
/// `unknown_prop`은 표에 없는 prop뿐 아니라 모양이 다르거나 빠진 prop도 뜻한다.
pub const UI_RULES: &[&str] = &[
    "message_kind",
    "surface_id",
    "catalog_id",
    "no_surface",
    "component_type",
    "unknown_prop",
    "limit",
    "root",
    "submit_count",
    "slot_id",
    "slot_target",
    "slot_duplicate_target",
    "binding",
    "data_model",
    "data_slot_path",
];

/// 위반 위치(`at`)는 컴포넌트 id·prop 이름이나 메시지 순번만 — 경로·값은 넣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{rule}")]
pub struct UiError {
    pub rule: &'static str,
    pub at: Option<String>,
}

impl UiError {
    pub fn reply(&self) -> Reply {
        match self.rule {
            "no_surface" => Reply::err(Code::UiNoSurface),
            _ => Reply::err_with(Code::UiInvalid, json!({"rule": self.rule, "at": self.at})),
        }
    }
}

fn fail(rule: &'static str, at: Option<String>) -> UiError {
    UiError { rule, at }
}

#[derive(Clone, Copy)]
enum Prop {
    /// 글자 — 다듬고 이 글자 수에서 자른다.
    Text(usize),
    Bool,
    /// LLM은 `{"path"}`, 저장본은 `{"@path"}`.
    Binding,
    Children,
    Steps,
    /// 선택지 개수 범위.
    Options(usize, usize),
    Slot,
    Target,
}
use Prop::*;

/// 컴포넌트별 prop 허용 표 — 8종과 한도는 설계 1절 표 그대로. 입력·저장본 검사가 함께 쓰고 바인딩 표기만 다르게 본다.
const COMPONENTS: &[(&str, &[(&str, Prop)])] = &[
    ("Blueprint", &[("title", Text(40)), ("children", Children)]),
    ("FlowPreview", &[("steps", Steps)]),
    (
        "ChipSelect",
        &[
            ("label", Text(30)),
            ("options", Options(2, 8)),
            ("multiple", Bool),
            ("value", Binding),
        ],
    ),
    (
        "Select",
        &[
            ("label", Text(30)),
            ("options", Options(2, 30)),
            ("value", Binding),
        ],
    ),
    ("Checkbox", &[("label", Text(40)), ("value", Binding)]),
    (
        "TextInput",
        &[("label", Text(30)), ("value", Binding), ("hint", Text(80))],
    ),
    ("AppGate", &[("slot", Slot), ("target", Target)]),
    ("SubmitButton", &[]),
];
/// 빠져도 되는 prop. 나머지는 빠지면 그 prop의 위반이다(렌더러 스키마가 깨지지 않게).
const OPTIONAL: &[&str] = &["multiple", "hint"];

#[derive(Clone, Copy, PartialEq)]
enum Dialect {
    Llm,
    Stored,
}

/// AppGate 하나 = (slot, event, field).
type Gate = (String, String, String);

/// LLM의 `ui` 메시지 묶음을 적용한 새 설계서. 실패하면 아무것도 바꾸지 않는다(호출자가 저장하지 않음).
pub fn apply_ui(current: Option<&Blueprint>, messages: &[Value]) -> Result<Blueprint, UiError> {
    if messages.is_empty() {
        return Err(fail("message_kind", None));
    }
    // 이어서 고칠 때는 저장본을 LLM 표기로 되돌려 들어온 것과 같은 표기로 다룬다.
    let mut work = current
        .filter(|b| b.status == BlueprintStatus::Active)
        .cloned()
        .map(|mut b| {
            b.components.iter_mut().for_each(to_llm);
            b
        });
    for (i, m) in messages.iter().enumerate() {
        let at = || Some(format!("messages[{i}]"));
        // A2UI v1.0 문서가 가르치는 봉투 키 — 모델이 습관적으로 붙인다. 값이 "1.0"일 때만 무시한다.
        let mut keys = m
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(k, v)| !(*k == "version" && *v == "1.0"));
        let (kind, body) = match (keys.next(), keys.next()) {
            (Some(entry), None) => entry,
            _ => return Err(fail("message_kind", at())),
        };
        let allowed: &[&str] = match kind.as_str() {
            "createSurface" => &["surfaceId", "catalogId", "components", "dataModel"],
            "updateComponents" => &["surfaceId", "components"],
            "updateDataModel" => &["surfaceId", "path", "value"],
            _ => return Err(fail("message_kind", at())),
        };
        let body = body
            .as_object()
            .filter(|b| b.keys().all(|k| allowed.contains(&k.as_str())))
            .ok_or_else(|| fail("message_kind", at()))?;
        if body.get("surfaceId").and_then(Value::as_str) != Some(SURFACE_ID) {
            return Err(fail("surface_id", at()));
        }
        if kind == "createSurface" {
            if body.get("catalogId").and_then(Value::as_str) != Some(CATALOG_ID) {
                return Err(fail("catalog_id", at()));
            }
            // 새 설계서는 슬롯도 새로 시작한다 — 옛 경로가 넘어오지 않는다.
            work = Some(Blueprint {
                status: BlueprintStatus::Active,
                version: 0,
                components: Vec::new(),
                data_model: body.get("dataModel").cloned().unwrap_or_else(|| json!({})),
                slots: BTreeMap::new(),
                build_pending: false,
                build_attempts: 0,
            });
        }
        // components 없는 updateComponents는 바꾸는 것 없이 판만 올린다. createSurface는 components 없이 열고
        // 같은 묶음의 updateComponents로 채울 수 있다(A2UI 순서) — 비면 root 검사에서 걸린다.
        if kind == "updateComponents" && !body.contains_key("components") {
            return Err(fail("message_kind", at()));
        }
        let b = work.as_mut().ok_or_else(|| fail("no_surface", at()))?;
        if kind == "updateDataModel" {
            set_data(&mut b.data_model, body.get("path"), body.get("value"))
                .ok_or_else(|| fail("data_model", at()))?;
        } else if let Some(list) = body.get("components") {
            let list = list.as_array().ok_or_else(|| fail("message_kind", at()))?;
            for c in list {
                let id = c
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| fail("component_type", at()))?;
                match b.components.iter_mut().find(|x| x["id"] == id) {
                    Some(x) => *x = c.clone(),
                    None => b.components.push(c.clone()),
                }
            }
        }
    }
    let mut b = work.ok_or_else(|| fail("no_surface", None))?;
    let gates = normalize(&mut b.components, Dialect::Llm)?;
    b.slots = gates
        .into_iter()
        .map(|(slot, event, field)| {
            let kept = b
                .slots
                .get(&slot)
                .filter(|s| s.event == event && s.field == field)
                .cloned();
            let s = kept.unwrap_or_else(|| Slot {
                event,
                field,
                ..Slot::default()
            });
            (slot, s)
        })
        .collect();
    for c in b
        .components
        .iter()
        .filter(|c| c["component"] == "TextInput")
    {
        let bound = c["value"]["@path"]
            .as_str()
            .and_then(|p| b.data_model.pointer_mut(p));
        if let Some(Value::String(s)) = bound {
            *s = clean(s, MAX_TEXT_VALUE);
        }
    }
    b.version = current.map_or(0, |c| c.version).saturating_add(1);
    check_data(&b, &b.data_model)?;
    Ok(b)
}

/// 저장 파일에서 읽은 설계서를 믿어도 되는가 — 입력과 같은 표로 보되 v1.0 표기만 받고,
/// 다듬을 것·고아·슬롯 어긋남·경로 섞인 데이터가 하나라도 있으면 거짓.
pub fn validate_stored(b: &Blueprint) -> bool {
    let mut comps = b.components.clone();
    let Ok(gates) = normalize(&mut comps, Dialect::Stored) else {
        return false;
    };
    comps == b.components
        && gates.len() == b.slots.len()
        && gates.iter().all(|(slot, event, field)| {
            b.slots
                .get(slot)
                .is_some_and(|s| s.event == *event && s.field == *field)
        })
        && check_data(b, &b.data_model).is_ok()
}

/// 화면이 보낸(또는 LLM이 쓴) 데이터 모델 검사. 경로를 쥔 Rust가 직접 본다 — 슬롯 경로가 섞이면
/// 프롬프트로 새므로 화면 구현에만 맡기지 않는다.
pub fn check_data(b: &Blueprint, data: &Value) -> Result<(), UiError> {
    let size = serde_json::to_vec(data).map_or(usize::MAX, |v| v.len());
    if !data.is_object() || size > MAX_DATA_BYTES {
        return Err(fail("data_model", None));
    }
    let paths: Vec<&str> = b
        .slots
        .values()
        .filter_map(|s| s.path.as_deref())
        .filter(|p| !p.is_empty())
        .collect();
    if any_text(data, &|s: &str| paths.iter().any(|p| s.contains(p))) {
        return Err(fail("data_slot_path", None));
    }
    Ok(())
}

/// 프롬프트 "## 설계서" 구역 본문. 바인딩은 LLM 표기로 되돌리고, 슬롯은 고름·노드에 넣음 여부만 싣는다.
pub fn prompt_summary(b: &Blueprint) -> String {
    let status = serde_json::to_value(b.status).unwrap_or_default();
    let mut out = format!(
        "상태: {}\n버전: {}\n컴포넌트:\n",
        status.as_str().unwrap_or_default(),
        b.version
    );
    for c in &b.components {
        let mut c = c.clone();
        to_llm(&mut c);
        let Some(o) = c.as_object_mut() else { continue };
        let id = o.shift_remove("id").unwrap_or_default();
        let ty = o.shift_remove("component").unwrap_or_default();
        out += &format!(
            "- {} {} {}\n",
            id.as_str().unwrap_or_default(),
            ty.as_str().unwrap_or_default(),
            Value::Object(o.clone())
        );
    }
    out += &format!("데이터: {}\n앱 전용 칸:\n", b.data_model);
    for (slot, s) in &b.slots {
        let picked = match (&s.path, &s.bound_node) {
            (Some(_), _) => "고름".to_string(),
            (None, Some(n)) => format!("노드에 넣음({n})"),
            (None, None) => "안 고름".to_string(),
        };
        out += &format!("- {slot} {}.{} {picked}\n", s.event, s.field);
    }
    out
}

/// 완료 확인이 끝나거나 설계서를 거치지 않은 변경으로 취소될 때 — 남은 경로를 들고 있지 않는다.
pub fn clear_slot_paths(b: &mut Blueprint) {
    for s in b.slots.values_mut() {
        s.path = None;
    }
}

impl Builder {
    /// 사람의 [이대로 만들기]. 사람이 본 판(`version`)이 지금 판이고 앱 전용 칸을 다 골랐어야 한다.
    pub async fn submit_blueprint(&self, workflow_id: &str, version: u32, data: Value) -> Reply {
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        match self.submit(workflow_id, version, data) {
            Ok(()) => Reply::ok(()),
            Err(reply) => reply,
        }
    }

    /// 앱 전용 칸 고르기. 성공 data `{"path"}`는 화면 전용이다 — 채팅 루프 결과로 보내지 않는다.
    pub async fn pick_slot(&self, workflow_id: &str, slot: &str, version: u32) -> Reply {
        // 창이 열린 동안(최대 5분)도 쥔다 — pick-folder와 같다.
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        match self.pick_slot_path(workflow_id, slot, version).await {
            Ok(path) => Reply::ok(json!({"path": path})),
            Err(reply) => reply,
        }
    }

    /// 화면의 입력값을 따라 적는다. 떠 있는(Active) 설계서가 아니면 아무것도 하지 않는다.
    pub async fn sync_blueprint_data(&self, workflow_id: &str, data: Value) -> Reply {
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        match self.sync_data(workflow_id, data) {
            Ok(()) => Reply::ok(()),
            Err(reply) => reply,
        }
    }

    /// 믿을 수 있는 저장본 설계서. `validate_stored`를 못 넘으면 없는 것으로 본다.
    fn stored_blueprint(&self, id: &str) -> Result<Option<Blueprint>, Reply> {
        let chat = self.host.load_chat(id).map_err(|_| Code::Internal)?;
        Ok(chat.state.blueprint.filter(validate_stored))
    }

    /// 사람이 본 판이 지금도 떠 있는 판인가 — 아니면 Stale.
    fn active_blueprint(&self, id: &str, version: u32) -> Result<Blueprint, Reply> {
        let b = self.stored_blueprint(id)?;
        b.filter(|b| b.status == BlueprintStatus::Active && b.version == version)
            .ok_or_else(|| Code::Stale.into())
    }

    fn submit(&self, id: &str, version: u32, data: Value) -> Result<(), Reply> {
        let mut b = self.active_blueprint(id, version)?;
        if b.slots.values().any(|s| s.path.is_none()) {
            return Err(Code::SlotEmpty.into());
        }
        check_data(&b, &data).map_err(|e| e.reply())?;
        b.data_model = data;
        b.status = BlueprintStatus::Submitted;
        b.build_pending = true;
        b.build_attempts = 0;
        self.update_chat(id, |s| s.blueprint = Some(b.clone()))?;
        Ok(())
    }

    async fn pick_slot_path(&self, id: &str, slot: &str, version: u32) -> Result<String, Reply> {
        let b = self.active_blueprint(id, version)?;
        let s = b.slots.get(slot).ok_or(Code::SlotUnknown)?;
        let target = (s.event.clone(), s.field.clone());
        let title = format!(
            "IEUM — {} 고르기",
            edit::field_title(&self.catalog, &target.0, &target.1)
        );
        let path = self
            .host
            .pick_folder(id, &title)
            .await
            .ok_or(Code::PickCancelled)?;
        // 문구에는 경로가 섞여 있어 버린다.
        self.host
            .check_folder(&path)
            .map_err(|_| Code::FolderNotAllowed)?;
        // 창이 열린 사이 설계서가 바뀌었을 수 있다 — 같은 판·같은 대상의 칸일 때만 적는다.
        let mut b = self.active_blueprint(id, version)?;
        let s = b
            .slots
            .get_mut(slot)
            .filter(|s| (&s.event, &s.field) == (&target.0, &target.1))
            .ok_or(Code::Stale)?;
        let text = path.to_string_lossy().into_owned();
        s.path = Some(text.clone());
        s.picked_version = Some(version);
        // 데이터에 이미 이 경로가 있으면 적은 저장본이 `validate_stored`를 못 넘어 설계서가 사라진다.
        check_data(&b, &b.data_model).map_err(|e| e.reply())?;
        self.update_chat(id, |c| c.blueprint = Some(b.clone()))?;
        Ok(text)
    }

    fn sync_data(&self, id: &str, data: Value) -> Result<(), Reply> {
        let Some(mut b) = self
            .stored_blueprint(id)?
            .filter(|b| b.status == BlueprintStatus::Active)
        else {
            return Ok(());
        };
        check_data(&b, &data).map_err(|e| e.reply())?;
        b.data_model = data;
        self.update_chat(id, |s| s.blueprint = Some(b.clone()))?;
        Ok(())
    }

    /// `ui` 동작 — 검증을 통과한 설계서만 저장한다. 워크플로우는 건드리지 않는다.
    pub(super) fn ui(
        &self,
        id: &str,
        state: &ChatState,
        messages: &[Value],
    ) -> Result<Value, Reply> {
        let current = state.blueprint.as_ref().filter(|b| validate_stored(b));
        let b = apply_ui(current, messages).map_err(|e| e.reply())?;
        let version = b.version;
        self.update_chat(id, |s| s.blueprint = Some(b.clone()))?;
        Ok(json!({"status": "awaiting_user", "version": version}))
    }

    /// `bind-folder` 동작 — 제출된 설계서의 슬롯 경로를 노드 폴더 칸에 넣고 슬롯을 다 쓴 것으로 적는다.
    /// 저장은 다른 변경과 같은 `commit`(끄기·Stale·확인 대기 무효화). 경로는 결과·에러에 싣지 않는다.
    pub(super) fn bind_folder(
        &self,
        id: &str,
        mut doc: Doc,
        state: &ChatState,
        node_id: &str,
        field: &str,
        slot: &str,
    ) -> Result<(), Reply> {
        let b = state
            .blueprint
            .as_ref()
            .filter(|b| b.status == BlueprintStatus::Submitted && validate_stored(b))
            .ok_or(Code::BlueprintNotSubmitted)?;
        let s = b.slots.get(slot).ok_or(Code::SlotUnknown)?;
        let path = s.path.as_deref().ok_or(Code::SlotEmpty)?;
        let node = edit::find_mut(&mut doc.workflow, node_id)?;
        if !edit::is_folder_field(&node.event, field) {
            return Err(Code::NotFolderField.into());
        }
        if s.event != node.event || s.field != field {
            return Err(Code::SlotMismatch.into());
        }
        // 고른 뒤 폴더가 바뀌었을 수 있다. 문구에는 경로가 섞여 있어 버린다.
        self.host
            .check_folder(Path::new(path))
            .map_err(|_| Code::FolderNotAllowed)?;
        node.config.insert(field.to_string(), path.into());
        self.commit(id, doc)?;
        self.update_chat(id, |c| {
            if let Some(s) = c.blueprint.as_mut().and_then(|b| b.slots.get_mut(slot)) {
                s.path = None;
                s.bound_node = Some(node_id.to_string());
            }
        })?;
        Ok(())
    }
}

/// 고아를 정리하고(입력) 또는 고아가 있으면 원본과 달라지게 하고(저장본), 표·한도·구조를 검사한다.
/// 표기·글자 다듬기는 제자리에서 한다. 돌려주는 것은 남은 AppGate들.
fn normalize(comps: &mut Vec<Value>, d: Dialect) -> Result<Vec<Gate>, UiError> {
    prune(comps);
    if comps.len() > MAX_COMPONENTS {
        return Err(fail("limit", None));
    }
    // 입력은 id upsert라 중복이 없다. 저장본에 id를 다시 쓴 컴포넌트가 있으면 고아 정리를 빠져나가므로 거부한다.
    let mut ids = HashSet::new();
    for c in comps.iter_mut() {
        let id = c["id"].as_str().unwrap_or_default();
        if !ids.insert(id.to_string()) {
            return Err(fail("root", Some(id.into())));
        }
        check_component(c, d)?;
    }
    // 여기부터 모든 원소는 표대로 생긴 객체다.
    let root_err = || fail("root", Some("root".into()));
    let blueprints = comps
        .iter()
        .filter(|c| c["component"] == "Blueprint")
        .count();
    let root = comps
        .iter()
        .find(|c| c["id"] == "root")
        .filter(|r| r["component"] == "Blueprint" && blueprints == 1)
        .ok_or_else(root_err)?;
    let mut seen = HashSet::new();
    let children_ok = root["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .all(|id| id != "root" && seen.insert(id) && comps.iter().any(|c| c["id"] == id));
    if !children_ok {
        return Err(root_err());
    }
    let submits = comps
        .iter()
        .filter(|c| c["component"] == "SubmitButton")
        .count();
    if submits != 1 {
        return Err(fail("submit_count", None));
    }
    let mut gates: Vec<Gate> = Vec::new();
    for c in comps.iter().filter(|c| c["component"] == "AppGate") {
        let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let gate = (
            s(&c["slot"]),
            s(&c["target"]["event"]),
            s(&c["target"]["field"]),
        );
        let at = Some(s(&c["id"]));
        if gates.iter().any(|g| g.0 == gate.0) {
            return Err(fail("slot_id", at));
        }
        if gates.iter().any(|g| g.1 == gate.1 && g.2 == gate.2) {
            return Err(fail("slot_duplicate_target", at));
        }
        gates.push(gate);
    }
    Ok(gates)
}

/// root와 그 children만 남긴다. 자식을 가진 컴포넌트는 root(`Blueprint`) 하나뿐이라 한 단계면 된다.
fn prune(comps: &mut Vec<Value>) {
    let Some(root) = comps.iter().find(|c| c["id"] == "root") else {
        comps.clear();
        return;
    };
    let keep: HashSet<String> = root["children"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(["root"])
        .map(String::from)
        .collect();
    comps.retain(|c| c["id"].as_str().is_some_and(|id| keep.contains(id)));
}

fn check_component(c: &mut Value, d: Dialect) -> Result<(), UiError> {
    let id = c["id"].as_str().unwrap_or_default().to_string();
    let at = |name: &str| Some(format!("{id}.{name}"));
    let props = c["component"]
        .as_str()
        .and_then(|t| COMPONENTS.iter().find(|(n, _)| *n == t))
        .map(|(_, p)| *p)
        .filter(|_| !id.is_empty())
        .ok_or_else(|| fail("component_type", Some(id.clone())))?;
    // A2UI 함수 호출·v1.0 표기를 LLM이 직접 쓰면 조용히 리터럴이 되지 않게 돌려보낸다.
    if d == Dialect::Llm && has_at_key(c) {
        return Err(fail("binding", Some(id.clone())));
    }
    let o = c.as_object_mut().expect("component가 있으면 객체");
    if let Some(k) = o
        .keys()
        .find(|k| *k != "id" && *k != "component" && !props.iter().any(|(n, _)| n == k))
    {
        return Err(fail("unknown_prop", at(k)));
    }
    for (name, kind) in props {
        let r = match o.get_mut(*name) {
            Some(v) => check_prop(*kind, v, d),
            None if OPTIONAL.contains(name) => Ok(()),
            None => check_prop(*kind, &mut Value::Null, d),
        };
        r.map_err(|rule| fail(rule, at(name)))?;
    }
    Ok(())
}

fn check_prop(kind: Prop, v: &mut Value, d: Dialect) -> Result<(), &'static str> {
    match kind {
        Text(max) => *v = Value::String(text(v, max)?),
        Bool if v.is_boolean() => {}
        Bool => return Err(shape(v)),
        Binding => *v = binding(v, d).ok_or("binding")?,
        Children => {
            let a = v.as_array().ok_or_else(|| shape(v))?;
            if !a.iter().all(Value::is_string) {
                return Err("unknown_prop");
            }
            if a.len() > MAX_CHILDREN {
                return Err("limit");
            }
        }
        Steps => items(
            v,
            0,
            MAX_STEPS,
            &[("label", 20, true), ("event", 40, false)],
        )?,
        Options(min, max) => items(v, min, max, &[("value", 40, true), ("label", 20, true)])?,
        Slot => {
            let s = v.as_str().ok_or("slot_id")?.as_bytes();
            let ok = (1..=32).contains(&s.len())
                && s[0].is_ascii_lowercase()
                && s.iter()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_');
            if !ok {
                return Err("slot_id");
            }
        }
        Target => {
            let t = v
                .as_object()
                .filter(|t| t.len() == 2)
                .ok_or("slot_target")?;
            let event = t.get("event").and_then(Value::as_str).unwrap_or_default();
            let field = t.get("field").and_then(Value::as_str).unwrap_or_default();
            if !FOLDER_FIELDS.contains(&(event, field)) {
                return Err("slot_target");
            }
        }
    }
    Ok(())
}

/// 객체 목록(steps·options): 키는 `fields`(이름, 글자 수, 필수)만, 개수는 `min..=max`.
fn items(
    v: &mut Value,
    min: usize,
    max: usize,
    fields: &[(&str, usize, bool)],
) -> Result<(), &'static str> {
    let shp = shape(v);
    let a = v.as_array_mut().ok_or(shp)?;
    for item in a.iter_mut() {
        let o = item.as_object_mut().ok_or("unknown_prop")?;
        if !o.keys().all(|k| fields.iter().any(|(n, ..)| n == k)) {
            return Err("unknown_prop");
        }
        for (name, len, required) in fields {
            match o.get_mut(*name) {
                Some(x) => *x = Value::String(text(x, *len)?),
                None if *required => return Err("unknown_prop"),
                None => {}
            }
        }
    }
    if !(min..=max).contains(&a.len()) {
        return Err("limit");
    }
    Ok(())
}

/// 글자 prop 자리에 객체가 오면 바인딩을 쓰려 한 것이다(A2UI는 거의 모든 prop을 바인딩할 수 있다).
fn shape(v: &Value) -> &'static str {
    if v.is_object() {
        "binding"
    } else {
        "unknown_prop"
    }
}

fn text(v: &Value, max: usize) -> Result<String, &'static str> {
    Ok(clean(v.as_str().ok_or_else(|| shape(v))?, max))
}

/// 줄바꿈은 공백으로, 그 밖의 제어문자는 지우고 `max`글자에서 자른다.
fn clean(s: &str, max: usize) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\n' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .take(max)
        .collect()
}

/// 바인딩 하나를 v1.0 `{"@path"}`로. 입력은 `{"path"}`만, 저장본은 `{"@path"}`만 받는다.
fn binding(v: &Value, d: Dialect) -> Option<Value> {
    let key = match d {
        Dialect::Llm => "path",
        Dialect::Stored => "@path",
    };
    let o = v.as_object().filter(|o| o.len() == 1)?;
    // 루트 "/"는 거부한다 — 입력 하나가 데이터 모델 전체를 글자로 덮어쓴다.
    let p = o
        .get(key)?
        .as_str()
        .filter(|p| p.len() > 1 && p.starts_with('/'))?;
    Some(json!({"@path": p}))
}

/// 저장본 컴포넌트의 바인딩을 LLM 표기로 되돌린다(바인딩은 맨 위 prop에만 있다).
fn to_llm(c: &mut Value) {
    let Some(o) = c.as_object_mut() else { return };
    for v in o.values_mut() {
        if let Some(p) = v
            .as_object()
            .filter(|b| b.len() == 1)
            .and_then(|b| b.get("@path"))
        {
            *v = json!({"path": p});
        }
    }
}

/// `@`(전각 `＠` 포함)로 시작하는 키가 어딘가에 있는가.
fn has_at_key(v: &Value) -> bool {
    match v {
        Value::Object(o) => o
            .iter()
            .any(|(k, v)| k.starts_with(['@', '＠']) || has_at_key(v)),
        Value::Array(a) => a.iter().any(has_at_key),
        _ => false,
    }
}

/// 키·문자열 값 중 `f`를 만족하는 것이 있는가.
fn any_text(v: &Value, f: &dyn Fn(&str) -> bool) -> bool {
    match v {
        Value::String(s) => f(s),
        Value::Object(o) => o.iter().any(|(k, v)| f(k) || any_text(v, f)),
        Value::Array(a) => a.iter().any(|v| any_text(v, f)),
        _ => false,
    }
}

/// `updateDataModel` — path가 없거나 `/`이면 전체, 아니면 JSON Pointer 자리에 쓴다. 값이 없거나 null이면 지운다.
// ponytail: 중간 경로는 객체만 따라간다(배열 원소 `/a/0` 쓰기는 거부). LLM이 배열 원소만 고치는 일이 잦으면 그때 더한다.
fn set_data(data: &mut Value, path: Option<&Value>, value: Option<&Value>) -> Option<()> {
    let path = match path {
        None => "/",
        Some(p) => p.as_str()?,
    };
    let value = value.filter(|v| !v.is_null()).cloned();
    if path == "/" || path.is_empty() {
        *data = value.unwrap_or_else(|| json!({}));
        return Some(());
    }
    let (parent, last) = path.rsplit_once('/').filter(|_| path.starts_with('/'))?;
    let unescape = |t: &str| t.replace("~1", "/").replace("~0", "~");
    let last = unescape(last);
    match value {
        None => {
            if let Some(o) = data.pointer_mut(parent).and_then(Value::as_object_mut) {
                o.shift_remove(&last);
            }
        }
        Some(v) => {
            let mut cur = data;
            for t in parent.split('/').skip(1) {
                cur = cur
                    .as_object_mut()?
                    .entry(unescape(t))
                    .or_insert_with(|| json!({}));
            }
            cur.as_object_mut()?.insert(last, v);
        }
    }
    Some(())
}
