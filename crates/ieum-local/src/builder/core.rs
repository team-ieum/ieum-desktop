use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use ieum_engine::{Catalog, ValidationError, Workflow};
use serde_json::{Value, json};

use super::edit::{self, AI_EVENT};
use super::fields;
use super::guide::GUIDE;
use super::{
    AiDefault, ChatState, Code, Doc, Host, Op, PendingFinish, Reply, SaveError, node_hash,
    workflow_hash,
};

/// 워크플로우 id별 잠금(프로세스 전역) — 한 워크플로우에는 한 번에 한 동작만.
// ponytail: 지운 워크플로우의 잠금이 맵에 남는다(id당 몇십 바이트) — 수만 개가 되면 지울 때 같이 빼기
static LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(Mutex::default);

fn lock_of(workflow_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    LOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(workflow_id.to_string())
        .or_default()
        .clone()
}

/// 동작 함수는 `Result<_, Code>`로 거부하고 `?`가 응답으로 바꾼다.
impl From<Code> for Reply {
    fn from(code: Code) -> Reply {
        Reply::err(code)
    }
}

/// 유일한 편집·판정 지점. 두 입구(터미널 소켓·채팅 패널)가 같은 `apply`를 부른다.
#[derive(Clone)]
pub struct Builder {
    pub(super) host: Arc<dyn Host>,
    pub(super) catalog: Arc<Catalog>,
}

impl Builder {
    pub fn new(host: Arc<dyn Host>) -> Result<Builder, String> {
        let catalog = Catalog::embedded().map_err(|e| format!("노드 정의를 읽지 못했어요: {e}"))?;
        Ok(Builder {
            host,
            catalog: Arc::new(catalog),
        })
    }

    /// 유일한 편집·판정 입구. `ai` = 채팅 패널이 쓰는 키·모델(터미널은 None → `host.ai_default()`).
    pub async fn apply(&self, workflow_id: &str, op: Op, ai: Option<AiDefault>) -> Reply {
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        // 사람의 편집 우선 — 읽지도, baseline을 채우지도, 저장하지도 않는다.
        let changes = op.mutates() || matches!(op, Op::Test { .. } | Op::Finish {});
        if changes && self.host.user_editing(workflow_id) {
            return Reply::err(Code::UserEditing);
        }
        match self.dispatch(workflow_id, op, ai).await {
            Ok(data) => Reply::ok(data),
            Err(reply) => reply,
        }
    }

    /// 사람의 [켜기]/[아직]. `Op`로는 올 수 없다.
    pub async fn confirm_finish(&self, workflow_id: &str, accept: bool) -> Reply {
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        match self.confirm(workflow_id, accept).await {
            Ok(()) => Reply::ok(()),
            Err(reply) => reply,
        }
    }

    /// 저장본과 hash가 같은 확인 대기만 돌려준다. 다르면 지우고 None.
    pub fn pending(&self, workflow_id: &str) -> Option<PendingFinish> {
        let doc = self.host.load(workflow_id).ok()?;
        self.pending_of(workflow_id, &doc)
    }

    async fn dispatch(&self, id: &str, op: Op, ai: Option<AiDefault>) -> Result<Value, Reply> {
        let mut doc = self.load(id)?;
        let state = self.start_chat(id, &doc)?;
        let wf = &mut doc.workflow;
        match op {
            Op::Status {} => {
                let errors = self.errors(&doc.workflow);
                return Ok(edit::status(&doc, &state, errors));
            }
            Op::Events {} => return Ok(edit::events(&self.catalog)),
            Op::Outline { name, steps } => {
                self.update_chat(id, |c| c.outline = steps.clone())?;
                // 이름을 안 바꾸면 저장본은 건드리지 않는다(끄지도 않는다).
                match name.filter(|n| !n.trim().is_empty()) {
                    Some(name) => doc.name = name.trim().to_string(),
                    None => return Ok(Value::Null),
                }
            }
            Op::Add {
                event,
                label,
                after,
                branch,
            } => {
                let ai = match ai {
                    None if event == AI_EVENT => self.host.ai_default().await,
                    ai => ai,
                };
                let out = edit::add(
                    &self.catalog,
                    wf,
                    &event,
                    &label,
                    after.as_deref(),
                    branch,
                    ai,
                )?;
                self.commit(id, doc)?;
                return Ok(out);
            }
            Op::Connect { from, to, branch } => {
                if !edit::connect(&self.catalog, wf, &from, &to, branch)? {
                    return Ok(Value::Null);
                }
            }
            Op::Disconnect { from, to } => edit::disconnect(wf, &from, &to)?,
            Op::Remove { id: node } => edit::remove(&self.catalog, wf, &node)?,
            Op::Rename { id: node, label } => edit::rename(wf, &node, &label)?,
            Op::PickFolder { id: node, field } => {
                let path = self.pick_folder(id, &doc, &node, &field).await?;
                return Ok(json!({"path": path}));
            }
            Op::Fields { id: node } => return Ok(fields::fields(&self.catalog, wf, &node)?),
            Op::Set { id: node, values } => {
                let branches = fields::set(&self.catalog, wf, &node, values)?;
                let doc = self.commit(id, doc)?;
                // 에러가 있어도 저장은 됐다 — 그 노드 것만 돌려준다.
                let errors: Vec<ValidationError> = self
                    .errors(&doc.workflow)
                    .into_iter()
                    .filter(|e| e.node_id.as_deref() == Some(node.as_str()))
                    .collect();
                let mut out = json!({"errors": errors});
                if let Some(branches) = branches {
                    out["branches"] = branches;
                }
                return Ok(out);
            }
            Op::Test { id: node, file } => {
                return self.test(id, &doc, &node, file.as_deref()).await;
            }
            Op::Finish {} => return self.finish(id, &doc, &state).await,
            Op::Guide {} => return Ok(json!({"text": GUIDE})),
        }
        self.commit(id, doc)?;
        Ok(Value::Null)
    }

