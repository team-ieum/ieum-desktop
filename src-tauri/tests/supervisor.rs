//! 실행 감독. 모든 파일은 임시 폴더 안, AI는 가짜 러너.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, QUEUE_CAP, Supervisor, WatchState};
use ieum_engine::{NodeCtx, NodeError, NodeRunner, Workflow};
use ieum_local::WatchTiming;
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::sync::Semaphore;

/// 가짜 AI: `{"folder": "sorted"}`. 동시 실행 수를 재고, `gate`가 있으면 허가를 하나씩 받는다.
#[derive(Default)]
struct FakeAi {
    calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    gate: Option<Semaphore>,
}

#[async_trait]
impl NodeRunner for FakeAi {
    async fn run(&self, _ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now, Ordering::SeqCst);
        if let Some(g) = &self.gate {
            g.acquire().await.unwrap().forget();
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(json!({"folder": "sorted"}).as_object().unwrap().clone())
    }
}

struct Env {
    _t: TempDir,
    root: PathBuf,
    sup: Supervisor,
    store: Store,
    ai: Arc<FakeAi>,
    events: Arc<Mutex<Vec<AppEvent>>>,
}

impl Env {
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

fn env_with(ai: FakeAi, recheck: Duration) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let store = Store::new(root.join("data"));
    let ai = Arc::new(ai);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        trash_dir: root.join("trash"),
        ai: ai.clone(),
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck,
    })
    .unwrap();
    Env {
        _t: t,
        root,
        sup,
        store,
        ai,
        events,
    }
}

fn env() -> Env {
    env_with(FakeAi::default(), Duration::from_secs(30))
}

/// 트리거(watch) → AI → base 아래 `{{folder}}`로 옮기기.
fn move_wf(watch: &Path, base: &Path) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "분류", "outputFields": [{"key": "folder", "type": "text"}]}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}", "baseFolder": base, "destination": "{{nodes.a.output.folder}}"}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}, {"id": "e2", "source": "a", "target": "m"}]
    }))
    .unwrap()
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

fn save(env: &Env, wf: Workflow, enabled: bool) -> String {
    env.sup
        .save_workflow(StoredWorkflow {
            id: String::new(),
            name: "정리".into(),
            enabled,
            workflow: wf,
            updated_at: String::new(),
        })
        .unwrap()
        .id
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

fn runs(env: &Env, id: &str) -> usize {
    env.store.list_runs(Some(id)).unwrap().len()
}

fn status_events(env: &Env, id: &str, ok: bool) -> usize {
    env.events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, AppEvent::WatchStatus { workflow_id, ok: o, .. } if workflow_id == id && *o == ok))
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn supervisor_runs_on_new_file() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), true);
    tokio::time::sleep(Duration::from_millis(200)).await;

    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 1).await);

    let summary = &env.store.list_runs(Some(&id)).unwrap()[0];
    let run = env.store.get_run(&summary.run_id).unwrap();
    assert_eq!(run.workflow_id, id);
    assert_eq!(run.file_ops.len(), 1);
    assert!(out.join("sorted/a.pdf").exists());
    assert!(run.allowed_roots.contains(&inbox));
    let exec = env
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, AppEvent::Exec { workflow_id, .. } if workflow_id == &id))
        .count();
    assert!(exec > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn supervisor_queues_same_workflow() {
    let env = env();
    let id = save(&env, ai_only_wf(&env.dir("in")), false);
    for n in 0..3 {
        env.sup
            .enqueue(&id, json!({"n": n}).as_object().unwrap().clone());
    }
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 3).await);
    assert_eq!(env.ai.max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_cap_drops_oldest() {
    let env = env_with(
        FakeAi {
            gate: Some(Semaphore::new(0)),
            ..Default::default()
        },
        Duration::from_secs(30),
    );
    let id = save(&env, ai_only_wf(&env.dir("in")), false);
    let job = |n: usize| json!({"n": n}).as_object().unwrap().clone();

    env.sup.enqueue(&id, job(0));
    assert!(
        wait_until(Duration::from_secs(5), || env
            .ai
            .calls
            .load(Ordering::SeqCst)
            == 1)
        .await
    );
    // 0번이 도는 동안 QUEUE_CAP + 2개 → 가장 오래된 1·2번이 버려진다.
    for n in 1..=QUEUE_CAP + 2 {
        env.sup.enqueue(&id, job(n));
    }
    env.ai.gate.as_ref().unwrap().add_permits(1000);
    assert!(wait_until(Duration::from_secs(20), || runs(&env, &id) == QUEUE_CAP + 1).await);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut ns: Vec<u64> = env
        .store
        .list_runs(Some(&id))
        .unwrap()
        .iter()
        .map(|s| {
            let run = env.store.get_run(&s.run_id).unwrap();
            run.run.nodes["t"].output.as_ref().unwrap()["n"]
                .as_u64()
                .unwrap()
        })
        .collect();
    ns.sort();
    let want: Vec<u64> = std::iter::once(0)
        .chain(3..=(QUEUE_CAP as u64 + 2))
        .collect();
    assert_eq!(ns, want);
}

#[tokio::test(flavor = "multi_thread")]
async fn disabled_workflow_not_watched() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    tokio::time::sleep(Duration::from_millis(200)).await;

    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(runs(&env, &id), 0);
    assert!(inbox.join("a.pdf").exists());
    let entry = env.sup.list_workflows().unwrap().remove(0);
    assert_eq!(entry.watch, WatchState::Off);
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_run_twice_errors() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();

    let run_id = env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();
    assert!(out.join("sorted/a.pdf").exists());

    let report = env.sup.undo_run(&run_id).unwrap();
    assert_eq!(report.restored.len(), 1);
    assert!(inbox.join("a.pdf").exists());
    assert!(env.store.get_run(&run_id).unwrap().undo.is_some());
    assert!(env.sup.undo_run(&run_id).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_uses_stored_roots_not_current() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let run_id = env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();

    // 실행 뒤 워크플로우의 폴더를 전혀 다른 곳으로 바꾼다.
    let mut wf = env.store.get_workflow(&id).unwrap();
    wf.workflow = move_wf(&env.dir("other-in"), &env.dir("other-out"));
    env.sup.save_workflow(wf).unwrap();

    let report = env.sup.undo_run(&run_id).unwrap();
    assert_eq!(report.restored.len(), 1, "{report:?}");
    assert!(inbox.join("a.pdf").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_error_reported_and_recovers() {
    let env = env_with(FakeAi::default(), Duration::from_millis(200));
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), true);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        env.sup.list_workflows().unwrap()[0].watch,
        WatchState::Watching
    );

    fs::remove_dir_all(&inbox).unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || status_events(&env, &id, false)
            == 1)
        .await
    );
    assert_eq!(
        env.sup.list_workflows().unwrap()[0].watch,
        WatchState::WatchError
    );

    fs::create_dir(&inbox).unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || status_events(&env, &id, true)
            == 1)
        .await
    );
    assert_eq!(
        env.sup.list_workflows().unwrap()[0].watch,
        WatchState::Watching
    );

    // 다시 건 감시가 실제로 동작한다.
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox.join("b.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 1).await);
}
