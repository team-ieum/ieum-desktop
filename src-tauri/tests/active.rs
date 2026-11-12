//! 활성 계정 슬롯. 모든 파일은 임시 폴더 안, AI·키는 가짜.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ieum_desktop_lib::account::{AccountId, account_key, account_root};
use ieum_desktop_lib::active::{Active, BaseDeps};
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, WatchState};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiError, AiProvider, AiRequest, AiRunner, ChunkFn, KeyStore};
use serde_json::json;
use tempfile::TempDir;

struct FakeAi;

#[async_trait]
impl AiProvider for FakeAi {
    async fn ask(&self, _req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<String, AiError> {
        Ok(r#"{"folder": "sorted"}"#.into())
    }
}

#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl KeyStore for FakeKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().insert(account.into(), key.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

struct Env {
    _t: TempDir,
    root: PathBuf,
    app_data: PathBuf,
    settings: Store,
    active: Active,
    events: Arc<Mutex<Vec<AppEvent>>>,
}

impl Env {
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn account_store(&self, id: &AccountId) -> Store {
        Store::new(account_root(
            &self.app_data,
            &account_key(&id.server_url, &id.email),
        ))
    }

    fn runs(&self, id: &AccountId) -> usize {
        self.account_store(id).list_runs(None).unwrap().len()
    }
}

/// 임시 폴더 = 가짜 홈. 앱 데이터는 `data/`.
fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let app_data = root.join("data");
    let settings = Store::new(app_data.clone());
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let active = Active::new(BaseDeps {
        app_data: app_data.clone(),
        settings: settings.clone(),
        home: root.clone(),
        trash_dir: root.join("trash"),
        keys: Arc::new(FakeKeys::default()),
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck: Duration::from_secs(30),
        ai_factory: Arc::new(|_keys| {
            Arc::new(AiRunner::empty().with_provider("fake", Arc::new(FakeAi)))
        }),
    });
    Env {
        _t: t,
        root,
        app_data,
        settings,
        active,
        events,
    }
}

fn acc(email: &str) -> AccountId {
    AccountId {
        email: email.into(),
        server_url: "http://127.0.0.1:1".into(),
    }
}

/// 트리거 → AI만(파일을 건드리지 않는다).
fn ai_only_wf(watch: &Path) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "분류", "outputFields": [{"key": "folder", "type": "text"}]}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap()
}

fn stored(wf: Workflow) -> StoredWorkflow {
    StoredWorkflow {
        id: String::new(),
        name: "정리".into(),
        enabled: true,
        workflow: wf,
        updated_at: String::new(),
    }
}

