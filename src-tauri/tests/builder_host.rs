//! 채팅 빌더의 앱 쪽 Host(`AppHost`). 모든 파일은 임시 폴더 안, AI는 가짜 러너, 키체인은 가짜.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ieum_desktop_lib::builder_host::{AppHost, PickFolder, no_pick};
use ieum_desktop_lib::credentials::AccountCredentials;
use ieum_desktop_lib::store::{RunTrigger, Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, Supervisor, WatchState};
use ieum_engine::{ExecEvent, Workflow};
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, ChunkFn, KeyStore};
use ieum_local::builder::{Builder, Code, Host, Op, SaveError, TestStatus};
use serde_json::json;
use tempfile::TempDir;

struct FakeAi;

#[async_trait]
impl AiProvider for FakeAi {
    async fn ask(&self, _req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
        Ok(AiAnswer {
            text: r#"{"folder": "sorted"}"#.into(),
            usage: None,
        })
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

type OnEvent = Box<dyn FnMut(&AppEvent) + Send>;

struct Env {
    _t: TempDir,
    root: PathBuf,
    sup: Supervisor,
    store: Store,
    creds: AccountCredentials,
    events: Arc<Mutex<Vec<AppEvent>>>,
    /// 이벤트가 올 때마다 부른다 — 실행 도중 끼어드는 일(원래 자리에 파일이 다시 생김) 흉내.
    on_event: Arc<Mutex<Option<OnEvent>>>,
}

impl Env {
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

fn env() -> Env {
    env_with_providers(&["fake"])
}

/// 임시 폴더 = 가짜 홈. 앱 데이터는 `data/`. `providers` = 러너에 등록할 공급원 id(전부 가짜 AI).
fn env_with_providers(providers: &[&str]) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let store = Store::new(root.join("data"));
    let creds = AccountCredentials::new(
        root.join("data"),
        "0123456789abcdef",
        Arc::new(FakeKeys::default()),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let on_event: Arc<Mutex<Option<OnEvent>>> = Arc::default();
    let (ev, hook) = (events.clone(), on_event.clone());
    let ai = providers.iter().fold(AiRunner::empty(), |r, id| {
        r.with_provider(id, Arc::new(FakeAi))
    });
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(ai.with_resolver(Arc::new(creds.clone()))),
        emit: Arc::new(move |e| {
            if let Some(f) = hook.lock().unwrap().as_mut() {
                f(&e);
            }
            ev.lock().unwrap().push(e);
        }),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck: Duration::from_secs(30),
    })
    .unwrap();
    Env {
        _t: t,
        root,
        sup,
        store,
        creds,
        events,
        on_event,
    }
}

/// 트리거 → 옮기기(`base/sorted`로).
fn move_only_wf(watch: &Path, base: &Path) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}", "baseFolder": base, "destination": "sorted"}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "m"}]
    }))
    .unwrap()
}

fn builder(env: &Env) -> Builder {
    let host = AppHost::new(env.sup.clone(), env.creds.clone(), no_pick());
    Builder::new(Arc::new(host)).unwrap()
}

fn test_m() -> Op {
    Op::Test {
        id: "m".into(),
        file: None,
    }
}

