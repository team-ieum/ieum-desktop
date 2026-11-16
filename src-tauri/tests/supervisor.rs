//! 실행 감독. 모든 파일은 임시 폴더 안, AI는 가짜 러너.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ieum_desktop_lib::store::{RunTrigger, Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{
    AppEvent, Deps, NoticeKind, QUEUE_CAP, RunNotice, Supervisor, WatchState,
};
use ieum_engine::{ExecEvent, NodeStatus, RunStatus, Workflow};
use ieum_local::WatchTiming;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, ChunkFn};
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
    async fn ask(&self, req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
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
        Ok(AiAnswer {
            text: r#"{"folder": "sorted"}"#.into(),
            usage: None,
        })
    }
}

struct Env {
    _t: TempDir,
    root: PathBuf,
    sup: Supervisor,
    store: Store,
    ai: Arc<FakeAi>,
    events: Arc<Mutex<Vec<AppEvent>>>,
    /// 가짜 시계(`Deps.now`).
    clock: Arc<Mutex<DateTime<Utc>>>,
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
    let clock = Arc::new(Mutex::new(utc("2026-11-16T01:30:00Z")));
    let c = clock.clone();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
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
        now: Arc::new(move || *c.lock().unwrap()),
    })
    .unwrap();
    Env {
        _t: t,
        root,
        sup,
        store,
        ai,
        events,
        clock,
    }
}

fn utc(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
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
    let inbox = env.dir("in");
    let id = save(&env, ai_only_wf(&inbox), true);
    for n in 0..3 {
        env.sup
            .enqueue(&id, &inbox, json!({"n": n}).as_object().unwrap().clone());
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
    let inbox = env.dir("in");
    let id = save(&env, ai_only_wf(&inbox), true);
    let job = |n: usize| json!({"n": n}).as_object().unwrap().clone();

    env.sup.enqueue(&id, &inbox, job(0));
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
        env.sup.enqueue(&id, &inbox, job(n));
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
async fn watch_run_records_watch_trigger() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), true);
    tokio::time::sleep(Duration::from_millis(200)).await;

    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 1).await);
    let summary = &env.store.list_runs(Some(&id)).unwrap()[0];
    assert_eq!(summary.trigger, Some(RunTrigger::Watch));
    let run = env.store.get_run(&summary.run_id).unwrap();
    assert_eq!(run.trigger, Some(RunTrigger::Watch));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_now_records_manual_trigger() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();

    let run_id = env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();
    let run = env.store.get_run(&run_id).unwrap();
    assert_eq!(run.trigger, Some(RunTrigger::Manual));
    assert_eq!(
        env.store.list_runs(Some(&id)).unwrap()[0].trigger,
        Some(RunTrigger::Manual)
    );
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
    let inbox = env.dir("in");
    let id = save(&env, ai_only_wf(&inbox), true);
    for n in 0..5 {
        env.sup
            .enqueue(&id, &inbox, json!({"n": n}).as_object().unwrap().clone());
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

/// 감시가 보내는 것과 같은 트리거 출력.
fn trigger(p: &Path) -> serde_json::Map<String, serde_json::Value> {
    json!({"path": p, "name": p.file_name().unwrap().to_string_lossy(), "extension": "pdf", "size": 3})
        .as_object()
        .unwrap()
        .clone()
}

/// 같은 앱 데이터로 앱을 다시 켠다.
fn restart(env: &Env) -> Supervisor {
    Supervisor::start(Deps {
        store: env.store.clone(),
        app_data: env.root.join("data"),
        home: env.root.clone(),
        trash_dir: env.root.join("trash"),
        ai: Arc::new(AiRunner::empty()),
        emit: Arc::new(|_| {}),
        timing: WatchTiming::default(),
        recheck: Duration::from_secs(30),
        now: Arc::new(Utc::now),
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn file_op_recorded_while_run_in_flight() {
    let env = env_with(
        FakeAi {
            gate: Some(Semaphore::new(0)),
            ..Default::default()
        },
        Duration::from_secs(30),
    );
    let inbox = env.dir("in");
    // 트리거 → 이름 바꾸기 → AI(멈춤).
    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": inbox}},
            {"id": "r", "type": "ACTION", "event": "local.file_rename", "config": {"path": "{{nodes.t.output.path}}", "newName": "renamed"}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "분류", "outputFields": [{"key": "folder", "type": "text"}]}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "r"}, {"id": "e2", "source": "r", "target": "a"}]
    }))
    .unwrap();
    let id = save(&env, wf, false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let (sup, wid, f) = (env.sup.clone(), id.clone(), inbox.join("a.pdf"));
    tokio::spawn(async move { sup.run_now(&wid, &f).await });
    assert!(
        wait_until(Duration::from_secs(5), || env
            .ai
            .calls
            .load(Ordering::SeqCst)
            == 1)
        .await
    );
    assert!(inbox.join("renamed.pdf").exists());

    // AI가 도는 동안에도 파일 작업이 기록에 있다. 도중 기록은 AI 호출과 따로 써지니 기다린다.
    let recorded = || {
        env.store.list_runs(Some(&id)).is_ok_and(|l| {
            l.first().is_some_and(|r| {
                env.store
                    .get_run(&r.run_id)
                    .is_ok_and(|run| run.file_ops.len() == 1)
            })
        })
    };
    assert!(wait_until(Duration::from_secs(5), recorded).await);
    let list = env.store.list_runs(Some(&id)).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].finished_at.is_empty());
    let run_id = list[0].run_id.clone();
    assert_eq!(env.store.get_run(&run_id).unwrap().file_ops.len(), 1);
    // 도는 중인 실행은 되돌리지 않는다(끝날 때 기록을 다시 쓴다).
    assert!(env.sup.undo_run(&run_id).is_err());

    // 여기서 앱이 죽었다가 다시 켜지면: 끝나지 않은 기록은 중단됨으로 닫히고 되돌릴 수 있다.
    let sup2 = restart(&env);
    let run = env.store.get_run(&run_id).unwrap();
    assert_eq!(run.run.status, RunStatus::Cancelled);
    assert!(!run.finished_at.is_empty());
    assert_eq!(sup2.undo_run(&run_id).unwrap().restored.len(), 1);
    assert!(inbox.join("a.pdf").exists());
}

