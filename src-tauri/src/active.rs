//! 활성 계정 슬롯: 로그인한 계정 하나의 `Store`·`Supervisor`. 로그인·로그아웃이 갈아 끼운다.
//! 만료는 슬롯을 건드리지 않는다 — 명시적 로그아웃(`deactivate`)만 감시를 멈춘다.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ieum_local::WatchTiming;
use ieum_local::ai::{AiRunner, KeyStore};

use crate::account::{AccountId, account_key, account_root, claim_legacy};
use crate::store::Store;
use crate::supervisor::{Deps, Emit, Supervisor};
use crate::web::validate_server_url;

pub const NOT_SIGNED_IN: &str = "NOT_SIGNED_IN";

pub type AiFactory = Arc<dyn Fn(Arc<dyn KeyStore>) -> Arc<AiRunner> + Send + Sync>;

/// 계정과 무관한 의존성. 계정마다 `Deps`를 여기서 만든다.
pub struct BaseDeps {
    pub app_data: PathBuf,
    /// 앱 데이터 폴더 바로 아래 `settings.json` 전용.
    pub settings: Store,
    pub home: PathBuf,
    pub trash_dir: PathBuf,
    /// 키 저장소(운영 = 키체인). Task 3에서 계정 범위로 감싼다.
    pub keys: Arc<dyn KeyStore>,
    pub emit: Emit,
    pub timing: WatchTiming,
    pub recheck: Duration,
    /// 키 저장소 → AI 러너. 테스트가 가짜 AI를 넣는다.
    pub ai_factory: AiFactory,
}

struct ActiveAccount {
    id: AccountId,
    key: String,
    sup: Supervisor,
}

#[derive(Default)]
struct Slot {
    current: Option<ActiveAccount>,
    /// 이 프로세스에서 감독을 연 적 있는 계정 키 — 끝나지 않은 기록은 처음 열 때만 닫는다.
    opened: HashSet<String>,
}

pub struct Active {
    base: BaseDeps,
    /// activate·deactivate는 이 잠금을 끝까지 쥔다(이중 클릭·동시 로그인).
    slot: tokio::sync::Mutex<Slot>,
}

impl Active {
    pub fn new(base: BaseDeps) -> Self {
        Self {
            base,
            slot: tokio::sync::Mutex::default(),
        }
    }

    pub fn settings(&self) -> &Store {
        &self.base.settings
    }

    /// 같은 계정이 이미 활성이면 그대로 둔다(만료 뒤 재로그인). 다른 계정이면 그 감시를 멈추고 바꾼다.
    /// 네트워크·키체인을 쓰지 않는다. tokio 런타임 안에서 부른다.
    pub async fn activate(&self, mut id: AccountId) -> Result<(), String> {
        id.server_url = validate_server_url(&id.server_url)?;
        let key = account_key(&id.server_url, &id.email);
        let mut slot = self.slot.lock().await;
        if slot.current.as_ref().is_some_and(|a| a.key == key) {
            return Ok(());
        }
        if let Some(old) = slot.current.take() {
            old.sup.stop();
        }
        // 옮기기 실패(표시 파일 손상 등)는 로그인을 막지 않는다 — 옛 데이터는 그대로, 계정은 빈 상태로.
        if let Err(e) = claim_legacy(&self.base.app_data, &key) {
            eprintln!("[ieum] 옛 데이터를 옮기지 못했어요(그대로 둠): {e}");
        }
        let b = &self.base;
        let first_open = !slot.opened.contains(&key);
        let started = Supervisor::start_with(
            Deps {
                store: Store::new(account_root(&b.app_data, &key)),
                app_data: b.app_data.clone(),
                home: b.home.clone(),
                trash_dir: b.trash_dir.clone(),
                ai: (b.ai_factory)(b.keys.clone()),
                emit: b.emit.clone(),
                timing: b.timing,
                recheck: b.recheck,
            },
            first_open,
        );
        let sup = match started {
            Ok(sup) => sup,
            Err(e) => {
                // 슬롯이 비었으니 저장값도 비운다(다음 실행 때 관문).
                if let Err(se) = self.save_current(None) {
                    eprintln!("[ieum] 설정을 저장하지 못했어요: {se}");
                }
                return Err(e);
            }
        };
        slot.opened.insert(key.clone());
        slot.current = Some(ActiveAccount {
            id: id.clone(),
            key,
            sup,
        });
        self.save_current(Some(id))
    }

    /// 명시적 로그아웃: 감시를 멈추고 슬롯을 비운다.
    pub async fn deactivate(&self) -> Result<(), String> {
        let mut slot = self.slot.lock().await;
        if let Some(old) = slot.current.take() {
            old.sup.stop();
        }
        self.save_current(None)
    }

    fn save_current(&self, account: Option<AccountId>) -> Result<(), String> {
        let mut s = self.base.settings.get_settings()?;
        s.current_account = account;
        self.base.settings.set_settings(&s)
    }

    /// 로컬 명령용. 로그인 전이면 `NOT_SIGNED_IN`.
    pub async fn sup(&self) -> Result<Supervisor, String> {
        self.slot
            .lock()
            .await
            .current
            .as_ref()
            .map(|a| a.sup.clone())
            .ok_or_else(|| NOT_SIGNED_IN.to_string())
    }

    pub async fn current(&self) -> Option<AccountId> {
        self.slot
            .lock()
            .await
            .current
            .as_ref()
            .map(|a| a.id.clone())
    }

    pub async fn account_key(&self) -> Option<String> {
        self.slot
            .lock()
            .await
            .current
            .as_ref()
            .map(|a| a.key.clone())
    }
}
