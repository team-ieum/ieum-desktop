//! 웹 실행 기록·노드 로그 조회와 실행 생중계(SSE) 구독. GET만.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::task::AbortHandle;
use tokio::time::Instant;
use uuid::Uuid;

use super::client::{BeClient, WebError};
use super::sse::{self, SseLimits};

pub const PAGE_SIZE: u32 = 20;
/// 동시 구독 상한 — 넘으면 가장 오래된 구독을 끊는다.
pub const MAX_WATCHES: usize = 5;
/// 구독 하나의 재연결 상한 — BE 배포 전환 중 스냅샷 직후 닫히기를 반복할 때 헛돌지 않게.
const MAX_RECONNECTS: usize = 30;

/// BE `PageResponse`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub content: Vec<T>,
    pub has_next: bool,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPage {
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default)]
    has_next: bool,
    next_cursor: Option<String>,
}

/// BE `WorkflowExecutionResponse`. 시간은 UTC `…Z` 문자열 그대로. 나머지 키는 `extra`로 보존.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebExecution {
    pub id: String,
    pub workflow_id: String,
    pub status: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// BE `WorkflowExecutionLogResponse`. 나머지 키(`inputJson`·`errorMessage`·`createdAt` 등)는 `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebNodeLog {
    pub id: String,
    pub node_id: String,
    pub status: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 경로에 끼워 넣는 id는 UUID만(정규형으로).
pub(crate) fn uuid(id: &str) -> Result<String, WebError> {
    Uuid::parse_str(id)
        .map(|u| u.to_string())
        .map_err(|_| WebError::Server("잘못된 id예요.".into()))
}

/// 형식이 맞지 않는 항목은 그것만 건너뛰고 경고.
fn parse_items<T: DeserializeOwned>(items: Vec<Value>, what: &str) -> Vec<T> {
    items
        .into_iter()
        .filter_map(|v| match serde_json::from_value(v) {
            Ok(t) => Some(t),
            Err(_) => {
                eprintln!("[ieum] 형식이 맞지 않는 {what} 1건을 건너뛰었어요.");
                None
            }
        })
        .collect()
}

/// `path?size=20[&cursor=…]` 한 쪽.
pub(crate) async fn fetch_page<T: DeserializeOwned>(
    c: &BeClient,
    path: &str,
    cursor: Option<&str>,
    what: &str,
) -> Result<Page<T>, WebError> {
    let mut q = Url::parse("http://q/").expect("고정 주소");
    q.query_pairs_mut()
        .append_pair("size", &PAGE_SIZE.to_string());
    if let Some(cur) = cursor {
        q.query_pairs_mut().append_pair("cursor", cur);
    }
    let raw: RawPage = c
        .get(&format!("{path}?{}", q.query().unwrap_or_default()))
        .await?;
    Ok(Page {
        content: parse_items(raw.content, what),
        has_next: raw.has_next,
        next_cursor: raw.next_cursor,
    })
}

pub async fn list(
    c: &BeClient,
    workflow_id: &str,
    cursor: Option<String>,
) -> Result<Page<WebExecution>, WebError> {
    let path = format!("/api/v1/workflows/{}/executions", uuid(workflow_id)?);
    fetch_page(c, &path, cursor.as_deref(), "실행 기록").await
}

pub async fn logs(
    c: &BeClient,
    workflow_id: &str,
    execution_id: &str,
) -> Result<Vec<WebNodeLog>, WebError> {
    let path = format!(
        "/api/v1/workflows/{}/executions/{}/logs",
        uuid(workflow_id)?,
        uuid(execution_id)?
    );
    let items: Vec<Value> = c.get(&path).await?;
    Ok(parse_items(items, "노드 로그"))
}

/// 화면으로 보내는 `web-exec-event` payload: `{executionId, event}`, 생중계를 못 이으면 `{executionId, logs}`
/// 또는 `{executionId, error}`, 구독 상한으로 밀려나면 `{executionId, stopped: true}`.
pub type Emit = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub limits: SseLimits,
    /// 끊긴 뒤 재연결까지 기다림.
    pub reconnect_delay: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            limits: SseLimits::default(),
            reconnect_delay: Duration::from_secs(3),
        }
    }
}

struct Watch {
    execution_id: String,
    id: u64,
    task: AbortHandle,
    emit: Emit,
}

#[derive(Default)]
struct Inner {
    next: u64,
    watches: VecDeque<Watch>,
}