/// 재리뷰 N-2: 화면이 `runFinished`를 받고 곧장 기록을 읽어도 도중 기록(중단됨·끝 시각 없음)이 아니라 최종 기록이다.
#[tokio::test(flavor = "multi_thread")]
async fn run_finished_emitted_after_final_save() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (store, s) = (env.store.clone(), seen.clone());
    let sup = Supervisor::start(Deps {
        store: env.store.clone(),
        app_data: env.root.join("data"),
        home: env.root.clone(),
        trash_dir: env.root.join("trash"),
        ai: Arc::new(AiRunner::empty().with_provider("fake", env.ai.clone())),
        emit: Arc::new(move |e| {
            if let AppEvent::Exec {
                event: ExecEvent::RunFinished { .. },
                workflow_id,
            } = e
            {
                let list = store.list_runs(Some(&workflow_id)).unwrap();
                s.lock().unwrap().extend(
                    list.into_iter()
                        .map(|r| (r.status, r.finished_at.is_empty())),
                );
            }
        }),
        timing: WatchTiming::default(),
        recheck: Duration::from_secs(30),
        now: Arc::new(Utc::now),
    })
    .unwrap();
    sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();
    assert_eq!(*seen.lock().unwrap(), [(RunStatus::Succeeded, false)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn late_trigger_after_disable_or_folder_change_dropped() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    // 감시 시작 전 파일은 감시가 보지 않는다.
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let id = save(&env, move_wf(&inbox, &out), true);

    // 끈 뒤 늦게 도착한 트리거.
    let mut wf = env.store.get_workflow(&id).unwrap();
    wf.enabled = false;
    env.sup.save_workflow(wf).unwrap();
    env.sup.enqueue(&id, &inbox, trigger(&inbox.join("a.pdf")));
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(runs(&env, &id), 0);

    // 폴더를 바꾼 뒤 옛 폴더에서 늦게 도착한 트리거.
    let other = env.dir("other");
    let mut wf = env.store.get_workflow(&id).unwrap();
    (wf.enabled, wf.workflow) = (true, move_wf(&other, &out));
    env.sup.save_workflow(wf).unwrap();
    env.sup.enqueue(&id, &inbox, trigger(&inbox.join("a.pdf")));
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(runs(&env, &id), 0);
    assert!(inbox.join("a.pdf").exists());
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);

    // 지금 폴더에서 온 트리거는 돈다.
    fs::write(other.join("b.pdf"), "pdf").unwrap();
    env.sup.enqueue(&id, &other, trigger(&other.join("b.pdf")));
    assert!(wait_until(Duration::from_secs(5), || runs(&env, &id) == 1).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn planted_workflow_refused_at_run() {
    let env = env();
    let inbox = env.dir("in");
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    // 홈 자체를 옮길 곳으로 — 저장 검사를 거치지 않고 저장소에 직접 심는다.
    let planted = env
        .store
        .save_workflow(StoredWorkflow {
            id: String::new(),
            name: "심음".into(),
            enabled: true,
            workflow: move_wf(&inbox, &env.root),
            updated_at: String::new(),
        })
        .unwrap();
    env.sup
        .enqueue(&planted.id, &inbox, trigger(&inbox.join("a.pdf")));
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(runs(&env, &planted.id), 0);
    assert!(inbox.join("a.pdf").exists());
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_ignores_tampered_roots_outside_policy() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let run_id = env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();

    // 기록의 허용 폴더를 홈 자체로 바꿔 심는다.
    let mut run = env.store.get_run(&run_id).unwrap();
    run.allowed_roots = vec![env.root.clone()];
    env.store.save_run(&run).unwrap();

    let report = env.sup.undo_run(&run_id).unwrap();
    assert!(report.restored.is_empty(), "{report:?}");
    assert!(out.join("sorted/a.pdf").exists());
    assert!(!inbox.join("a.pdf").exists());
}

fn notices(env: &Env, id: &str) -> Vec<RunNotice> {
    env.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            AppEvent::RunNotice(n) if n.workflow_id == id => Some(n.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn run_notice_emitted_with_trigger_and_name() {
    let env = env();
    // 수동 실행(꺼진 워크플로우)
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let run_id = env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();
    let manual = notices(&env, &id);
    assert_eq!(manual.len(), 2, "{manual:?}");
    assert!(matches!(manual[0].kind, NoticeKind::Started));
    assert!(
        matches!(
            manual[1].kind,
            NoticeKind::Finished {
                ok: true,
                failed_node: None,
                ..
            }
        ),
        "{manual:?}"
    );
    for n in &manual {
        assert_eq!(n.trigger, RunTrigger::Manual);
        assert_eq!(n.workflow_name, "정리");
        assert_eq!(n.run_id, run_id);
    }

    // 감시 실행
    let (inbox2, out2) = (env.dir("in2"), env.dir("out2"));
    let wid = save(&env, move_wf(&inbox2, &out2), true);
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(inbox2.join("b.pdf"), "pdf").unwrap();
    assert!(wait_until(Duration::from_secs(5), || notices(&env, &wid).len() == 2).await);
    let watch = notices(&env, &wid);
    assert!(matches!(watch[0].kind, NoticeKind::Started));
    assert!(matches!(
        watch[1].kind,
        NoticeKind::Finished { ok: true, .. }
    ));
    assert!(watch.iter().all(|n| n.trigger == RunTrigger::Watch));
}

#[tokio::test(flavor = "multi_thread")]
async fn run_notice_failed_node_is_label_not_error() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let mut wf = move_wf(&inbox, &out);
    wf.nodes[2].label = Some("파일 옮기기".into());
    let id = save(&env, wf, false);
    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    // 옮길 폴더 자리에 파일을 두어 옮기기 노드를 실패시킨다.
    fs::write(out.join("sorted"), "not a folder").unwrap();
    env.sup.run_now(&id, &inbox.join("a.pdf")).await.unwrap();
    let n = notices(&env, &id);
    match &n[1].kind {
        NoticeKind::Finished {
            ok: false,
            failed_node,
            ..
        } => assert_eq!(failed_node.as_deref(), Some("파일 옮기기")),
        k => panic!("{k:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn save_expecting_does_not_revive_deleted_workflow() {
    let env = env();
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    let wf = env.store.get_workflow(&id).unwrap();
    env.sup.delete_workflow(&id).unwrap();
    let expected = wf.updated_at.clone();
    assert!(env.sup.save_workflow_expecting(wf, &expected).is_err());
    assert!(
        !env.root
            .join("data/workflows")
            .join(format!("{id}.json"))
            .exists()
    );
}

/// 최종 리뷰 I-2: 노드 테스트 도중 앱이 끝나면 다음 시작 때 옮긴 파일을 되돌리고 결과를 기록에 남긴다.
/// 못 되돌린 것이 있으면 `TestUndoFailed`. 테스트가 아닌 실행은 닫기만 한다.
#[tokio::test(flavor = "multi_thread")]
async fn unfinished_test_run_undone_on_next_start() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let id = save(&env, move_wf(&inbox, &out), false);
    // 파일을 옮긴 실행 셋을 "도중에 앱이 꺼진 기록"(끝 시각 없음)으로 바꾼다.
    let unfinished = |name: &'static str, trigger: RunTrigger| {
        let (sup, store, id, file) = (
            env.sup.clone(),
            env.store.clone(),
            id.clone(),
            inbox.join(name),
        );
        async move {
            fs::write(&file, "pdf").unwrap();
            let run_id = sup.run_now(&id, &file).await.unwrap();
            let mut run = store.get_run(&run_id).unwrap();
            assert_eq!(run.file_ops.len(), 1);
            run.finished_at = String::new();
            run.trigger = Some(trigger);
            store.save_run(&run).unwrap();
            run_id
        }
    };
    let tested = unfinished("a.pdf", RunTrigger::Test).await;
    let lost = unfinished("b.pdf", RunTrigger::Test).await;
    let manual = unfinished("c.pdf", RunTrigger::Manual).await;
    // b는 옮긴 자리에서 사라져 되돌릴 수 없다.
    fs::remove_file(out.join("sorted/b.pdf")).unwrap();
    env.sup.stop();

    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let _sup2 = Supervisor::start(Deps {
        store: env.store.clone(),
        app_data: env.root.join("data"),
        home: env.root.clone(),
        trash_dir: env.root.join("trash"),
        ai: Arc::new(AiRunner::empty()),
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming::default(),
        recheck: Duration::from_secs(30),
        now: Arc::new(Utc::now),
    })
    .unwrap();

    assert!(inbox.join("a.pdf").exists() && !out.join("sorted/a.pdf").exists());
    let run = env.store.get_run(&tested).unwrap();
    assert_eq!(run.run.status, RunStatus::Cancelled);
    assert!(!run.finished_at.is_empty());
    let undo = run.undo.unwrap();
    assert_eq!((undo.restored.len(), undo.skipped.len()), (1, 0));

    let undo = env.store.get_run(&lost).unwrap().undo.unwrap();
    assert_eq!((undo.restored.len(), undo.skipped.len()), (0, 1));
    let failed: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            AppEvent::TestUndoFailed {
                workflow_id,
                run_id,
                at_start,
            } => Some((workflow_id.clone(), run_id.clone(), at_start.clone())),
            _ => None,
        })
        .collect();
    // 재리뷰 I-1: 시작 때는 화면 리스너가 없다 — 이벤트가 워크플로우 이름을 실어 Rust가 바로 알림을 보낸다.
    assert_eq!(failed, [(id.clone(), lost, Some("정리".to_string()))]);

    // 테스트가 아닌 실행은 닫기만 한다 — 사람이 실행 기록에서 되돌린다.
    assert!(env.store.get_run(&manual).unwrap().undo.is_none());
    assert!(out.join("sorted/c.pdf").exists());
}

/// 127.0.0.1 가짜 서버 — GET에는 `get_body`, 그 밖에는 `{}`. 받은 요청 본문을 남긴다.
async fn http_server(get_body: Vec<u8>) -> (String, Arc<Mutex<Vec<Vec<u8>>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let (mut buf, mut tmp) = (Vec::new(), [0u8; 65536]);
            let (head_end, len) = loop {
                let n = s.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map_or(0, |v| v.trim().parse().unwrap());
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let n = s.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
            }
            let body = match buf.starts_with(b"GET") {
                true => get_body.clone(),
                false => b"{}".to_vec(),
            };
            log.lock().unwrap().push(buf[head_end..].to_vec());
            let head = format!(
                "HTTP/1.1 200 X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            s.write_all(head.as_bytes()).await.unwrap();
            s.write_all(&body).await.unwrap();
        }
    });
    (base, seen)
}

