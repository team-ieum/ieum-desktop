use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use ieum_engine::Catalog;

use super::{AiDefault, Code, Host, Op, PendingFinish, Reply};

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

/// 유일한 편집·판정 지점. 두 입구(터미널 소켓·채팅 패널)가 같은 `apply`를 부른다.
#[derive(Clone)]
pub struct Builder {
    // 동작 구현(갈래 A)이 쓴다.
    #[allow(dead_code)]
    host: Arc<dyn Host>,
    #[allow(dead_code)]
    catalog: Arc<Catalog>,
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
    pub async fn apply(&self, workflow_id: &str, _op: Op, _ai: Option<AiDefault>) -> Reply {
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        Reply::err(Code::Internal)
    }

    /// 사람의 [켜기]/[아직]. `Op`로는 올 수 없다.
    pub async fn confirm_finish(&self, workflow_id: &str, _accept: bool) -> Reply {
        let lock = lock_of(workflow_id);
        let _guard = lock.lock().await;
        Reply::err(Code::Internal)
    }

    /// 저장본과 hash가 같은 확인 대기만 돌려준다. 다르면 지우고 None.
    pub fn pending(&self, _workflow_id: &str) -> Option<PendingFinish> {
        None
    }
}