/// 실행 생중계 구독들. 창이 숨겨져도 유지되고, 끝 이벤트·최종 상태를 받으면 스스로 빠진다.
#[derive(Clone, Default)]
pub struct Watches {
    cfg: WatchConfig,
    inner: Arc<Mutex<Inner>>,
}

fn lock(m: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Watches {
    pub fn new(cfg: WatchConfig) -> Self {
        Self {
            cfg,
            inner: Arc::default(),
        }
    }

    /// 이미 구독 중이면 그대로 둔다. tokio 런타임 안에서 불러야 한다.
    pub fn watch(
        &self,
        c: Arc<BeClient>,
        workflow_id: &str,
        execution_id: &str,
        emit: Emit,
    ) -> Result<(), WebError> {
        let (wf, ex) = (uuid(workflow_id)?, uuid(execution_id)?);
        let mut g = lock(&self.inner);
        if g.watches.iter().any(|w| w.execution_id == ex) {
            return Ok(());
        }
        if g.watches.len() >= MAX_WATCHES
            && let Some(old) = g.watches.pop_front()
        {
            old.task.abort();
            (old.emit)(json!({"executionId": old.execution_id, "stopped": true}));
        }
        g.next += 1;
        let id = g.next;
        let (inner, cfg, ex2, emit2) = (
            self.inner.clone(),
            self.cfg.clone(),
            ex.clone(),
            emit.clone(),
        );
        let task = tokio::spawn(async move {
            follow(&c, &wf, &ex2, &cfg, &emit2).await;
            lock(&inner).watches.retain(|w| w.id != id);
        })
        .abort_handle();
        g.watches.push_back(Watch {
            execution_id: ex,
            id,
            task,
            emit,
        });
        Ok(())
    }

    pub fn unwatch(&self, execution_id: &str) {
        let mut g = lock(&self.inner);
        if let Some(i) = g
            .watches
            .iter()
            .position(|w| w.execution_id == execution_id)
            && let Some(w) = g.watches.remove(i)
        {
            w.task.abort();
        }
    }

    /// 전부 해제(로그아웃·서버 변경 — 옛 세션의 연결을 남기지 않는다).
    pub fn clear(&self) {
        for w in lock(&self.inner).watches.drain(..) {
            w.task.abort();
        }
    }

    /// 구독 중인 실행 id(오래된 순).
    pub fn active(&self) -> Vec<String> {
        lock(&self.inner)
            .watches
            .iter()
            .map(|w| w.execution_id.clone())
            .collect()
    }
}

/// SSE를 받아 넘긴다. 끝 이벤트면 끝. 끊기면 다시 붙는다 — BE가 연결마다 스냅샷(끝난 실행이면 끝 이벤트까지)을
/// 다시 보내므로 재연결이 곧 상태 조회다. 연결 실패 2연속·전체 `limits.total`·재연결 `MAX_RECONNECTS`회에 닿으면
/// `logs`로 최종 상태만.
async fn follow(c: &BeClient, wf: &str, ex: &str, cfg: &WatchConfig, emit: &Emit) {
    let path = format!("/api/v1/workflows/{wf}/executions/{ex}/events");
    let deadline = Instant::now() + cfg.limits.total;
    let mut fails = 0;
    for attempt in 0..=MAX_RECONNECTS {
        if attempt > 0 {
            tokio::time::sleep(cfg.reconnect_delay).await;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let r = match c.stream(&path, cfg.limits.connect).await {
            Ok(resp) => {
                fails = 0;
                let limits = SseLimits {
                    total: left,
                    ..cfg.limits.clone()
                };
                sse::read_events(resp, &limits, |event| {
                    let end = event["type"] == "EXECUTION_COMPLETED";
                    emit(json!({"executionId": ex, "event": event}));
                    end
                })
                .await
            }
            Err(e) => {
                fails += 1;
                Err(e)
            }
        };
        match r {
            Ok(true) => return,
            Ok(false) => eprintln!("[ieum] 실행 생중계가 끝 이벤트 없이 닫혔어요."),
            Err(e) => eprintln!("[ieum] 실행 생중계가 끊겼어요: {e}"),
        }
        if fails >= 2 {
            break;
        }
    }
    match logs(c, wf, ex).await {
        Ok(l) => emit(json!({"executionId": ex, "logs": l})),
        Err(e) => emit(json!({"executionId": ex, "error": e})),
    }
}
