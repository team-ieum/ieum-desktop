//! 캔버스 저장 — 화면이 읽은 `updatedAt`이 저장본과 다르면 덮지 않는다.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ieum_desktop_lib::commands::save_from_canvas;
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, STALE, Supervisor};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, ChunkFn};
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

struct Env {
    _t: TempDir,
    root: PathBuf,
    sup: Supervisor,
    store: Store,
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
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(AiRunner::empty().with_provider("fake", Arc::new(FakeAi))),
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
async fn canvas_save_rejects_stale_updated_at() {
    let env = env();
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    let seen = env.store.get_workflow(&id).unwrap(); // 화면이 읽은 사본
    let mut by_builder = seen.clone();
    by_builder.name = "빌더가 고침".into();
    tokio::time::sleep(Duration::from_millis(5)).await; // updatedAt은 밀리초 단위
    let newer = env.sup.save_workflow(by_builder).unwrap();
    let mut mine = seen.clone();
    mine.name = "화면이 고침".into();
    assert_eq!(save_from_canvas(&env.sup, mine.clone()).unwrap_err(), STALE);
    assert_eq!(env.store.get_workflow(&id).unwrap().name, "빌더가 고침"); // 덮지 않았다
    mine.updated_at = newer.updated_at;
    assert_eq!(
        save_from_canvas(&env.sup, mine).unwrap().name,
        "화면이 고침"
    );
    // 새 워크플로우(id 없음)는 검사 없이 저장된다.
    let fresh = StoredWorkflow {
        id: String::new(),
        name: "새 워크플로우".into(),
        enabled: false,
        workflow: serde_json::from_value(json!({"nodes": [], "edges": []})).unwrap(),
        updated_at: String::new(),
    };
    assert!(!save_from_canvas(&env.sup, fresh).unwrap().id.is_empty());
    // 캔버스 저장은 workflow-changed를 내지 않는다.
    assert!(
        !env.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, AppEvent::WorkflowChanged { .. }))
    );
}
