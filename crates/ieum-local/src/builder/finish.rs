//! finish·확인 대기·사람의 [켜기]. 켜는 길은 `confirm_finish`뿐이고 `Op`로는 올 수 없다.

use std::fs;
use std::path::Path;

use ieum_engine::{Node, ValidationError, Workflow};
use serde::Serialize;
use serde_json::{Value, json};

use super::edit::{AI_EVENT, FOLDER_FIELDS};
use super::{
    Builder, ChatState, Code, Doc, FinishAction, FinishAi, NodeState, PendingFinish, Reply,
    SaveError, node_state, workflow_hash,
};

/// 완료까지 남은 것 한 줄.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Remaining {
    #[serde(skip_serializing_if = "Option::is_none")]
    node_id: Option<String>,
    /// `UNTESTED` | `TEST_FAILED` | `INVALID` | `FOLDER_MISSING` | `AI_NOT_CONNECTED`
    reason: &'static str,
}

fn at(node: &Node, reason: &'static str) -> Remaining {
    Remaining {
        node_id: Some(node.id.clone()),
        reason,
    }
}

fn text<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.config
        .get(key)?
        .as_str()
        .filter(|s| !s.trim().is_empty())
}

/// 확인 카드에 보일 파일 동작 — 파일 노드만, 배열 순서로.
fn actions(wf: &Workflow) -> Vec<FinishAction> {
    wf.nodes
        .iter()
        .filter_map(|n| {
            let target = match n.event.as_str() {
                "local.file_move" => {
                    let base = text(n, "baseFolder").unwrap_or_default();
                    Some(match text(n, "destination") {
                        Some(dest) => format!("{base}/{dest}"),
                        None => base.to_string(),
                    })
                }
                "local.file_rename" => Some(text(n, "newName").unwrap_or_default().to_string()),
                "local.file_trash" => None,
                _ => return None,
            };
            Some(FinishAction {
                node_id: n.id.clone(),
                event: n.event.clone(),
                label: n.label.clone().unwrap_or_default(),
                target,
            })
        })
        .collect()
}

/// 확인 카드에 보일 AI 노드 — 배열 순서로.
fn ai_nodes(wf: &Workflow) -> Vec<FinishAi> {
    wf.nodes
        .iter()
        .filter(|n| n.event == AI_EVENT)
        .map(|n| FinishAi {
            node_id: n.id.clone(),
            label: n.label.clone().unwrap_or_default(),
            provider: text(n, "provider").unwrap_or_default().to_string(),
            send_content: n.config.get("sendContent").and_then(Value::as_bool) == Some(true),
        })
        .collect()
}

fn not_ready(remaining: Vec<Remaining>, errors: Vec<ValidationError>) -> Reply {
    Reply::err_with(
        Code::NotReady,
        json!({"remaining": remaining, "errors": errors}),
    )
}

impl Builder {
    /// 완료 조건. `finish`와 `confirm_finish`가 같이 쓴다. 비어 있으면 통과.
    async fn readiness(
        &self,
        doc: &Doc,
        state: &ChatState,
    ) -> (Vec<Remaining>, Vec<ValidationError>) {
        let nodes = &doc.workflow.nodes;
        let mut remaining = Vec::new();
        let errors = self.errors(&doc.workflow);
        if !errors.is_empty() {
            remaining.push(Remaining {
                node_id: None,
                reason: "INVALID",
            });
        }
        for n in nodes {
            match node_state(n, state) {
                NodeState::Untested => remaining.push(at(n, "UNTESTED")),
                NodeState::Failed => remaining.push(at(n, "TEST_FAILED")),
                _ => {}
            }
        }
        // 폴더가 정책 안이고 실제로 열리는가.
        for n in nodes {
            for (_, key) in FOLDER_FIELDS.iter().filter(|(event, _)| *event == n.event) {
                let Some(folder) = text(n, key).map(Path::new) else {
                    continue; // 빈 칸은 검증 에러(REQUIRED) 몫
                };
                if self.host.check_folder(folder).is_err() || fs::read_dir(folder).is_err() {
                    remaining.push(at(n, "FOLDER_MISSING"));
                }
            }
        }
        for n in nodes.iter().filter(|n| n.event == AI_EVENT) {
            let provider = text(n, "provider").unwrap_or_default();
            // OpenAI는 기본 모델이 없다.
            let no_model = provider == "api:openai" && text(n, "model").is_none();
            if no_model || !self.host.ai_ready(provider, text(n, "credential")).await {
                remaining.push(at(n, "AI_NOT_CONNECTED"));
            }
        }
        (remaining, errors)
    }

    /// 완료 요청 — 통과하면 확인 대기를 남기고 사람을 부른다. `enabled`는 건드리지 않는다.
    pub(super) async fn finish(
        &self,
        id: &str,
        doc: &Doc,
        state: &ChatState,
    ) -> Result<Value, Reply> {
        let (remaining, errors) = self.readiness(doc, state).await;
        if !remaining.is_empty() {
            return Err(not_ready(remaining, errors));
        }
        let wf = &doc.workflow;
        let folder = wf
            .nodes
            .iter()
            .filter(|n| n.event == "local.folder_new_file")
            .find_map(|n| text(n, "folder"));
        let pending = PendingFinish {
            hash: workflow_hash(wf),
            folder: folder.unwrap_or_default().to_string(),
            actions: actions(wf),
            ai: ai_nodes(wf),
        };
        self.update_chat(id, |s| s.pending_finish = Some(pending.clone()))?;
        self.host.finish_pending(id, &doc.name);
        Ok(json!({"status": "awaiting_user"}))
    }

    /// 저장본과 hash가 같은 확인 대기. 달라졌으면 None — 지우지는 않는다(읽기만).
    /// 잠금 없이 불리는 `pending()`이 두 읽기 사이에 생긴 새 대기를 지우지 않게. 낡은 대기는 commit·confirm이 지운다.
    pub(super) fn pending_of(&self, id: &str, doc: &Doc) -> Option<PendingFinish> {
        let pending = self.host.load_chat(id).ok()?.state.pending_finish?;
        (pending.hash == workflow_hash(&doc.workflow)).then_some(pending)
    }

    /// 사람의 [켜기]/[아직]. 잠금은 호출자(`confirm_finish`)가 잡는다.
    pub(super) async fn confirm(&self, id: &str, accept: bool) -> Result<(), Reply> {
        if !accept {
            self.update_chat(id, |s| s.pending_finish = None)?;
            return Ok(());
        }
        let mut doc = self.load(id)?;
        let waiting = self.pending_of(id, &doc).is_some();
        let state = self.update_chat(id, |s| s.pending_finish = None)?;
        // 대기 뒤에 바뀌었거나 그사이 조건이 깨졌으면 켜지 않는다 — 다시 finish해야 한다.
        let (remaining, errors) = self.readiness(&doc, &state).await;
        if !waiting || !remaining.is_empty() {
            return Err(not_ready(remaining, errors));
        }
        doc.enabled = true;
        self.host.save(id, doc).map_err(|e| match e {
            SaveError::Stale => Code::Stale,
            SaveError::Rejected(_) => Code::SaveRejected,
        })?;
        Ok(())
    }
}