/// 기록 파일에는 큰 응답 본문을 줄여 쓴다 — 실행 중 뒤 노드와 화면 이벤트는 전체를 받는다.
#[tokio::test(flavor = "multi_thread")]
async fn large_http_body_is_clipped_only_in_stored_run() {
    let env = env();
    let inbox = env.dir("in");
    let big = json!({"data": "x".repeat(300_000)});
    let full = serde_json::to_string(&big).unwrap();
    let full_units = full.encode_utf16().count();
    let (base, seen) = http_server(full.clone().into_bytes()).await;
    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": inbox}},
            {"id": "g", "type": "HTTP", "event": "http.request", "config": {"method": "GET", "url": base}},
            {"id": "p", "type": "HTTP", "event": "http.request", "config": {"method": "POST", "url": base, "body": {"echo": "{{nodes.g.output.body}}"}}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "g"}, {"id": "e2", "source": "g", "target": "p"}]
    }))
    .unwrap();
    let id = save(&env, wf, false);
    fs::write(inbox.join("a.txt"), "a").unwrap();
    let run_id = env.sup.run_now(&id, &inbox.join("a.txt")).await.unwrap();

    let run = env.store.get_run(&run_id).unwrap();
    assert_eq!(run.run.status, RunStatus::Succeeded, "{:?}", run.run.nodes);
    let g = &run.run.nodes["g"];
    let body = g.output.as_ref().unwrap()["body"].as_str().unwrap();
    let marker = format!("…(잘림, 전체 {full_units}자)");
    assert!(body.ends_with(&marker), "{}", &body[body.len() - 40..]);
    let kept = &body[..body.len() - marker.len()];
    assert_eq!(kept.encode_utf16().count(), 100_000);
    assert!(full.starts_with(kept));
    assert!(g.output_clipped);
    // 작은 응답은 그대로.
    let p = &run.run.nodes["p"];
    assert_eq!(p.output.as_ref().unwrap()["body"], json!({}));
    assert!(!p.output_clipped);
    // 파일에 표시가 남고, 작은 노드에는 칸이 없다.
    let raw: serde_json::Value = serde_json::to_value(&run).unwrap();
    assert_eq!(raw["run"]["nodes"]["g"]["outputClipped"], json!(true));
    assert_eq!(raw["run"]["nodes"]["p"].get("outputClipped"), None);

    // 실행 중 뒤 노드가 받은 값은 전체다.
    let posted = seen.lock().unwrap()[1].clone();
    let posted: serde_json::Value = serde_json::from_slice(&posted).unwrap();
    assert_eq!(posted["echo"].as_str().unwrap(), full);
    // 화면 이벤트도 전체다.
    let shown = env.events.lock().unwrap().iter().find_map(|e| match e {
        AppEvent::Exec {
            event: ExecEvent::NodeSucceeded { node_id, output },
            ..
        } if node_id == "g" => Some(output["body"].clone()),
        _ => None,
    });
    assert_eq!(shown, Some(big));
}

