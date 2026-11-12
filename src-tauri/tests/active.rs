//! 활성 계정 슬롯. 모든 파일은 임시 폴더 안, AI·키는 가짜.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ieum_desktop_lib::account::{AccountId, account_key, account_root};
use ieum_desktop_lib::active::{Active, AiFactory, BaseDeps};
use ieum_desktop_lib::commands::save_screen_settings;
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, WatchState};
use ieum_engine::{RunStatus, Workflow};
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiError, AiProvider, AiRequest, AiRunner, ChunkFn, KeyStore};
use serde_json::json;
use std::future::Future;
use tempfile::TempDir;
use tokio::sync::Semaphore;

/// `gate`가 있으면 허가를 하나씩 받아야 답한다(도는 실행을 붙잡기).
struct FakeAi {
    gate: Option<Arc<Semaphore>>,
}

#[async_trait]
impl AiProvider for FakeAi {
    async fn ask(&self, _req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<String, AiError> {
        if let Some(g) = &self.gate {
            g.acquire().await.unwrap().forget();
        }
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
    /// `ai_factory` 호출 수 = Supervisor 시작 수.
    starts: Arc<AtomicUsize>,
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
    env_gated(None)
}

fn env_gated(gate: Option<Arc<Semaphore>>) -> Env {
    env_with(
        Arc::new(FakeKeys::default()),
        Arc::new(move |_keys| {
            let ai = FakeAi { gate: gate.clone() };
            Arc::new(AiRunner::empty().with_provider("fake", Arc::new(ai)))
        }),
    )
}

fn env_with(keys: Arc<FakeKeys>, ai_factory: AiFactory) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let app_data = root.join("data");
    let settings = Store::new(app_data.clone());
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let starts = Arc::new(AtomicUsize::new(0));
    let counter = starts.clone();
    let active = Active::new(BaseDeps {
        app_data: app_data.clone(),
        settings: settings.clone(),
        home: root.clone(),
        trash_dir: root.join("trash"),
        keys,
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck: Duration::from_secs(30),
        ai_factory: Arc::new(move |keys| {
            counter.fetch_add(1, Ordering::SeqCst);
            ai_factory(keys)
        }),
    });
    Env {
        _t: t,
        root,
        app_data,
        settings,
        active,
        events,
        starts,
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

/// 옛 키 옮기기가 끝난 뒤 범위 없는 옛 키가 다시 생겨도 주인이 아닌 B는 가져가지 않는다.
#[tokio::test(flavor = "multi_thread")]
async fn legacy_keys_never_go_to_second_account() {
    let inner = Arc::new(FakeKeys::default());
    inner.set("anthropic", "sk-a").unwrap();
    let env = env_with(
        inner.clone(),
        Arc::new(|k| Arc::new(AiRunner::with_keys(k))),
    );
    let (a, b) = (acc("a@x.com"), acc("b@x.com"));
    let (ka, kb) = (
        account_key(&a.server_url, &a.email),
        account_key(&b.server_url, &b.email),
    );
    env.active.activate(a).await.unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || inner
            .get(&format!("anthropic.{ka}"))
            .unwrap()
            .is_some())
        .await
    );
    inner.set("anthropic", "sk-late").unwrap();
    env.active.activate(b).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(inner.get(&format!("anthropic.{kb}")).unwrap(), None);
}

/// 옛 이름 삭제가 실패해 옛 키가 남아도, 표시가 있으면 다시 옮기지 않는다(사용자가 지운 키가 되살아나지 않게).
#[tokio::test(flavor = "multi_thread")]
async fn migrated_keys_are_not_moved_again() {
    let inner = Arc::new(FakeKeys::default());
    inner.set("openai", "sk-o").unwrap();
    let env = env_with(
        inner.clone(),
        Arc::new(|k| Arc::new(AiRunner::with_keys(k))),
    );
    let a = acc("a@x.com");
    let ka = account_key(&a.server_url, &a.email);
    let scoped = format!("openai.{ka}");
    env.active.activate(a.clone()).await.unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || inner
            .get(&scoped)
            .unwrap()
            .is_some())
        .await
    );
    assert!(
        account_root(&env.app_data, &ka)
            .join("keys-migrated")
            .exists()
    );
    // 삭제 실패 흉내: 옛 키가 남았고, 사용자는 계정 키를 지웠다.
    inner.set("openai", "sk-o").unwrap();
    inner.delete(&scoped).unwrap();
    env.active.deactivate().await.unwrap();
    env.active.activate(a).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(inner.get(&scoped).unwrap(), None);
}

