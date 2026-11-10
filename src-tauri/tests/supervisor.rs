//! 실행 감독. 모든 파일은 임시 폴더 안, AI는 가짜 러너.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, QUEUE_CAP, Supervisor, WatchState};
use ieum_engine::{NodeStatus, Workflow};
use ieum_local::WatchTiming;
use ieum_local::ai::{AiError, AiProvider, AiRequest, AiRunner, ChunkFn};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::Semaphore;

/// 가짜 AI 공급원(`fake`): `{"folder": "sorted"}`. 동시 실행 수를 재고, `gate`가 있으면 허가를 하나씩 받는다.
/// 받은 프롬프트를 남긴다(파일 내용이 갔는지 확인).
#[derive(Default)]
struct FakeAi {
    calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    gate: Option<Semaphore>,
    prompts: Mutex<Vec<String>>,
}

#[async_trait]
impl AiProvider for FakeAi {
    async fn ask(&self, req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<String, AiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.prompts.lock().unwrap().push(req.prompt);
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now, Ordering::SeqCst);
        if let Some(g) = &self.gate {
            g.acquire().await.unwrap().forget();
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(r#"{"folder": "sorted"}"#.into())
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
    env_full(ai, recheck, Duration::from_secs(30))
}

/// 임시 폴더 = 가짜 홈. 앱 데이터는 `data/`.
fn env_full(ai: FakeAi, recheck: Duration, repeat_window: Duration) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let store = Store::new(root.join("data"));
    let ai = Arc::new(ai);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(AiRunner::empty().with_provider("fake", ai.clone())),
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window,
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
    // 버린 실행은 이벤트로 알린다.
    let dropped = env
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, AppEvent::QueueDropped { workflow_id, .. } if workflow_id == &id))
        .count();
    assert_eq!(dropped, 2);
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

/// `~/.ssh/id_rsa` 흉내 — 홈 아래지만 워크플로우 폴더 밖.
fn secret(env: &Env) -> PathBuf {
    let p = env.dir("ssh").join("id_rsa");
    fs::write(&p, "-----BEGIN OPENSSH PRIVATE KEY----- SECRET").unwrap();
    p
}

#[tokio::test(flavor = "multi_thread")]
async fn run_now_outside_roots_rejected() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    let key = secret(&env);
    std::os::unix::fs::symlink(&key, inbox.join("innocent.pdf")).unwrap();

    for f in [key.clone(), inbox.join("innocent.pdf")] {
        assert!(env.sup.run_now(&id, &f).await.is_err(), "{}", f.display());
    }
    assert_eq!(runs(&env, &id), 0);
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);
    assert!(key.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_file_outside_roots_rejected() {
    let env = env();
    let inbox = env.dir("in");
    let key = secret(&env);
    // AI 노드 file 칸에 고정 경로(트리거 파일과 무관).
    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": inbox}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "내용 그대로", "file": key}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap();
    let id = save(&env, wf, false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();

    let run_id = env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();
    let run = env.store.get_run(&run_id).unwrap();
    assert_eq!(run.run.nodes["a"].status, NodeStatus::Failed);
    assert_eq!(
        run.run.nodes["a"].error.as_deref(),
        Some("허용된 폴더 밖 파일은 AI에 보낼 수 없어요.")
    );
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn save_rejects_bad_folders_and_provider() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let stored = |wf: Workflow| StoredWorkflow {
        id: String::new(),
        name: "x".into(),
        enabled: true,
        workflow: wf,
        updated_at: String::new(),
    };
    for (watch, base) in [
        (env.root.clone(), out.clone()),    // 홈 자체
        (inbox.clone(), env.dir("data/x")), // 앱 데이터
        (inbox.clone(), env.dir("Library/LaunchAgents")),
        (inbox.clone(), PathBuf::from("/")),
    ] {
        assert!(
            env.sup
                .save_workflow(stored(move_wf(&watch, &base)))
                .is_err()
        );
    }
    let mut wf = move_wf(&inbox, &out);
    wf.nodes[1]
        .config
        .insert("provider".into(), json!("cli:codex"));
    assert!(env.sup.save_workflow(stored(wf)).is_err());
    assert!(env.sup.list_workflows().unwrap().is_empty());

    // 저장소에 직접 심은 정의도 실행 직전에 막는다.
    let planted = env
        .store
        .save_workflow(stored(move_wf(&inbox, &env.root)))
        .unwrap();
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(
        env.sup
            .run_now(&planted.id, &inbox.join("a.pdf"))
            .await
            .is_err()
    );
    assert!(inbox.join("a.pdf").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn disabling_drops_queued_runs() {
    let env = env_with(
        FakeAi {
            gate: Some(Semaphore::new(0)),
            ..Default::default()
        },
        Duration::from_secs(30),
    );
    let id = save(&env, ai_only_wf(&env.dir("in")), true);
    for n in 0..5 {
        env.sup
            .enqueue(&id, json!({"n": n}).as_object().unwrap().clone());
    }
    assert!(
        wait_until(Duration::from_secs(5), || env
            .ai
            .calls
            .load(Ordering::SeqCst)
            == 1)
        .await
    );

    let mut wf = env.store.get_workflow(&id).unwrap();
    wf.enabled = false;
    env.sup.save_workflow(wf).unwrap();
    env.ai.gate.as_ref().unwrap().add_permits(100);

    // 진행 중이던 0번만 끝까지 돈다.
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 1).await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(runs(&env, &id), 1);
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_marks_app_recent_writes() {
    // 반복 창을 짧게 — 되돌린 파일을 막는 게 반복 창이 아니라 앱 전역 RecentWrites여야 한다.
    let env = env_full(
        FakeAi::default(),
        Duration::from_secs(30),
        Duration::from_millis(300),
    );
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), true);
    tokio::time::sleep(Duration::from_millis(200)).await;

    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 1).await);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let run_id = env.store.list_runs(Some(&id)).unwrap()[0].run_id.clone();
    assert_eq!(env.sup.undo_run(&run_id).unwrap().restored.len(), 1);
    assert!(inbox.join("a.pdf").exists());
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(runs(&env, &id), 1, "되돌린 파일이 감시를 다시 일으켰다");
    assert!(inbox.join("a.pdf").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn journal_not_shared_between_runs() {
    let env = env();
    let mut jobs = Vec::new();
    for w in 0..2 {
        let (inbox, out) = (env.dir(&format!("in{w}")), env.dir(&format!("out{w}")));
        let id = save(&env, move_wf(&inbox, &out), false);
        for n in 0..3 {
            let f = inbox.join(format!("f{n}.pdf"));
            fs::write(&f, "pdf").unwrap();
            let (sup, id) = (env.sup.clone(), id.clone());
            jobs.push(tokio::spawn(async move { sup.run_now(&id, &f).await }));
        }
    }
    for j in jobs {
        let run_id = j.await.unwrap().unwrap();
        assert_eq!(env.store.get_run(&run_id).unwrap().file_ops.len(), 1);
    }
}
