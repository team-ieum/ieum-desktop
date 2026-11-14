//! 소켓 → Builder 잇기와 세션 정리. 모든 파일은 임시 폴더 안, AI·키는 가짜.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ieum_desktop_lib::account::AccountId;
use ieum_desktop_lib::active::{Active, BaseDeps};
use ieum_desktop_lib::builder_host::{PickFolder, no_pick};
use ieum_desktop_lib::channel::{Channel, Handler, apply_op, open_gate};
use ieum_desktop_lib::commands::{delete_workflow_and_sessions, end_all_sessions};
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::AppEvent;
use ieum_desktop_lib::terminal::{self, Gate, Terms, Tool};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, ChunkFn, KeyStore};
use ieum_local::builder::{Code, Reply};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

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
    active: Arc<Active>,
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
    let app_data = root.join("data");
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let active = Active::new(BaseDeps {
        app_data: app_data.clone(),
        settings: Store::new(app_data),
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
        active: Arc::new(active),
        events,
    }
}

fn acc(email: &str) -> AccountId {
    AccountId {
        email: email.into(),
        server_url: "http://127.0.0.1:1".into(),
    }
}

/// 트리거 → AI만(파일을 건드리지 않는다). AI 노드에는 label이 없다.
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

/// 경로가 짧아야 소켓이 열린다(macOS 104바이트).
fn short() -> TempDir {
    tempfile::Builder::new()
        .prefix("i")
        .tempdir_in("/tmp")
        .unwrap()
}

/// 앱이 쓰는 것과 같은 길: 소켓 요청 → `apply_op` → Builder.
fn handler(active: Arc<Active>, pick: PickFolder) -> Handler {
    Arc::new(move |s, op| {
        let (a, p) = (active.clone(), pick.clone());
        Box::pin(async move { apply_op(&a, &p, s, op).await })
    })
}

/// `{"token":…,"op":…,"args":…}` 한 줄.
fn req(token: &str, mut op: Value) -> String {
    op["token"] = json!(token);
    op.to_string()
}

async fn ask(path: &Path, line: &str) -> Reply {
    let mut s = UnixStream::connect(path).await.unwrap();
    let _ = s.write_all(format!("{line}\n").as_bytes()).await;
    let mut out = String::new();
    BufReader::new(s).read_line(&mut out).await.unwrap();
    serde_json::from_str(&out).unwrap()
}