/// 줄임 표시 칸이 생기기 전 기록도 읽힌다.
#[test]
fn old_node_record_without_clip_flag_reads() {
    let rec: ieum_engine::NodeRecord = serde_json::from_value(
        json!({"status": "succeeded", "output": {"statusCode": 200, "body": "x"}, "error": null}),
    )
    .unwrap();
    assert!(!rec.output_clipped);
}

// ── 스케줄 트리거 ─────────────────────────────────────────────

/// 스케줄 트리거 → AI(가짜). 시계 기본값 2026-11-16 10:30 KST.
fn sched_wf(event: &str, config: serde_json::Value) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": event, "config": config},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "요약", "outputFields": [{"key": "folder", "type": "text"}]}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap()
}

fn set_clock(env: &Env, s: &str) {
    *env.clock.lock().unwrap() = utc(s);
}

fn slot(env: &Env, id: &str) -> Option<DateTime<Utc>> {
    env.store.get_schedule_slots().unwrap().get(id).copied()
}

fn set_enabled(env: &Env, id: &str, enabled: bool) {
    let mut wf = env.store.get_workflow(id).unwrap();
    wf.enabled = enabled;
    env.sup.save_workflow(wf).unwrap();
}

/// 끝난(끝 시각 있는) 실행 수.
fn finished_runs(env: &Env, id: &str) -> usize {
    env.store
        .list_runs(Some(id))
        .unwrap()
        .iter()
        .filter(|r| !r.finished_at.is_empty())
        .count()
}

