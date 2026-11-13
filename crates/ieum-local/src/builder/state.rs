use std::collections::BTreeMap;

use ieum_engine::{Node, Workflow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::ai::AiUsage;

/// `accounts/<계정>/chats/<workflowId>.json`. `state`는 두 입구(터미널·채팅 패널)가 공유한다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatFile {
    pub messages: Vec<ChatMessage>,
    pub state: ChatState,
    pub usage: Vec<UsageEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: Role,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    #[serde(default)]
    pub at: String,
    pub usage: AiUsage,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatState {
    pub outline: Vec<String>,
    /// 노드 id → `node_hash`. None = 아직 채팅을 시작하지 않음.
    pub baseline: Option<BTreeMap<String, String>>,
    pub tests: BTreeMap<String, TestEntry>,
    pub pending_finish: Option<PendingFinish>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEntry {
    #[serde(default)]
    pub hash: String,
    pub status: TestStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    Success,
    Failed,
    Skipped,
    Passed,
    Untested,
}

impl NodeState {
    pub fn is_done(self) -> bool {
        matches!(
            self,
            NodeState::Success | NodeState::Skipped | NodeState::Passed
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PendingFinish {
    pub hash: String,
    pub folder: String,
    pub actions: Vec<FinishAction>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FinishAction {
    pub node_id: String,
    pub event: String,
    pub label: String,
    pub target: Option<String>,
}

/// 객체 키를 재귀로 정렬한다(배열 순서는 유지). `serde_json`의 `preserve_order` 여부와 무관하게 같은 글자가 나온다.
fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let ordered: BTreeMap<&String, Value> = o.iter().map(|(k, v)| (k, sorted(v))).collect();
            Value::Object(ordered.into_iter().map(|(k, v)| (k.clone(), v)).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

fn sha_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// sha256 hex of `"<event>\n<config JSON, 키 재귀 정렬, 공백 없음>"`. label·position은 들어가지 않는다.
pub fn node_hash(node: &Node) -> String {
    let config: Map<String, Value> = match sorted(&Value::Object(node.config.clone())) {
        Value::Object(o) => o,
        _ => unreachable!("객체를 정렬하면 객체다"),
    };
    let json = serde_json::to_string(&config).expect("JSON 값은 항상 직렬화된다");
    sha_hex(&format!("{}\n{json}", node.event))
}

/// sha256 hex of 줄 목록: `n <id> <node_hash>`(id 순) + `e <source> <target> <conditionType|->`(정렬), `\n`으로 이음.
pub fn workflow_hash(wf: &Workflow) -> String {
    let mut nodes: Vec<String> = wf
        .nodes
        .iter()
        .map(|n| format!("n {} {}", n.id, node_hash(n)))
        .collect();
    nodes.sort();
    let mut edges: Vec<String> = wf
        .edges
        .iter()
        .map(|e| {
            format!(
                "e {} {} {}",
                e.source,
                e.target,
                e.condition_type.as_deref().unwrap_or("-")
            )
        })
        .collect();
    edges.sort();
    nodes.extend(edges);
    sha_hex(&nodes.join("\n"))
}

/// 테스트 기록이 baseline보다 먼저다. 기록이 있는데 hash가 다르면 baseline과 같아도 untested.
pub fn node_state(node: &Node, state: &ChatState) -> NodeState {
    let hash = node_hash(node);
    if let Some(t) = state.tests.get(&node.id) {
        return match (t.hash == hash, t.status) {
            (false, _) => NodeState::Untested,
            (true, TestStatus::Success) => NodeState::Success,
            (true, TestStatus::Failed) => NodeState::Failed,
            (true, TestStatus::Skipped) => NodeState::Skipped,
        };
    }
    match state.baseline.as_ref().and_then(|b| b.get(&node.id)) {
        Some(h) if *h == hash => NodeState::Passed,
        _ => NodeState::Untested,
    }
}
