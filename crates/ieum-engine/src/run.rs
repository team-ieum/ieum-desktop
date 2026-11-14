//! 순차 실행기 — BE `SyncExecutionRuntime.propagate`·`resolveEdges`, `ExecutionCursor.liveOutgoingEdges`를 옮겼다.
//! 다른 점: 병렬 없이 준비된 노드를 노드 배열 순서로 하나씩 돌린다.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::catalog::Catalog;
use crate::event::{EventSink, ExecEvent, RunStatus};
use crate::model::{Node, Workflow};
use crate::render::{Outputs, render_deep};
use crate::validate::{ValidationError, validate};

/// agent 정적 검증의 TOO_LARGE 상한과 같다.
const MAX_NODES: usize = 500;
const MAX_EDGES: usize = 2000;
const BRANCH_KEYS: &str = "_branchKeys";

pub struct NodeCtx<'a> {
    pub node: &'a Node,
    /// 참조식을 치환한 사본.
    pub config: Map<String, Value>,
    pub events: &'a EventSink,
    pub cancel: &'a CancellationToken,
}

#[derive(Debug, Clone)]
pub struct NodeError {
    pub message: String,
    /// AI에게 보여도 되는 코드. 모양은 `<CODE>` 또는 `<CODE> <칸 key>`. 경로·파일 이름·사용자 값은 절대 넣지 않는다.
    pub code: Option<String>,
}

#[async_trait]
pub trait NodeRunner: Send + Sync {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError>;

    /// `run` + 사용량(AI 토큰 등 — 엔진은 모양을 모르고 `NodeRecord.usage`에 그대로 싣는다).
    /// 실패해도 이미 쓴 사용량은 돌려줄 수 있다. 기본은 사용량 없음.
    async fn run_metered(
        &self,
        ctx: NodeCtx<'_>,
    ) -> (Result<Map<String, Value>, NodeError>, Option<Value>) {
        (self.run(ctx).await, None)
    }
}

