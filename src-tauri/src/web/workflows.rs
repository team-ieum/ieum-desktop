//! 웹 워크플로우 목록·상세 조회. GET만.

use ieum_engine::Workflow;
use serde::{Deserialize, Serialize};

use super::client::{BeClient, WebError};
use super::executions::{Page, fetch_page, uuid};

/// 목록을 끝까지 넘겨 읽는 쪽 수 상한(20개씩).
const MAX_PAGES: usize = 50;

/// BE `WorkflowResponse`의 목록용 부분.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebWorkflowSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub trigger_type: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// BE `WorkflowServiceType` 이름(`GOOGLE_DRIVE` 등). 옛 응답엔 없다.
    #[serde(default)]
    pub services: Option<Vec<String>>,
    /// BE `nodes` 배열의 길이만 — 화면에는 `nodeCount`로 간다.
    #[serde(
        default,
        rename(deserialize = "nodes", serialize = "nodeCount"),
        deserialize_with = "count"
    )]
    pub node_count: Option<usize>,
}

fn count<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<usize>, D::Error> {
    Ok(Option::<Vec<serde::de::IgnoredAny>>::deserialize(d)?.map(|v| v.len()))
}

/// BE `WorkflowResponse`. `nodes`·`edges`와 그 밖의 키는 엔진 `Workflow`로(모르는 키 보존).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebWorkflow {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub graph: Workflow,
}

/// 형식이 맞지 않는 항목은 건너뛴다.
pub async fn list(c: &BeClient) -> Result<Vec<WebWorkflowSummary>, WebError> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    // ponytail: 50쪽(1,000개)에서 멈춘다 — 더 많아지면 화면에 쪽 넘김을 둔다.
    for _ in 0..MAX_PAGES {
        let p: Page<WebWorkflowSummary> =
            fetch_page(c, "/api/v1/workflows", cursor.as_deref(), "워크플로우").await?;
        out.extend(p.content);
        match p.next_cursor {
            Some(next) if p.has_next => cursor = Some(next),
            _ => break,
        }
    }
    Ok(out)
}

pub async fn get(c: &BeClient, id: &str) -> Result<WebWorkflow, WebError> {
    c.get(&format!("/api/v1/workflows/{}", uuid(id)?)).await
}