/// 옛 키는 첫 계정(A)으로 옮겨지고, B의 AI 노드는 A의 키를 못 써 "키 없음"으로 실패한다.
#[tokio::test(flavor = "multi_thread")]
async fn activate_runs_ai_with_account_keys() {
    let inner = Arc::new(FakeKeys::default());
    inner.set("anthropic", "sk-a").unwrap();
    let given: Arc<Mutex<Vec<Arc<dyn KeyStore>>>> = Arc::default();
    let g = given.clone();
    let env = env_with(
        inner.clone(),
        Arc::new(move |keys| {
            g.lock().unwrap().push(keys.clone());
            Arc::new(AiRunner::with_keys(keys))
        }),
    );
    let (a, b) = (acc("a@x.com"), acc("b@x.com"));
    let ka = account_key(&a.server_url, &a.email);

    env.active.activate(a.clone()).await.unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || inner
            .get(&format!("anthropic.{ka}"))
            .unwrap()
            .is_some())
        .await
    );
    assert_eq!(inner.get("anthropic").unwrap(), None);
    assert_eq!(
        given.lock().unwrap()[0]
            .get("anthropic")
            .unwrap()
            .as_deref(),
        Some("sk-a")
    );

    env.active.activate(b.clone()).await.unwrap();
    assert_eq!(given.lock().unwrap()[1].get("anthropic").unwrap(), None);
    let inbox = env.dir("in");
    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": inbox}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "api:anthropic", "prompt": "분류"}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap();
    env.active
        .sup()
        .await
        .unwrap()
        .save_workflow(stored(wf))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let store = env.account_store(&b);
    let finished = || {
        store
            .list_runs(None)
            .unwrap()
            .first()
            .is_some_and(|r| !r.finished_at.is_empty())
    };
    assert!(wait_until(Duration::from_secs(5), finished).await);
    let run = store.list_runs(None).unwrap()[0].clone();
    assert_eq!(run.status, RunStatus::Failed);
    let detail = serde_json::to_string(&store.get_run(&run.run_id).unwrap()).unwrap();
    assert!(detail.contains("API 키가 없어요"), "{detail}");
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
    assert_eq!(env.starts.load(Ordering::SeqCst), 1);
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

/// 트리거 → `out/sorted`로 옮기기 → (붙잡히는) AI. 옮긴 뒤 끝나지 않은 기록이 남는다.
fn move_then_ai_wf(watch: &Path, base: &Path) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}", "baseFolder": base, "destination": "sorted"}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "분류", "outputFields": [{"key": "folder", "type": "text"}]}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "m"}, {"id": "e2", "source": "m", "target": "a"}]
    }))
    .unwrap()
}

/// 실행이 도는 중에 같은 계정을 다시 활성화해도 그 기록을 닫지 않는다(되돌리기는 끝난 뒤 한 번만).
async fn reactivate_keeps_running_record(via_other: bool) {
    let gate = Arc::new(Semaphore::new(0));
    let env = env_gated(Some(gate.clone()));
    let a = acc("a@x.com");
    env.active.activate(a.clone()).await.unwrap();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    env.active
        .sup()
        .await
        .unwrap()
        .save_workflow(stored(move_then_ai_wf(&inbox, &out)))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let store = env.account_store(&a);
    let running = || {
        store
            .list_runs(None)
            .unwrap()
            .first()
            .map(|r| r.finished_at.is_empty())
    };
    assert!(wait_until(Duration::from_secs(5), || running() == Some(true)).await);
    let run_id = store.list_runs(None).unwrap()[0].run_id.clone();

    if via_other {
        env.active.activate(acc("b@x.com")).await.unwrap();
    } else {
        env.active.deactivate().await.unwrap();
    }
    env.active.activate(a.clone()).await.unwrap();
    assert_eq!(running(), Some(true));
    let sup = env.active.sup().await.unwrap();
    assert!(sup.undo_run(&run_id).is_err());
    assert!(out.join("sorted/a.pdf").exists());

    gate.add_permits(1);
    assert!(wait_until(Duration::from_secs(5), || running() == Some(false)).await);
    assert_eq!(
        store.get_run(&run_id).unwrap().run.status,
        RunStatus::Succeeded
    );
    sup.undo_run(&run_id).unwrap();
    assert!(inbox.join("a.pdf").exists());
    assert!(sup.undo_run(&run_id).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn relogin_does_not_close_running_record() {
    reactivate_keeps_running_record(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn switch_back_does_not_close_running_record() {
    reactivate_keeps_running_record(true).await;
}

/// 다른 계정의 감독 시작이 실패하면 슬롯처럼 저장값도 비운다(다음 실행 때 관문).
#[test]
fn failed_switch_clears_current_account() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let env = env();
    rt.block_on(env.active.activate(acc("a@x.com"))).unwrap();
    // tokio 런타임 밖에서 폴링 → Supervisor::start가 런타임을 못 찾아 실패.
    let mut fut = std::pin::pin!(env.active.activate(acc("b@x.com")));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match fut.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(r) => assert!(r.is_err()),
        std::task::Poll::Pending => panic!("잠금이 비어 있으면 바로 끝나야 해요"),
    }
    assert_eq!(rt.block_on(env.active.current()), None);
    assert_eq!(env.settings.get_settings().unwrap().current_account, None);
}

#[test]
fn screen_settings_cannot_change_current_account() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let mut s = store.get_settings().unwrap();
    s.current_account = Some(acc("a@x.com"));
    store.set_settings(&s).unwrap();

    let mut from_screen = s.clone();
    from_screen.current_account = Some(acc("evil@x.com"));
    from_screen.notifications_seen_at = "2026-11-12T00:00:00.000Z".into();
    save_screen_settings(&store, &FakeKeys::default(), from_screen).unwrap();
    let saved = store.get_settings().unwrap();
    assert_eq!(saved.current_account, Some(acc("a@x.com")));
    assert_eq!(saved.notifications_seen_at, "2026-11-12T00:00:00.000Z");
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