fn label_of_ai(env_sup: &ieum_desktop_lib::supervisor::Supervisor, id: &str) -> Option<String> {
    env_sup.store().get_workflow(id).unwrap().workflow.nodes[1]
        .label
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn socket_request_edits_saved_workflow_and_canvas_hears_it() {
    let (env, t) = (env(), short());
    env.active.activate(acc("a@x.com")).await.unwrap();
    let sup = env.active.sup().await.unwrap();
    let id = sup
        .save_workflow(stored(ai_only_wf(&env.dir("in"))))
        .unwrap()
        .id; // enabled: true
    let ch = Channel::new(
        handler(env.active.clone(), no_pick()),
        t.path().to_path_buf(),
    );
    // 없는 워크플로우
    assert!(
        ch.session(&env.active, "33333333-3333-4333-8333-333333333333")
            .await
            .is_err()
    );
    let s = ch.session(&env.active, &id).await.unwrap();
    assert_eq!(
        s.bin_dir,
        std::env::current_exe().unwrap().parent().unwrap()
    );
    let rename = |label: &str| json!({"op": "rename", "args": {"id": "a", "label": label}});
    // 사람이 캔버스에서 고치는 중(미저장)이면 명령은 거절되고 저장본은 그대로다.
    sup.set_editing(&id, true);
    let r = ask(&s.socket, &req(&s.token, rename("새 이름"))).await;
    assert_eq!((r.code, r.exit_code()), (Some(Code::UserEditing), 2));
    assert_eq!(label_of_ai(&sup, &id), None);
    // 읽기는 된다
    assert!(
        ask(&s.socket, &req(&s.token, json!({"op": "status"})))
            .await
            .ok
    );
    sup.set_editing(&id, false); // 저장한 뒤
    let r = ask(&s.socket, &req(&s.token, rename("새 이름"))).await;
    assert!(r.ok, "{r:?}");
    let saved = sup.store().get_workflow(&id).unwrap();
    assert_eq!(saved.workflow.nodes[1].label.as_deref(), Some("새 이름"));
    assert!(!saved.enabled); // 첫 편집에서 꺼진다
    assert!(env.events.lock().unwrap().iter().any(|e| matches!(e,
        AppEvent::WorkflowChanged { workflow_id, updated_at }
            if workflow_id == &id && updated_at == &saved.updated_at)));
    // 다른 계정으로 바뀌면 옛 세션은 그 계정의 것도 새 계정의 것도 건드리지 못한다.
    env.active.activate(acc("b@x.com")).await.unwrap();
    let r = ask(&s.socket, &req(&s.token, rename("또"))).await;
    assert_eq!(r.code, Some(Code::Unauthorized));
    assert_eq!(label_of_ai(&sup, &id).as_deref(), Some("새 이름"));
}

#[tokio::test(flavor = "multi_thread")]
async fn same_workflow_ops_run_one_at_a_time() {
    let (env, t) = (env(), short());
    env.active.activate(acc("a@x.com")).await.unwrap();
    let sup = env.active.sup().await.unwrap();
    let id = sup
        .save_workflow(stored(ai_only_wf(&env.dir("in"))))
        .unwrap()
        .id;
    // 폴더 고르기: gate가 풀리면 picked를 준다.
    let (gate, picked) = (Arc::new(tokio::sync::Notify::new()), env.dir("picked"));
    let (g, p) = (gate.clone(), picked.clone());
    let pick = PickFolder(Arc::new(move |_, _| {
        let (g, p) = (g.clone(), p.clone());
        Box::pin(async move {
            g.notified().await;
            Some(p)
        })
    }));
    let ch = Channel::new(handler(env.active.clone(), pick), t.path().to_path_buf());
    let s = ch.session(&env.active, &id).await.unwrap();
    let (sock, tk) = (s.socket.clone(), s.token.clone());
    let picking = tokio::spawn(async move {
        let op = json!({"op": "pick-folder", "args": {"id": "t", "field": "folder"}});
        ask(&sock, &req(&tk, op)).await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (sock, tk) = (s.socket.clone(), s.token.clone());
    let renaming = tokio::spawn(async move {
        let op = json!({"op": "rename", "args": {"id": "a", "label": "둘째"}});
        ask(&sock, &req(&tk, op)).await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!renaming.is_finished()); // 고르기가 끝날 때까지 줄을 선다
    assert_eq!(label_of_ai(&sup, &id), None);
    gate.notify_one();
    let (picking, renaming) = (picking.await.unwrap(), renaming.await.unwrap());
    assert!(picking.ok && renaming.ok, "{picking:?} {renaming:?}");
    let saved = sup.store().get_workflow(&id).unwrap(); // 둘 다 남았다(덮어쓰기 없음)
    assert_eq!(saved.workflow.nodes[0].config["folder"], json!(picked));
    assert_eq!(saved.workflow.nodes[1].label.as_deref(), Some("둘째"));
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_workflow_revokes_its_sessions() {
    let (env, t) = (env(), short());
    env.active.activate(acc("a@x.com")).await.unwrap();
    let sup = env.active.sup().await.unwrap();
    let id = sup
        .save_workflow(stored(ai_only_wf(&env.dir("in"))))
        .unwrap()
        .id;
    let ch = Channel::new(
        handler(env.active.clone(), no_pick()),
        t.path().to_path_buf(),
    );
    let s = ch.session(&env.active, &id).await.unwrap();
    delete_workflow_and_sessions(&env.active, &ch, &Terms::default(), &id)
        .await
        .unwrap();
    assert!(sup.store().get_workflow(&id).is_err());
    assert_eq!(
        ask(&s.socket, &req(&s.token, json!({"op": "status"})))
            .await
            .code,
        Some(Code::Unauthorized)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signed_out_app_gives_no_session() {
    let (env, t) = (env(), short());
    let ch = Channel::new(
        handler(env.active.clone(), no_pick()),
        t.path().to_path_buf(),
    );
    let r = ch
        .session(&env.active, "33333333-3333-4333-8333-333333333333")
        .await;
    assert_eq!(r.err().as_deref(), Some("NOT_SIGNED_IN"));
}

// ── I-1: 터미널 세션 ↔ 소켓 토큰 ──

/// 앱(`lib.rs`)이 만드는 것과 같은 Gate — `open` ↔ `session`, `close(token)` ↔ `revoke(token)`.
fn gate_of(ch: &Channel, active: &Arc<Active>) -> Gate {
    let (c, a, c2) = (ch.clone(), active.clone(), ch.clone());
    Gate {
        open: Arc::new(move |id| {
            let (c, a) = (c.clone(), a.clone());
            Box::pin(async move { open_gate(&c, &a, &id).await })
        }),
        close: Arc::new(move |t| c2.revoke(t)),
    }
}

/// 실제 경로(`prepare` → `start`)로 세션을 띄운다. 프로그램만 `cat`으로 바꾼다. (소켓, 토큰)
async fn launch(env: &Env, terms: &Terms, gate: &Gate, id: &str) -> (PathBuf, String) {
    let root = env.active.sup().await.unwrap().store().root().to_path_buf();
    let mut p = terminal::prepare(&root, id, Tool::Claude, None, gate, 80, 24)
        .await
        .unwrap();
    let get = |k: &str| {
        let found = p.spawn.env.iter().find(|(key, _)| key == k);
        found.map(|(_, v)| v.clone()).unwrap()
    };
    let (socket, token) = (PathBuf::from(get("IEUM_SOCKET")), get("IEUM_TOKEN"));
    assert_eq!(p.token.as_deref(), Some(token.as_str()));
    p.spawn.program = "/bin/sh".into();
    p.spawn.args = vec!["-c".into(), "cat".into()];
    terminal::start(
        terms,
        id,
        Tool::Claude,
        p,
        gate,
        Arc::new(|_| {}),
        Box::new(|| {}),
    )
    .unwrap();
    (socket, token)
}

#[tokio::test(flavor = "multi_thread")]
async fn gate_from_channel_gives_env_and_revokes_when_session_ends() {
    let (env, t) = (env(), short());
    env.active.activate(acc("a@x.com")).await.unwrap();
    let sup = env.active.sup().await.unwrap();
    let save = || {
        sup.save_workflow(stored(ai_only_wf(&env.dir("in"))))
            .unwrap()
            .id
    };
    let id = save();
    let ch = Channel::new(
        handler(env.active.clone(), no_pick()),
        t.path().to_path_buf(),
    );
    let (gate, terms) = (gate_of(&ch, &env.active), Terms::default());
    let status = |token: &str| req(token, json!({"op": "status"}));

    let (socket, token) = launch(&env, &terms, &gate, &id).await;
    assert!(ask(&socket, &status(&token)).await.ok);
    // 터미널이 끝나면 그 토큰도 끝난다. 저장본은 그대로.
    let before = sup.store().get_workflow(&id).unwrap().updated_at;
    terms.kill(&id);
    let rename = json!({"op": "rename", "args": {"id": "a", "label": "끝난 뒤"}});
    assert_eq!(
        ask(&socket, &req(&token, rename)).await.code,
        Some(Code::Unauthorized)
    );
    assert_eq!(sup.store().get_workflow(&id).unwrap().updated_at, before);
    assert_eq!(label_of_ai(&sup, &id), None);
    // 저장된 적 없는 워크플로우에는 터미널을 띄우지 못한다.
    let unsaved = "33333333-3333-4333-8333-333333333333";
    let root = sup.store().root().to_path_buf();
    assert!(
        terminal::prepare(&root, unsaved, Tool::Claude, None, &gate, 80, 24)
            .await
            .is_err()
    );

    // 삭제: 터미널이 남지 않는다.
    launch(&env, &terms, &gate, &id).await;
    assert!(terms.running(&id).is_some());
    delete_workflow_and_sessions(&env.active, &ch, &terms, &id)
        .await
        .unwrap();
    assert!(terms.running(&id).is_none());

    // 로그아웃: 터미널도 토큰도 소켓도 남지 않는다.
    let id2 = save();
    let (socket, token) = launch(&env, &terms, &gate, &id2).await;
    end_all_sessions(&ch, &terms);
    assert!(terms.running(&id2).is_none());
    assert!(!socket.exists());
    // 소켓이 다시 열려도 옛 토큰은 닿지 않는다.
    let again = ch.session(&env.active, &id2).await.unwrap();
    assert_eq!(
        ask(&again.socket, &status(&token)).await.code,
        Some(Code::Unauthorized)
    );
}