/// 큐에 들어갔다면 끝날 만큼 기다린 뒤 실행 수.
async fn settle(env: &Env, id: &str) -> usize {
    tokio::time::sleep(Duration::from_millis(400)).await;
    runs(env, id)
}

fn hourly(env: &Env) -> String {
    save(env, sched_wf("schedule.hourly", json!({})), true)
}

#[tokio::test(flavor = "multi_thread")]
async fn first_enable_no_backfill() {
    let env = env();
    let id = hourly(&env);
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 0);
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T01:30:00Z")));
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_on_slot() {
    let env = env();
    let id = hourly(&env);
    set_clock(&env, "2026-11-16T02:00:10Z"); // 11:00:10 KST
    env.sup.tick_for_test();
    assert!(wait_until(Duration::from_secs(5), || finished_runs(&env, &id) == 1).await);
    let summary = &env.store.list_runs(Some(&id)).unwrap()[0];
    let run = env.store.get_run(&summary.run_id).unwrap();
    assert_eq!(run.trigger, Some(RunTrigger::Schedule));
    assert_eq!(run.scheduled_for.as_deref(), Some("2026-11-16T11:00:00"));
    assert_eq!(
        run.run.nodes["t"].output.as_ref().unwrap()["triggeredAt"],
        json!("2026-11-16T11:00:10")
    );
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T02:00:00Z")));
}