/// event → 러너.
pub type Runners = HashMap<String, Arc<dyn NodeRunner>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeStatus {
    Succeeded,
    Failed,
    Skipped,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRecord {
    pub status: NodeStatus,
    pub output: Option<Map<String, Value>>,
    pub error: Option<String>,
    /// 러너가 알린 사용량(`run_metered`). 옛 기록에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
    /// 실패한 노드의 `NodeError.code`. 옛 기록에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub run_id: String,
    pub status: RunStatus,
    pub validation_errors: Vec<ValidationError>,
    /// `Rejected`일 때만 Some.
    pub reject_reason: Option<RejectReason>,
    pub nodes: BTreeMap<String, NodeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RejectReason {
    TooLarge,
    UnknownEvent {
        event: String,
    },
    Cycle,
    /// 검증 에러 — `validation_errors`에 담긴다.
    Invalid,
}

fn is_type(node: &Node, t: &str) -> bool {
    node.r#type
        .as_deref()
        .is_some_and(|x| x.to_uppercase() == t)
}

/// CONDITION이면 live인 conditionType 집합, 그 밖이면 None(모든 나가는 엣지 live).
fn live_keys(node: &Node, output: &Map<String, Value>) -> Option<HashSet<String>> {
    if !is_type(node, "CONDITION") {
        return None;
    }
    Some(match output.get(BRANCH_KEYS) {
        Some(Value::Array(list)) => list
            .iter()
            .map(|k| match k {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => {
            let r = output.get("result") == Some(&Value::Bool(true));
            HashSet::from([r.to_string()])
        }
    })
}

/// 러너 future를 poll하다 panic하면 None — 러너 하나의 버그가 앱을 죽이지 않게 한다.
async fn catch_panic<F: Future + Unpin>(mut fut: F) -> Option<F::Output> {
    std::future::poll_fn(|cx| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| Pin::new(&mut fut).poll(cx))) {
            Ok(poll) => poll.map(Some),
            Err(_) => Poll::Ready(None),
        }
    })
    .await
}

/// BE `assertNoCycle`과 같은 Kahn 위상정렬 — 다 처리하지 못하면 사이클.
fn has_cycle(
    wf: &Workflow,
    idx: &HashMap<&str, usize>,
    indeg: &[usize],
    outgoing: &[Vec<usize>],
) -> bool {
    let mut indeg = indeg.to_vec();
    let mut queue: Vec<usize> = (0..indeg.len()).filter(|&i| indeg[i] == 0).collect();
    let mut processed = 0;
    while let Some(i) = queue.pop() {
        processed += 1;
        for &e in &outgoing[i] {
            let t = idx[wf.edges[e].target.as_str()];
            indeg[t] -= 1;
            if indeg[t] == 0 {
                queue.push(t);
            }
        }
    }
    processed < indeg.len()
}

pub async fn run_workflow(
    wf: &Workflow,
    catalog: &Catalog,
    runners: &Runners,
    trigger_output: Map<String, Value>,
    events: EventSink,
    cancel: CancellationToken,
) -> RunResult {
    let run_id = uuid::Uuid::new_v4().to_string();
    let send = |e: ExecEvent| {
        let _ = events.send(e); // 받는 쪽이 없어도 실행은 계속한다
    };
    send(ExecEvent::RunStarted {
        run_id: run_id.clone(),
    });
    let mut records: BTreeMap<String, NodeRecord> = wf
        .nodes
        .iter()
        .map(|n| {
            let rec = NodeRecord {
                status: NodeStatus::NotRun,
                output: None,
                error: None,
                usage: None,
                error_code: None,
            };
            (n.id.clone(), rec)
        })
        .collect();
    let finish = |status, reject_reason, validation_errors, nodes| {
        send(ExecEvent::RunFinished {
            run_id: run_id.clone(),
            status,
        });
        RunResult {
            run_id: run_id.clone(),
            status,
            validation_errors,
            reject_reason,
            nodes,
        }
    };
    let reject = |reason, errors, nodes| finish(RunStatus::Rejected, Some(reason), errors, nodes);

    if wf.nodes.len() > MAX_NODES || wf.edges.len() > MAX_EDGES {
        return reject(RejectReason::TooLarge, Vec::new(), records);
    }
    let errors = validate(wf, catalog);
    if !errors.is_empty() {
        return reject(RejectReason::Invalid, errors, records);
    }
    if let Some(n) = wf.nodes.iter().find(|n| {
        catalog.get(&n.event).is_none()
            || (!is_type(n, "TRIGGER") && !runners.contains_key(&n.event))
    }) {
        let reason = RejectReason::UnknownEvent {
            event: n.event.clone(),
        };
        return reject(reason, Vec::new(), records);
    }

    // 검증을 통과했으니 id는 유일하고, 엣지 끝점은 모두 있는 노드이며, 트리거는 하나다.
    let idx: HashMap<&str, usize> = wf
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let n = wf.nodes.len();
    let mut pending = vec![0usize; n];
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (e, edge) in wf.edges.iter().enumerate() {
        pending[idx[edge.target.as_str()]] += 1;
        outgoing[idx[edge.source.as_str()]].push(e);
    }
    if has_cycle(wf, &idx, &pending, &outgoing) {
        return reject(RejectReason::Cycle, Vec::new(), records);
    }
    let mut has_live = vec![false; n];
    let mut resolved = vec![false; n];
    let mut ready: BTreeSet<usize> = wf
        .nodes
        .iter()
        .position(|n| is_type(n, "TRIGGER"))
        .into_iter()
        .collect();
    let mut outputs = Outputs::new();
    let mut status = RunStatus::Succeeded;

    while let Some(i) = ready.pop_first() {
        if cancel.is_cancelled() {
            status = RunStatus::Cancelled;
            break;
        }
        let node = &wf.nodes[i];
        send(ExecEvent::NodeStarted {
            node_id: node.id.clone(),
        });
        let (result, usage) = if is_type(node, "TRIGGER") {
            (Ok(trigger_output.clone()), None)
        } else {
            let config = node
                .config
                .iter()
                .map(|(k, v)| (k.clone(), render_deep(v, &outputs)))
                .collect();
            let ctx = NodeCtx {
                node,
                config,
                events: &events,
                cancel: &cancel,
            };
            catch_panic(runners[&node.event].run_metered(ctx))
                .await
                .unwrap_or_else(|| {
                    let e = NodeError {
                        message: "노드 실행 중 내부 오류가 났습니다.".into(),
                        code: None,
                    };
                    (Err(e), None)
                })
        };
        let rec = records.get_mut(&node.id).unwrap();
        rec.usage = usage;
        let output = match result {
            Err(e) => {
                rec.status = NodeStatus::Failed;
                rec.error = Some(e.message.clone());
                rec.error_code = e.code;
                send(ExecEvent::NodeFailed {
                    node_id: node.id.clone(),
                    message: e.message,
                });
                status = if cancel.is_cancelled() {
                    RunStatus::Cancelled
                } else {
                    RunStatus::Failed
                };
                break;
            }
            Ok(output) => output,
        };
        rec.status = NodeStatus::Succeeded;
        rec.output = Some(output.clone());
        send(ExecEvent::NodeSucceeded {
            node_id: node.id.clone(),
            output: output.clone(),
        });
        resolved[i] = true;

        // 나가는 엣지 해소 — 모든 입력이 dead인 노드는 Skipped로 두고 그 아래로 dead를 전파한다.
        let keys = live_keys(node, &output);
        outputs.insert(node.id.clone(), output);
        let mut sources = VecDeque::from([(i, true)]);
        while let Some((src, src_live)) = sources.pop_front() {
            for &e in &outgoing[src] {
                let edge = &wf.edges[e];
                let t = idx[edge.target.as_str()];
                if resolved[t] {
                    continue;
                }
                let live = src_live
                    && keys.as_ref().is_none_or(|k| {
                        edge.condition_type.as_ref().is_some_and(|c| k.contains(c))
                    });
                has_live[t] |= live;
                pending[t] -= 1;
                if pending[t] > 0 {
                    continue;
                }
                if has_live[t] {
                    ready.insert(t);
                } else {
                    resolved[t] = true;
                    let id = &wf.nodes[t].id;
                    records.get_mut(id).unwrap().status = NodeStatus::Skipped;
                    send(ExecEvent::NodeSkipped {
                        node_id: id.clone(),
                    });
                    sources.push_back((t, false));
                }
            }
        }

        if cancel.is_cancelled() {
            status = RunStatus::Cancelled;
            break;
        }
    }
    finish(status, None, Vec::new(), records)
}
