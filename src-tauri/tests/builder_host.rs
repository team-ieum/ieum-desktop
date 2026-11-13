//! 채팅 빌더의 앱 쪽 Host(`AppHost`). 모든 파일은 임시 폴더 안, AI는 가짜 러너, 키체인은 가짜.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ieum_desktop_lib::builder_host::{AppHost, PickFolder, no_pick};
use ieum_desktop_lib::credentials::AccountCredentials;
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, Supervisor};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, ChunkFn, KeyStore};
use ieum_local::builder::{Host, SaveError};
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

struct Env {
    _t: TempDir,
    root: PathBuf,
    sup: Supervisor,
    store: Store,
    creds: AccountCredentials,
    events: Arc<Mutex<Vec<AppEvent>>>,
}

impl Env {
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }
}

/// 임시 폴더 = 가짜 홈. 앱 데이터는 `data/`.
fn env() -> Env {
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
    let ev = events.clone();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(
            AiRunner::empty()
                .with_provider("fake", Arc::new(FakeAi))
                .with_resolver(Arc::new(creds.clone())),
        ),
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
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
