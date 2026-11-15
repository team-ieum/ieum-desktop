//! 채팅 빌더(`ieum_local::builder`)의 앱 쪽 Host — 저장소·감독·폴더 고르기 창을 잇는다.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::Workflow;
use ieum_local::ai::detect_clis;
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

/// 터미널용 기본 AI 연결을 고르는 순서(설계 3절). 러너에 등록된 것 중 연결된 첫 번째를 쓴다.
pub const AI_ORDER: [&str; 6] = [
    "cli:claude",
    "cli:codex",
    "cli:agy",
    "api:anthropic",
    "api:openai",
    "api:gemini",
];

type DetectClis = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

pub struct AppHost {
    sup: Supervisor,
    creds: AccountCredentials,
    pick: PickFolder,
    /// 설치된 구독 CLI의 공급원 id(`cli:claude` 등).
    clis: DetectClis,
}

impl AppHost {
    pub fn new(sup: Supervisor, creds: AccountCredentials, pick: PickFolder) -> Self {
        let clis = || {
            detect_clis()
                .into_iter()
                .map(|c| c.id.to_string())
                .collect()
        };
        AppHost {
            sup,
            creds,
            pick,
            clis: Arc::new(clis),
        }
    }

    /// CLI 감지를 바꿔 끼운다(테스트용).
    pub fn with_clis(mut self, detect: DetectClis) -> Self {
        self.clis = detect;
        self
    }
}

/// `AI_ORDER`에서 러너에 등록돼 있고 연결된 첫 번째. 키체인·셸을 읽으니 blocking 스레드에서 부른다.
fn pick_ai_default(
    sup: &Supervisor,
    creds: &AccountCredentials,
    clis: &DetectClis,
) -> Option<AiDefault> {
    let listed = creds.list();
    let mut detected: Option<Vec<String>> = None; // CLI 감지는 느리다 — 필요할 때 한 번만
    for id in AI_ORDER {
        if !sup.ai().has_provider(id) {
            continue;
        }
        let credential = match id.strip_prefix("api:") {
            Some(provider) => {
                let Some(keys) = listed.api_keys.get(provider).filter(|k| !k.is_empty()) else {
                    continue;
                };
                keys.iter().find(|k| k.is_default).map(|k| k.id.clone())
            }
            None => {
                if !detected
                    .get_or_insert_with(|| clis())
                    .iter()
                    .any(|c| c == id)
                {
                    continue;
                }
                let accounts = match id {
                    "cli:claude" => listed.claude_accounts.as_slice(),
                    "cli:codex" => listed.codex_accounts.as_slice(),
                    _ => &[], // Antigravity는 계정 목록이 없다 — 이 Mac 로그인 하나.
                };
                let default = accounts.iter().find(|a| a.is_default).map(|a| a.id.clone());
                // 기본 계정이 없으면(이 Mac 로그인 숨김 등) 실행 때 AI_NOT_CONNECTED — 다음 공급원으로.
                if default.is_none() && matches!(id, "cli:claude" | "cli:codex") {
                    continue;
                }
                default
            }
        };
        return Some(AiDefault {
            provider: id.to_string(),
            credential,
            model: None,
            effort: None,
        });
    }
    None
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
        let (sup, creds, clis) = (self.sup.clone(), self.creds.clone(), self.clis.clone());
        tokio::task::spawn_blocking(move || pick_ai_default(&sup, &creds, &clis))
            .await
            .ok()?
    }

    async fn ai_ready(&self, provider: &str, credential: Option<&str>) -> bool {
        if provider.is_empty() {
            return false;
        }
        let (sup, provider) = (self.sup.clone(), provider.to_string());
        let credential = credential.map(str::to_string);
        tokio::task::spawn_blocking(move || sup.ai().can_resolve(&provider, credential.as_deref()))
            .await
            .unwrap_or(false)
    }

    async fn run_test(
        &self,
        workflow_id: &str,
        wf: Workflow,
        sample: &Path,
    ) -> Result<TestRun, String> {
        self.sup.run_test(workflow_id, wf, sample).await
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
