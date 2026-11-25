//! 노드 패널 테스트(`test_node`) — 노드 하나만 실행하고 샘플로 남긴다. 모든 파일은 임시 폴더 안, AI는 가짜 러너.

mod github_support;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use github_support::{no_github, no_watch};
use ieum_desktop_lib::store::{RunTrigger, Sample, SampleStatus, Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{
    AppEvent, Deps, GITHUB_POLL, NodeTestStatus, Supervisor, rewrite_undone_paths,
};
use ieum_engine::Workflow;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, AiUsage, ChunkFn};
use ieum_local::builder::TestTrigger;
use ieum_local::github::watch::CreatedIssues;
use ieum_local::{FileOp, FileOpKind, WatchTiming};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::sync::Semaphore;

/// 가짜 AI 공급원(`cli:claude` 자리 — 노드 단위 검증이 선택지를 본다): `{"folder": "sorted"}`. 받은 프롬프트를 남기고 동시 실행 수를 잰다.
#[derive(Default)]
struct FakeAi {
    calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
    gate: Option<Semaphore>,
    prompts: Mutex<Vec<String>>,
    usage: Option<AiUsage>,
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
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(AiAnswer {
            text: r#"{"folder": "sorted"}"#.into(),
            usage: self.usage.clone(),
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
}

impl Env {
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

fn env_with(ai: FakeAi) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let store = Store::new(root.join("data"));
    let ai = Arc::new(ai);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let now: DateTime<Utc> = "2026-11-16T01:30:00Z".parse().unwrap();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(AiRunner::empty().with_provider("cli:claude", ai.clone())),
        github: no_github(),
        github_watch: no_watch(),
        github_created: Arc::new(CreatedIssues::in_memory()),
        github_poll: GITHUB_POLL,
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck: Duration::from_secs(30),
        now: Arc::new(move || now),
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
    env_with(FakeAi::default())
}

/// 앞 단계 값은 `data` 칸에(지시 칸은 참조 금지).
fn ai_node(id: &str, data: &str) -> Value {
    json!({"id": id, "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "분류", "data": data, "outputFields": [{"key": "folder", "type": "text"}]}})
}

/// 수동 → a1 → a2(a1 참조) → a3(a2 참조).
fn chain_wf() -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "manual.run", "config": {}},
            ai_node("a1", "첫째"),
            ai_node("a2", "값 {{nodes.a1.output.folder}}"),
            ai_node("a3", "다음 {{nodes.a2.output.folder}}"),
        ],
        "edges": [
            {"id": "e1", "source": "t", "target": "a1"},
            {"id": "e2", "source": "a1", "target": "a2"},
            {"id": "e3", "source": "a2", "target": "a3"}
        ]
    }))
    .unwrap()
}

/// 폴더 트리거 → base 아래 `sorted`로 옮기기.
fn move_wf(watch: &std::path::Path, base: &std::path::Path) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}", "baseFolder": base, "destination": "sorted"}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "m"}]
    }))
    .unwrap()
}

fn save(env: &Env, wf: Workflow) -> String {
    env.sup
        .save_workflow(StoredWorkflow {
            id: String::new(),
            name: "정리".into(),
            enabled: false,
            workflow: wf,
            updated_at: String::new(),
        })
        .unwrap()
        .id
}

fn sample(status: SampleStatus, out: Value) -> Sample {
    Sample {
        status,
        output: out.as_object().cloned(),
        error: None,
        error_code: None,
        tested_at: "2026-01-01T00:00:00.000Z".into(),
        node: json!({"event": "local.ai", "config": {}}),
    }
}