fn undo_failed_events(env: &Env) -> Vec<(String, String)> {
    let events = env.events.lock().unwrap();
    events
        .iter()
        .filter_map(|e| match e {
            AppEvent::TestUndoFailed {
                workflow_id,
                run_id,
            } => Some((workflow_id.clone(), run_id.clone())),
            _ => None,
        })
        .collect()
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

#[tokio::test(flavor = "multi_thread")]
async fn app_host_save_emits_changed_and_rejects_stale() {
    let env = env(); // sup, store, events, root
    let host = AppHost::new(env.sup.clone(), env.creds.clone(), no_pick());
    let id = save(&env, ai_only_wf(&env.dir("watch")), true);
    let mut doc = host.load(&id).unwrap();
    doc.enabled = false;
    let saved = host.save(&id, doc.clone()).unwrap();
    assert!(!env.store.get_workflow(&id).unwrap().enabled);
    assert!(env.events.lock().unwrap().iter().any(|e| matches!(e,
        AppEvent::WorkflowChanged { workflow_id, updated_at } if workflow_id == &id && updated_at == &saved.updated_at)));
    // 옛 updated_at으로 다시 저장 = 그사이 캔버스가 고친 것 → 거부, 저장본 그대로.
    assert!(matches!(host.save(&id, doc), Err(SaveError::Stale)));
    assert_eq!(
        env.store.get_workflow(&id).unwrap().updated_at,
        saved.updated_at
    );
    // 저장 검사는 그대로 탄다(홈 밖 폴더).
    let mut bad = host.load(&id).unwrap();
    bad.workflow.nodes[0]
        .config
        .insert("folder".into(), "/etc".into());
    assert!(matches!(host.save(&id, bad), Err(SaveError::Rejected(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn app_host_chat_and_pick_and_notice() {
    let env = env();
    let picked = env.dir("picked");
    let p = picked.clone();
    let host = AppHost::new(
        env.sup.clone(),
        env.creds.clone(),
        PickFolder(Arc::new(move |_id, _title| {
            let p = p.clone();
            Box::pin(async move { Some(p) })
        })),
    );
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    host.update_chat(&id, &mut |c| c.state.outline.push("a".into()))
        .unwrap();
    assert_eq!(host.load_chat(&id).unwrap().state.outline, ["a"]);
    assert_eq!(host.pick_folder(&id, "제목").await, Some(picked));
    assert!(host.check_folder(&env.root.join("watch")).is_ok());
    assert!(host.check_folder(Path::new("/etc")).is_err());
    assert!(!host.user_editing(&id));
    env.sup.set_editing(&id, true); // 편집기에 미저장 편집이 생김(B-4 `set_editing` 명령이 부르는 것)
    assert!(host.user_editing(&id) && !host.user_editing("다른 id"));
    env.sup.set_editing(&id, false);
    assert!(!host.user_editing(&id));
    host.finish_pending(&id, "정리");
    assert!(
        env.events.lock().unwrap().iter().any(
            |e| matches!(e, AppEvent::FinishPending { workflow_id, .. } if workflow_id == &id)
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn builder_test_moves_real_file_then_puts_it_back() {
    let env = env();
    let (watch, base) = (env.dir("watch"), env.dir("base"));
    fs::write(watch.join("a.txt"), "hello").unwrap();
    let id = save(&env, move_only_wf(&watch, &base), false);
    let r = builder(&env).apply(&id, test_m(), None).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.data["undo"], json!({"restored": 1, "failed": 0}));
    assert!(watch.join("a.txt").exists()); // 원래 자리로
    assert!(!base.join("sorted/a.txt").exists());
    assert!(!r.to_line().contains("a.txt"));
    let runs = env.store.list_runs(Some(&id)).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].trigger, runs[0].undone, runs[0].file_ops),
        (Some(RunTrigger::Test), true, 1)
    );
    let st = env.store.get_chat(&id).unwrap().state.tests;
    assert_eq!(
        (st["t"].status, st["m"].status),
        (TestStatus::Success, TestStatus::Success)
    );
    // 워크플로우는 꺼진 그대로(감시 없음).
    assert_eq!(env.sup.list_workflows().unwrap()[0].watch, WatchState::Off);
    assert!(undo_failed_events(&env).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn undo_failure_reaches_the_screen() {
    let env = env();
    let (watch, base) = (env.dir("watch"), env.dir("base"));
    fs::write(watch.join("a.txt"), "hello").unwrap();
    let id = save(&env, move_only_wf(&watch, &base), false);
    // 옮긴 뒤·되돌리기 전에 원래 자리에 같은 이름 파일이 다시 생긴다.
    let again = watch.join("a.txt");
    *env.on_event.lock().unwrap() = Some(Box::new(move |e| {
        if let AppEvent::Exec {
            event: ExecEvent::NodeSucceeded { node_id, .. },
            ..
        } = e
            && node_id == "m"
        {
            fs::write(&again, "x").unwrap();
        }
    }));
    let r = builder(&env).apply(&id, test_m(), None).await;
    assert_eq!((r.ok, r.code), (false, Some(Code::UndoFailed)));
    assert_eq!(r.data["undo"], json!({"restored": 0, "failed": 1}));
    assert!(!r.to_line().contains("a.txt"));
    let run_id = r.data["runId"].as_str().unwrap().to_string();
    assert_eq!(undo_failed_events(&env), [(id.clone(), run_id)]);
    // 대상 노드는 failed로 남아 완료로 넘어가지 못한다.
    let st = env.store.get_chat(&id).unwrap().state.tests;
    assert_eq!(st["m"].status, TestStatus::Failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_starts_only_after_person_confirms() {
    let env = env();
    let (watch, base) = (env.dir("watch"), env.dir("base"));
    let id = save(&env, move_only_wf(&watch, &base), false);
    let b = builder(&env);
    assert!(b.apply(&id, Op::Finish {}, None).await.ok);
    // 확인 전엔 감시 없음
    assert_eq!(env.sup.list_workflows().unwrap()[0].watch, WatchState::Off);
    assert!(
        env.events.lock().unwrap().iter().any(
            |e| matches!(e, AppEvent::FinishPending { workflow_id, .. } if workflow_id == &id)
        )
    );
    assert!(b.confirm_finish(&id, true).await.ok);
    assert_eq!(
        env.sup.list_workflows().unwrap()[0].watch,
        WatchState::Watching
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn run_test_rejects_sample_outside_roots() {
    let env = env();
    let (watch, base) = (env.dir("watch"), env.dir("base"));
    fs::write(env.root.join("secret.txt"), "x").unwrap();
    let id = save(&env, move_only_wf(&watch, &base), false);
    let wf = env.store.get_workflow(&id).unwrap().workflow;
    assert!(
        env.sup
            .run_test(&id, wf, &env.root.join("secret.txt"))
            .await
            .is_err()
    );
    assert!(env.store.list_runs(Some(&id)).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_default_follows_order_and_ready_needs_a_key() {
    // 공급원: api:openai·api:anthropic만 등록된 러너(가짜 공급원 + creds resolver), CLI는 감지 안 됨.
    let env = env_with_providers(&["api:anthropic", "api:openai"]);
    let host = || {
        AppHost::new(env.sup.clone(), env.creds.clone(), no_pick()).with_clis(Arc::new(Vec::new))
    };
    assert_eq!(host().ai_default().await, None);
    let openai = env
        .creds
        .add_api_key("openai", "회사", "sk-test-000000001234")
        .unwrap();
    assert_eq!(host().ai_default().await.unwrap().provider, "api:openai");
    let anth = env
        .creds
        .add_api_key("anthropic", "개인", "sk-ant-000000005678")
        .unwrap();
    let d = host().ai_default().await.unwrap(); // 순서상 anthropic이 먼저
    assert_eq!(
        (d.provider.as_str(), d.credential.as_deref(), d.model),
        ("api:anthropic", Some(anth.id.as_str()), None)
    );
    assert!(host().ai_ready("api:openai", Some(&openai.id)).await);
    assert!(!host().ai_ready("api:openai", Some("없는키")).await);
    assert!(!host().ai_ready("cli:claude", None).await); // 등록 안 된 공급원
    assert!(!host().ai_ready("", None).await);
    // 감지돼도 러너에 없는 공급원(cli:codex — 갈래 E 전)은 건너뛴다.
    let with_codex = AppHost::new(env.sup.clone(), env.creds.clone(), no_pick())
        .with_clis(Arc::new(|| vec!["cli:codex".into()]));
    assert_eq!(
        with_codex.ai_default().await.unwrap().provider,
        "api:anthropic"
    );
}