async fn wait_until(max: Duration, f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + max;
    while Instant::now() < end {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    f()
}

#[tokio::test(flavor = "multi_thread")]
async fn activate_without_network_starts_supervisor() {
    let env = env();
    let a = acc("a@x.com");
    env.active.activate(a.clone()).await.unwrap();
    let inbox = env.dir("in");
    env.active
        .sup()
        .await
        .unwrap()
        .save_workflow(stored(ai_only_wf(&inbox)))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || env.runs(&a) == 1).await);
    assert_eq!(
        env.settings.get_settings().unwrap().current_account,
        Some(a)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn expire_keeps_supervisor_logout_stops() {
    let env = env();
    let a = acc("a@x.com");
    env.active.activate(a.clone()).await.unwrap();
    let inbox = env.dir("in");
    let first = env.active.sup().await.unwrap();
    first.save_workflow(stored(ai_only_wf(&inbox))).unwrap();

    // 만료 뒤 같은 계정 재로그인 — 감시를 다시 시작하지 않는다.
    env.active.activate(a.clone()).await.unwrap();
    assert_eq!(
        first.list_workflows().unwrap()[0].watch,
        WatchState::Watching
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || env.runs(&a) == 1).await);

    env.active.deactivate().await.unwrap();
    assert_eq!(first.list_workflows().unwrap()[0].watch, WatchState::Off);
    assert_eq!(
        env.active.sup().await.err().as_deref(),
        Some("NOT_SIGNED_IN")
    );
    assert_eq!(env.settings.get_settings().unwrap().current_account, None);
    fs::write(inbox.join("b.pdf"), "pdf").unwrap();
    assert!(!wait_until(Duration::from_millis(1500), || env.runs(&a) > 1).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn switch_account_isolates_store_and_stops_old() {
    let env = env();
    let (a, b) = (acc("a@x.com"), acc("b@x.com"));
    env.active.activate(a.clone()).await.unwrap();
    let inbox = env.dir("in");
    env.active
        .sup()
        .await
        .unwrap()
        .save_workflow(stored(ai_only_wf(&inbox)))
        .unwrap();

    env.active.activate(b.clone()).await.unwrap();
    assert!(
        env.active
            .sup()
            .await
            .unwrap()
            .list_workflows()
            .unwrap()
            .is_empty()
    );
    assert_eq!(env.active.current().await, Some(b.clone()));
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(!wait_until(Duration::from_millis(1500), || env.runs(&a) > 0).await);
    assert_eq!(env.runs(&b), 0);

    env.active.activate(a.clone()).await.unwrap();
    assert_eq!(
        env.active
            .sup()
            .await
            .unwrap()
            .list_workflows()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn local_commands_need_account() {
    let env = env();
    assert_eq!(
        env.active.sup().await.err().as_deref(),
        Some("NOT_SIGNED_IN")
    );
    assert_eq!(env.active.current().await, None);
    assert_eq!(env.active.account_key().await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn folder_check_forbids_whole_app_data() {
    let env = env();
    env.active.activate(acc("a@x.com")).await.unwrap();
    let forbidden = env.app_data.join("accounts");
    fs::create_dir_all(&forbidden).unwrap();
    let saved = env
        .active
        .sup()
        .await
        .unwrap()
        .save_workflow(stored(ai_only_wf(&forbidden)));
    assert!(saved.is_err(), "{saved:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn server_url_is_normalized_for_account_key() {
    let env = env();
    let with_slash = AccountId {
        email: "A@x.com".into(),
        server_url: "https://green.kiacms.co.kr/".into(),
    };
    env.active.activate(with_slash).await.unwrap();
    let k1 = env.active.account_key().await.unwrap();
    env.active.deactivate().await.unwrap();
    env.active
        .activate(AccountId {
            email: "a@x.com".into(),
            server_url: "https://green.kiacms.co.kr".into(),
        })
        .await
        .unwrap();
    assert_eq!(env.active.account_key().await.unwrap(), k1);
    assert_eq!(k1, account_key("https://green.kiacms.co.kr", "a@x.com"));
    assert_eq!(
        env.active.current().await.unwrap().server_url,
        "https://green.kiacms.co.kr"
    );
    let bad = env
        .active
        .activate(AccountId {
            email: "a@x.com".into(),
            server_url: "https://evil.example".into(),
        })
        .await;
    assert!(bad.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_same_account_starts_one_supervisor() {
    let env = env();
    let a = acc("a@x.com");
    let inbox = env.dir("in");
    // 계정 폴더에 미리 저장 — 감독이 둘 뜨면 둘 다 감시를 건다.
    let wf = env
        .account_store(&a)
        .save_workflow(stored(ai_only_wf(&inbox)))
        .unwrap();
    let (r1, r2) = tokio::join!(
        env.active.activate(a.clone()),
        env.active.activate(a.clone())
    );
    r1.unwrap();
    r2.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || env.runs(&a) >= 1).await);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(env.runs(&a), 1);
    let started = env
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, AppEvent::Exec { workflow_id, event: ieum_engine::ExecEvent::RunStarted { .. } } if workflow_id == &wf.id))
        .count();
    assert_eq!(started, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn broken_legacy_marker_does_not_block_login() {
    let env = env();
    fs::create_dir_all(env.app_data.join("accounts/.legacy-owner")).unwrap();
    let legacy = env.app_data.join("workflows");
    fs::create_dir_all(&legacy).unwrap();
    let old = legacy.join("11111111-1111-4111-8111-111111111111.json");
    fs::write(&old, "{}").unwrap();

    env.active.activate(acc("a@x.com")).await.unwrap();
    assert!(env.active.sup().await.is_ok());
    assert!(old.exists());
}