fn seed(env: &Env, id: &str, node: &str, s: Sample) {
    env.store
        .put_samples(id, BTreeMap::from([(node.to_string(), s)]))
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn node_test_runs_only_target() {
    let env = env();
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    seed(
        &env,
        &id,
        "a1",
        sample(SampleStatus::Succeeded, json!({"folder": "앞값"})),
    );

    let r = env.sup.test_node(&id, wf, "a2", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Succeeded, "{r:?}");
    assert_eq!(r.output.unwrap()["folder"], "sorted");
    assert!(r.tested_at.is_some());
    // 앞(a1)·뒤(a3) 노드는 돌지 않는다 — AI 호출은 대상 하나뿐.
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 1);
    let prompts = env.ai.prompts.lock().unwrap().clone();
    assert!(prompts[0].contains("값 앞값"), "{prompts:?}");
    let samples = env.store.get_samples(&id).unwrap();
    assert_eq!(samples["a2"].status, SampleStatus::Succeeded);
    assert_eq!(samples["a2"].node["event"], "local.ai");
    assert!(!samples.contains_key("a3"));
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_when_upstream_has_no_sample() {
    let env = env();
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    let r = env.sup.test_node(&id, wf, "a2", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Missing);
    assert_eq!(r.missing, vec!["a1".to_string()]);
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);
    assert!(env.store.get_samples(&id).unwrap().is_empty());
    assert!(env.store.list_runs(Some(&id)).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_sample_counts_as_missing() {
    let env = env();
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    seed(&env, &id, "a1", sample(SampleStatus::Failed, json!(null)));
    let r = env.sup.test_node(&id, wf, "a2", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Missing);
    assert_eq!(r.missing, vec!["a1".to_string()]);
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);
}

/// 트리거를 같은 id로 바꾸면 옛 트리거 샘플이 남는다 — event가 다르면 없는 것으로 본다(GAPS.md 노드별 테스트).
#[tokio::test(flavor = "multi_thread")]
async fn stale_event_sample_counts_as_missing() {
    let env = env();
    let wf = move_wf(&env.dir("in"), &env.dir("out"));
    let id = save(&env, wf.clone());
    seed(
        &env,
        &id,
        "t",
        Sample {
            node: json!({"event": "schedule.daily", "config": {}}),
            ..sample(SampleStatus::Succeeded, json!({"path": "/x"}))
        },
    );
    let r = env.sup.test_node(&id, wf, "m", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Missing, "{r:?}");
    assert_eq!(r.missing, vec!["t".to_string()]);
    assert!(!env.store.get_samples(&id).unwrap().contains_key("m"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_does_not_overwrite_sample() {
    let env = env();
    let mut wf = chain_wf();
    let id = save(&env, wf.clone());
    seed(
        &env,
        &id,
        "a1",
        sample(SampleStatus::Succeeded, json!({"folder": "앞값"})),
    );
    seed(
        &env,
        &id,
        "a2",
        sample(SampleStatus::Succeeded, json!({"folder": "예전"})),
    );
    // 편집본의 필수 칸을 비운다(저장하지 않은 편집).
    wf.nodes[2].config.insert("prompt".into(), json!(""));
    let r = env.sup.test_node(&id, wf, "a2", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Invalid, "{r:?}");
    assert!(r.error.is_some());
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 0);
    let samples = env.store.get_samples(&id).unwrap();
    assert_eq!(samples["a2"].output.as_ref().unwrap()["folder"], "예전");
}

#[tokio::test(flavor = "multi_thread")]
async fn trigger_test_saves_sample() {
    let env = env();
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    let r = env.sup.test_node(&id, wf, "t", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Succeeded);
    assert_eq!(r.output, Some(Map::new()));
    assert_eq!(r.run_id, None);
    assert_eq!(
        env.store.get_samples(&id).unwrap()["t"].status,
        SampleStatus::Succeeded
    );

    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "schedule.daily", "config": {"hour": "9"}},
            ai_node("a", "요약")
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap();
    let sid = save(&env, wf.clone());
    let r = env.sup.test_node(&sid, wf, "t", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Succeeded, "{r:?}");
    let out = r.output.unwrap();
    assert!(!out.is_empty(), "스케줄 출력: {out:?}");
    assert_eq!(env.store.get_samples(&sid).unwrap()["t"].output, Some(out));
    // 트리거 테스트는 실행 기록을 남기지 않는다.
    assert!(env.store.list_runs(None).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn folder_trigger_needs_file() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let wf = move_wf(&inbox, &out);
    let id = save(&env, wf.clone());
    let err = env
        .sup
        .test_node(&id, wf.clone(), "t", None)
        .await
        .unwrap_err();
    assert!(err.contains("파일"), "{err}");
    assert!(env.store.get_samples(&id).unwrap().is_empty());

    fs::write(inbox.join("a.pdf"), "pdf").unwrap();
    let r = env
        .sup
        .test_node(&id, wf, "t", Some(&inbox.join("a.pdf")))
        .await
        .unwrap();
    assert_eq!(r.status, NodeTestStatus::Succeeded);
    assert_eq!(
        r.output.unwrap()["path"],
        json!(inbox.join("a.pdf").to_string_lossy())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn file_node_sample_uses_undone_path() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let wf = move_wf(&inbox, &out);
    let id = save(&env, wf.clone());
    let file = inbox.join("a.pdf");
    fs::write(&file, "pdf").unwrap();
    env.sup
        .test_node(&id, wf.clone(), "t", Some(&file))
        .await
        .unwrap();

    let r = env.sup.test_node(&id, wf, "m", None).await.unwrap();
    assert_eq!(r.status, NodeTestStatus::Succeeded, "{r:?}");
    assert!(r.undone);
    assert_eq!(r.undo_failed, 0);
    let original = json!(file.to_string_lossy());
    assert_eq!(r.output.as_ref().unwrap()["path"], original);
    // 파일은 원래 자리에, 샘플 경로도 원래 경로.
    assert!(file.exists());
    assert!(!out.join("sorted").join("a.pdf").exists());
    let samples = env.store.get_samples(&id).unwrap();
    assert_eq!(samples["m"].output.as_ref().unwrap()["path"], original);
    let run = env.store.get_run(r.run_id.as_deref().unwrap()).unwrap();
    assert_eq!(run.undo.unwrap().restored.len(), 1);
}

#[test]
fn rewrite_undone_paths_nested() {
    let op = FileOp {
        kind: FileOpKind::Move,
        from: "/x/in/b.pdf".into(),
        to: "/x/out/a.pdf".into(),
        file_id: None,
    };
    let mut out = json!({
        "path": "/x/out/a.pdf",
        "name": "a.pdf",
        "list": [{"p": "/x/out/a.pdf"}, "a.pdf", 3],
        "deep": {"inner": {"path": "/x/out/a.pdf"}},
        "other": "그대로"
    })
    .as_object()
    .cloned()
    .unwrap();
    rewrite_undone_paths(&mut out, &[op]);
    assert_eq!(
        Value::Object(out),
        json!({
            "path": "/x/in/b.pdf",
            "name": "b.pdf",
            "list": [{"p": "/x/in/b.pdf"}, "b.pdf", 3],
            "deep": {"inner": {"path": "/x/in/b.pdf"}},
            "other": "그대로"
        })
    );

    // 이름 바꾸기(a→b) 뒤 이름 그대로 옮기기(b→sorted/b) — undo는 거꾸로 돌려준다.
    // 이름이 그대로인 짝(b.pdf→b.pdf)에 걸려 멈추지 않고 a.pdf까지 가야 한다.
    let restored = [
        FileOp {
            kind: FileOpKind::Move,
            from: "/x/b.pdf".into(),
            to: "/x/sorted/b.pdf".into(),
            file_id: None,
        },
        FileOp {
            kind: FileOpKind::Move,
            from: "/x/a.pdf".into(),
            to: "/x/b.pdf".into(),
            file_id: None,
        },
    ];
    let mut out = json!({"path": "/x/sorted/b.pdf", "name": "b.pdf"})
        .as_object()
        .cloned()
        .unwrap();
    rewrite_undone_paths(&mut out, &restored);
    assert_eq!(
        Value::Object(out),
        json!({"path": "/x/a.pdf", "name": "a.pdf"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn run_record_keeps_usage() {
    let env = env_with(FakeAi {
        usage: Some(AiUsage {
            source: "anthropic".into(),
            model: Some("claude-opus-5-5".into()),
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: None,
            cache_write_tokens: None,
            credential: None,
        }),
        ..Default::default()
    });
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    seed(
        &env,
        &id,
        "a1",
        sample(SampleStatus::Succeeded, json!({"folder": "앞값"})),
    );
    let r = env.sup.test_node(&id, wf, "a2", None).await.unwrap();
    let runs = env.store.list_runs(Some(&id)).unwrap();
    assert_eq!(runs.len(), 1);
    let run = env.store.get_run(r.run_id.as_deref().unwrap()).unwrap();
    assert_eq!(run.trigger, Some(RunTrigger::Test));
    assert!(!run.finished_at.is_empty());
    assert_eq!(run.run.nodes.len(), 1);
    assert!(run.run.nodes["a2"].usage.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn node_test_waits_in_queue() {
    let env = env_with(FakeAi {
        gate: Some(Semaphore::new(0)),
        ..Default::default()
    });
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    seed(
        &env,
        &id,
        "a1",
        sample(SampleStatus::Succeeded, json!({"folder": "앞값"})),
    );
    let full = {
        let (sup, id) = (env.sup.clone(), id.clone());
        tokio::spawn(async move { sup.run_now(&id, None).await })
    };
    // 전체 실행이 첫 AI에서 멈출 때까지.
    for _ in 0..500 {
        if env.ai.calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if env.ai.calls.load(Ordering::SeqCst) != 1 {
        env.ai.gate.as_ref().unwrap().add_permits(10);
        panic!("전체 실행이 AI에 닿지 않음");
    }
    let test = {
        let (sup, id) = (env.sup.clone(), id.clone());
        tokio::spawn(async move { sup.test_node(&id, wf, "a2", None).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (calls, finished) = (env.ai.calls.load(Ordering::SeqCst), test.is_finished());
    // 검사가 실패해도 막힌 실행이 풀리게 먼저 허가를 준다.
    env.ai.gate.as_ref().unwrap().add_permits(10);
    assert_eq!(calls, 1, "테스트가 줄을 서지 않음");
    assert!(!finished);

    full.await.unwrap().unwrap();
    let r = test.await.unwrap().unwrap();
    assert_eq!(r.status, NodeTestStatus::Succeeded);
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 4);
    assert_eq!(env.ai.max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn builder_test_writes_samples() {
    let env = env();
    let (inbox, out) = (env.dir("in"), env.dir("out"));
    let wf = move_wf(&inbox, &out);
    let id = save(&env, wf.clone());
    let file = inbox.join("a.pdf");
    fs::write(&file, "pdf").unwrap();

    env.sup
        .run_test(&id, wf, TestTrigger::File(file.clone()))
        .await
        .unwrap();
    let samples = env.store.get_samples(&id).unwrap();
    assert_eq!(samples["t"].status, SampleStatus::Succeeded);
    assert_eq!(samples["m"].status, SampleStatus::Succeeded);
    // 되돌린 뒤 경로(원래 경로)로.
    assert_eq!(
        samples["m"].output.as_ref().unwrap()["path"],
        json!(file.to_string_lossy())
    );
    assert_eq!(samples["m"].node["event"], "local.file_move");
    assert!(file.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn no_exec_events() {
    let env = env();
    let wf = chain_wf();
    let id = save(&env, wf.clone());
    seed(
        &env,
        &id,
        "a1",
        sample(SampleStatus::Succeeded, json!({"folder": "앞값"})),
    );
    env.events.lock().unwrap().clear();
    env.sup
        .test_node(&id, wf.clone(), "a2", None)
        .await
        .unwrap();
    env.sup.test_node(&id, wf, "t", None).await.unwrap();
    let events = env.events.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AppEvent::Exec { .. } | AppEvent::RunNotice(_))),
        "{events:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unsaved_workflow_asks_to_save_first() {
    let env = env();
    let err = env
        .sup
        .test_node("", chain_wf(), "t", None)
        .await
        .unwrap_err();
    assert!(err.contains("먼저 저장해 주세요"), "{err}");
}