#[tokio::test(flavor = "multi_thread")]
async fn missed_within_grace_once() {
    let env = env();
    let id = hourly(&env);
    set_clock(&env, "2026-11-16T05:30:00Z"); // 14:30 KST — 11·12·13·14시 놓침
    env.sup.tick_for_test();
    assert!(wait_until(Duration::from_secs(5), || finished_runs(&env, &id) == 1).await);
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 1);
    let run = env.store.list_runs(Some(&id)).unwrap()[0].run_id.clone();
    assert_eq!(
        env.store.get_run(&run).unwrap().scheduled_for.as_deref(),
        Some("2026-11-16T14:00:00")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn missed_beyond_grace_none() {
    let env = env();
    let id = save(&env, sched_wf("schedule.daily", json!({"hour": "9"})), true);
    env.store
        .set_schedule_slot(&id, utc("2026-11-15T00:00:00Z"))
        .unwrap();
    set_clock(&env, "2026-11-16T02:01:00Z"); // 오늘 11:01 KST — 9시 차례가 2시간 넘게 지남
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 0);
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T00:00:00Z")));
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 0);
    set_clock(&env, "2026-11-17T00:00:00Z"); // 내일 9시
    env.sup.tick_for_test();
    assert!(wait_until(Duration::from_secs(5), || finished_runs(&env, &id) == 1).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_no_duplicate() {
    let env = env();
    let id = hourly(&env);
    set_clock(&env, "2026-11-16T02:00:10Z");
    env.sup.tick_for_test();
    assert!(wait_until(Duration::from_secs(5), || finished_runs(&env, &id) == 1).await);
    env.sup.stop();
    let c = env.clock.clone();
    let sup2 = Supervisor::start_with(
        Deps {
            store: env.store.clone(),
            app_data: env.root.join("data"),
            home: env.root.clone(),
            trash_dir: env.root.join("trash"),
            ai: Arc::new(AiRunner::empty().with_provider("fake", env.ai.clone())),
            emit: Arc::new(|_| {}),
            timing: WatchTiming::default(),
            recheck: Duration::from_secs(30),
            now: Arc::new(move || *c.lock().unwrap()),
        },
        false,
    )
    .unwrap();
    sup2.tick_for_test();
    assert_eq!(settle(&env, &id).await, 1);
    sup2.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn overlap_skips() {
    let env = env_with(
        FakeAi {
            gate: Some(Semaphore::new(0)),
            ..Default::default()
        },
        Duration::from_secs(30),
    );
    let id = hourly(&env);
    set_clock(&env, "2026-11-16T02:00:10Z");
    env.sup.tick_for_test();
    assert!(
        wait_until(Duration::from_secs(5), || env
            .ai
            .calls
            .load(Ordering::SeqCst)
            == 1)
        .await
    );
    set_clock(&env, "2026-11-16T03:00:10Z"); // 다음 차례 — 첫 실행이 아직 도는 중
    env.sup.tick_for_test();
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T03:00:00Z")));
    env.ai.gate.as_ref().unwrap().add_permits(5);
    assert!(wait_until(Duration::from_secs(5), || finished_runs(&env, &id) == 1).await);
    assert_eq!(settle(&env, &id).await, 1);
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reenable_starts_now() {
    let env = env();
    let id = hourly(&env);
    set_enabled(&env, &id, false);
    set_clock(&env, "2026-11-16T06:30:00Z"); // +5시간
    set_enabled(&env, &id, true);
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 0);
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T06:30:00Z")));
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_change_starts_now() {
    let env = env();
    let id = hourly(&env);
    set_clock(&env, "2026-11-16T02:30:00Z"); // 11시 차례 놓침(유예 안)
    let mut wf = env.store.get_workflow(&id).unwrap();
    wf.workflow = sched_wf("schedule.daily", json!({"hour": "11"}));
    env.sup.save_workflow(wf).unwrap();
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 0);
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T02:30:00Z")));
}

#[tokio::test(flavor = "multi_thread")]
async fn clock_back_resets_slot() {
    let env = env();
    let id = hourly(&env);
    set_clock(&env, "2026-11-16T00:10:00Z"); // 시계가 뒤로
    env.sup.tick_for_test();
    assert_eq!(settle(&env, &id).await, 0);
    assert_eq!(slot(&env, &id), Some(utc("2026-11-16T00:10:00Z")));
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_cron_watch_error() {
    let env = env();
    let id = save(
        &env,
        sched_wf("schedule.every", json!({"cron": "0 9 1 * 1"})),
        true,
    );
    env.sup.tick_for_test();
    let entry = env
        .sup
        .list_workflows()
        .unwrap()
        .into_iter()
        .find(|e| e.workflow.id == id)
        .unwrap();
    assert_eq!(entry.watch, WatchState::WatchError);
    assert!(
        entry
            .watch_message
            .as_deref()
            .unwrap()
            .starts_with("스케줄을 계산하지 못했어요"),
        "{:?}",
        entry.watch_message
    );
    assert!(status_events(&env, &id, false) >= 1);
    assert_eq!(entry.next_run, None);
    assert_eq!(settle(&env, &id).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn next_run_listed() {
    let env = env();
    let on = save(&env, sched_wf("schedule.daily", json!({"hour": "9"})), true);
    let off = save(
        &env,
        sched_wf("schedule.daily", json!({"hour": "9"})),
        false,
    );
    let list = env.sup.list_workflows().unwrap();
    let get = |id: &str| list.iter().find(|e| e.workflow.id == id).unwrap();
    assert_eq!(get(&on).next_run.as_deref(), Some("2026-11-17T09:00:00"));
    assert_eq!(get(&on).watch, WatchState::Watching);
    assert_eq!(get(&off).next_run, None);
    assert_eq!(get(&off).watch, WatchState::Off);
}
