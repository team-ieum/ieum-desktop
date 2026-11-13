//! 채팅 빌더(`ieum_local::builder`)의 앱 쪽 Host — 저장소·감독·폴더 고르기 창을 잇는다.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::Workflow;
use ieum_local::builder::{AiDefault, Builder, ChatFile, Doc, Host, SaveError, TestRun};

use crate::active::Active;
use crate::credentials::AccountCredentials;
use crate::store::StoredWorkflow;
use crate::supervisor::{AppEvent, STALE, Supervisor};

/// 폴더 고르기 창을 띄우는 함수. 인자 = (워크플로우 id, 창 제목). 취소·시간 초과 = None.
#[derive(Clone)]
pub struct PickFolder(
    #[allow(clippy::type_complexity)]
    pub  Arc<
        dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send>>
            + Send
            + Sync,
    >,
);

/// 항상 None — 실제 창은 `lib.rs`에서 바꿔 끼운다.
pub fn no_pick() -> PickFolder {
    PickFolder(Arc::new(|_, _| Box::pin(async { None })))
}

pub struct AppHost {
    sup: Supervisor,
    // AI 연결 확인(`ai_default`·`ai_ready`)이 쓴다.
    #[allow(dead_code)]
    creds: AccountCredentials,
    pick: PickFolder,
}

impl AppHost {
    pub fn new(sup: Supervisor, creds: AccountCredentials, pick: PickFolder) -> Self {
        AppHost { sup, creds, pick }
    }
}

#[async_trait]
impl Host for AppHost {
    fn load(&self, workflow_id: &str) -> Result<Doc, String> {
        let wf = self.sup.store().get_workflow(workflow_id)?;
        Ok(Doc {
            name: wf.name,
            enabled: wf.enabled,
            workflow: wf.workflow,
            updated_at: wf.updated_at,
        })
    }

    fn save(&self, workflow_id: &str, doc: Doc) -> Result<Doc, SaveError> {
        let wf = StoredWorkflow {
            id: workflow_id.to_string(),
            name: doc.name,
            enabled: doc.enabled,
            workflow: doc.workflow,
            updated_at: doc.updated_at.clone(),
        };
        let saved = self
            .sup
            .save_workflow_expecting(wf, &doc.updated_at)
            .map_err(|e| match e.as_str() {
                STALE => SaveError::Stale,
                _ => SaveError::Rejected(e),
            })?;
        self.sup.emit(AppEvent::WorkflowChanged {
            workflow_id: saved.id,
            updated_at: saved.updated_at.clone(),
        });
        Ok(Doc {
            name: saved.name,
            enabled: saved.enabled,
            workflow: saved.workflow,
            updated_at: saved.updated_at,
        })
    }

    fn load_chat(&self, workflow_id: &str) -> Result<ChatFile, String> {
        self.sup.store().get_chat(workflow_id)
    }

    fn update_chat(
        &self,
        workflow_id: &str,
        f: &mut dyn FnMut(&mut ChatFile),
    ) -> Result<ChatFile, String> {
        self.sup.store().update_chat(workflow_id, f)
    }

    fn check_folder(&self, path: &Path) -> Result<(), String> {
        self.sup.check_user_folder(path)
    }

    fn user_editing(&self, workflow_id: &str) -> bool {
        self.sup.user_editing(workflow_id)
    }

    async fn pick_folder(&self, workflow_id: &str, title: &str) -> Option<PathBuf> {
        (self.pick.0)(workflow_id.to_string(), title.to_string()).await
    }

    async fn ai_default(&self) -> Option<AiDefault> {
        None
    }

    async fn ai_ready(&self, _provider: &str, _credential: Option<&str>) -> bool {
        false
    }

    async fn run_test(
        &self,
        _workflow_id: &str,
        _wf: Workflow,
        _sample: &Path,
    ) -> Result<TestRun, String> {
        Err("미구현".into())
    }

    fn finish_pending(&self, workflow_id: &str, workflow_name: &str) {
        self.sup.emit(AppEvent::FinishPending {
            workflow_id: workflow_id.to_string(),
            workflow_name: workflow_name.to_string(),
        });
    }
}

/// 활성 계정의 Builder. 로그인 전이면 `NOT_SIGNED_IN`.
pub async fn builder_of(active: &Active, pick: &PickFolder) -> Result<Builder, String> {
    Builder::new(Arc::new(AppHost::new(
        active.sup().await?,
        active.credentials().await?,
        pick.clone(),
    )))
}
