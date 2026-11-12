//! 웹 대시보드 — 요약·최근 실행·최근 오류. GET만, 모르는 필드는 무시.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::client::{BeClient, WebError};
use super::executions::{Page, fetch_page};

/// BE `WorkflowDashboardSummaryResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDashboardSummary {
    pub metrics: Metrics,
    /// 시(`"0"`~`"23"`, BE 표시 시간대) → 실행 수.
    pub hourly_counts: BTreeMap<String, u64>,
    pub workflow_stats: WorkflowStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub today_runs: u64,
    pub percentage_change: f64,
    pub average_duration_seconds: f64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStats {
    pub total: u64,
    pub active: u64,
    pub inactive: u64,
    pub errored: u64,
    pub running: u64,
}

/// BE `WorkflowDashboardExecutionResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDashboardExecution {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub status: String,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub trigger_type: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// BE `WorkflowDashboardErrorResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDashboardError {
    pub execution_id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    #[serde(default)]
    pub failed_node_id: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
}

pub async fn summary(c: &BeClient) -> Result<WebDashboardSummary, WebError> {
    c.get("/api/v1/workflows/dashboard/summary").await
}

pub async fn executions(
    c: &BeClient,
    cursor: Option<String>,
) -> Result<Page<WebDashboardExecution>, WebError> {
    fetch_page(
        c,
        "/api/v1/workflows/dashboard/executions",
        cursor.as_deref(),
        "최근 실행",
    )
    .await
}

pub async fn errors(
    c: &BeClient,
    cursor: Option<String>,
) -> Result<Page<WebDashboardError>, WebError> {
    fetch_page(
        c,
        "/api/v1/workflows/dashboard/errors",
        cursor.as_deref(),
        "최근 오류",
    )
    .await
}