    pub(super) fn load(&self, id: &str) -> Result<Doc, Reply> {
        Ok(self.host.load(id).map_err(|_| Code::Internal)?)
    }

    /// `ChatState`를 한 번에 읽고-고치고-쓴다.
    pub(super) fn update_chat(
        &self,
        id: &str,
        mut f: impl FnMut(&mut ChatState),
    ) -> Result<ChatState, Reply> {
        let chat = self
            .host
            .update_chat(id, &mut |c| f(&mut c.state))
            .map_err(|_| Code::Internal)?;
        Ok(chat.state)
    }

    /// 채팅 상태. 처음이면 지금 노드들을 baseline으로 적는다(채팅 시작 시점).
    fn start_chat(&self, id: &str, doc: &Doc) -> Result<ChatState, Reply> {
        let state = self.host.load_chat(id).map_err(|_| Code::Internal)?.state;
        if state.baseline.is_some() {
            return Ok(state);
        }
        let base: BTreeMap<String, String> = doc
            .workflow
            .nodes
            .iter()
            .map(|n| (n.id.clone(), node_hash(n)))
            .collect();
        self.update_chat(id, |s| {
            s.baseline.get_or_insert_with(|| base.clone());
        })
    }

    pub(super) fn errors(&self, wf: &Workflow) -> Vec<ValidationError> {
        fields::all_errors(wf, &self.catalog)
    }

    /// 고친 사본을 저장한다 — 워크플로우를 끄고, 없어진 노드의 테스트 기록과 낡은 확인 대기를 지운다.
    pub(super) fn commit(&self, id: &str, mut doc: Doc) -> Result<Doc, Reply> {
        doc.enabled = false;
        let doc = self.host.save(id, doc).map_err(|e| match e {
            SaveError::Stale => Code::Stale,
            // 거부 문구에는 경로가 섞인다 — 응답에 싣지 않는다.
            SaveError::Rejected(_) => Code::SaveRejected,
        })?;
        let hash = workflow_hash(&doc.workflow);
        let ids: HashSet<&str> = doc.workflow.nodes.iter().map(|n| n.id.as_str()).collect();
        self.update_chat(id, |s| {
            s.tests.retain(|node, _| ids.contains(node.as_str()));
            if s.pending_finish.as_ref().is_some_and(|p| p.hash != hash) {
                s.pending_finish = None;
            }
        })?;
        Ok(doc)
    }

    async fn pick_folder(
        &self,
        id: &str,
        doc: &Doc,
        node_id: &str,
        field: &str,
    ) -> Result<PathBuf, Reply> {
        let node = edit::find(&doc.workflow, node_id)?;
        if !edit::is_folder_field(&node.event, field) {
            return Err(Code::NotFolderField.into());
        }
        let (event, title) = (
            node.event.clone(),
            edit::pick_title(&self.catalog, node, field),
        );
        let path = self
            .host
            .pick_folder(id, &title)
            .await
            .ok_or(Code::PickCancelled)?;
        self.host
            .check_folder(&path)
            .map_err(|_| Code::FolderNotAllowed)?;
        // 기다리는 사이(최대 5분) 캔버스가 고쳤을 수 있다 — 저장본을 다시 읽어 그 위에 쓴다.
        let mut doc = self.load(id)?;
        let node = edit::find_mut(&mut doc.workflow, node_id)?;
        if node.event != event {
            return Err(Code::NodeNotFound.into());
        }
        if self.host.user_editing(id) {
            return Err(Code::UserEditing.into());
        }
        let text = path.to_string_lossy().into_owned();
        node.config.insert(field.to_string(), text.into());
        self.commit(id, doc)?;
        Ok(path)
    }
}
