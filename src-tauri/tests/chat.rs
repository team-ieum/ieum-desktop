//! 채팅 패널의 앱 쪽 한 턴(`chat::send`). 모든 파일은 임시 폴더 안, AI는 답을 미리 적어 둔 가짜.

mod github_support;

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use github_support::{no_github, no_watch};
use ieum_desktop_lib::builder_host::{AppHost, no_pick};
use ieum_desktop_lib::chat::{
    CHAT_BAD_DATA, CHAT_BUSY, CHAT_EMPTY, CHAT_MODEL_REQUIRED, ChatOpts, send, send_with, submit,
    visible_chat,
};
use ieum_desktop_lib::credentials::AccountCredentials;
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, GITHUB_POLL, Supervisor};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{
    AiAnswer, AiError, AiProvider, AiRequest, AiRunner, AiUsage, ChunkFn, KeyStore,
};
use ieum_local::builder::{
    AiDefault, Blueprint, BlueprintStatus, Builder, ChatFile, Role, apply_ui,
};
use ieum_local::chat::{BLUEPRINT_INSTRUCTION, CUT_SHORT, Limits, Stage};
use ieum_local::github::watch::CreatedIssues;
use serde_json::{Value, json};
use tempfile::TempDir;

/// 답 글자를 차례로 낸다. 떨어지면 빈 답. `"ERR:<글>"`은 호출 실패.
struct Script {
    answers: Mutex<VecDeque<String>>,
    seen: Mutex<Vec<AiRequest>>,
    delay_ms: u64,
}

impl Script {
    fn new(answers: impl IntoIterator<Item = String>) -> Arc<Script> {
        Script::slow(0, answers)
    }
    fn slow(delay_ms: u64, answers: impl IntoIterator<Item = String>) -> Arc<Script> {
        Arc::new(Script {
            answers: Mutex::new(answers.into_iter().collect()),
            seen: Mutex::default(),
            delay_ms,
        })
    }
    fn push(&self, answer: String) {
        self.answers.lock().unwrap().push_back(answer);
    }
    fn seen(&self) -> Vec<AiRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl AiProvider for Script {
    async fn ask(&self, req: AiRequest, _c: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
        self.seen.lock().unwrap().push(req);
        if self.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        }
        let text = self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ans("", json!([])));
        if let Some(e) = text.strip_prefix("ERR:") {
            return Err(AiError(e.into()));
        }
        Ok(AiAnswer {
            text,
            usage: Some(AiUsage {
                source: "anthropic".into(),
                // 단가표에 있는 모델 — 한도 검사에 금액이 잡히게.
                model: Some("claude-opus-5-5".into()),
                input_tokens: 1,
                output_tokens: 0,
                cache_read_tokens: None,
                cache_write_tokens: None,
                credential: None,
            }),
        })
    }
}

fn ans(say: &str, actions: Value) -> String {
    json!({"say": say, "actions": actions.to_string()}).to_string()
}

struct NoKeys;

impl KeyStore for NoKeys {
    fn get(&self, _account: &str) -> Result<Option<String>, KeyError> {
        Ok(None)
    }
    fn set(&self, _account: &str, _key: &str) -> Result<(), KeyError> {
        Ok(())
    }
    fn delete(&self, _account: &str) -> Result<(), KeyError> {
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

/// 임시 폴더 = 가짜 홈. 앱 데이터는 `data/`. 공급원은 가짜 `fake` 하나(노드와 채팅이 같이 쓴다).
fn env(script: Arc<Script>) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let store = Store::new(root.join("data"));
    let creds = AccountCredentials::new(root.join("data"), "0123456789abcdef", Arc::new(NoKeys));
    let ai = AiRunner::empty().with_provider("fake", script);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(ai.with_resolver(Arc::new(creds.clone()))),
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
        now: Arc::new(chrono::Utc::now),
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

fn builder(env: &Env) -> Builder {
    Builder::new(Arc::new(AppHost::new(
        env.sup.clone(),
        env.creds.clone(),
        no_pick(),
    )))
    .unwrap()
}

fn ai() -> AiDefault {
    AiDefault {
        provider: "fake".into(),
        credential: None,
        model: None,
        effort: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn turn_edits_workflow_and_saves_messages_and_usage() {
    let script = Script::new([]); // 답은 아래에서 채운다(노드 id가 필요)
    let env = env(script.clone());
    let id = save(&env, ai_only_wf(&env.dir("watch")), true);
    let node = env.store.get_workflow(&id).unwrap().workflow.nodes[1]
        .id
        .clone();
    script.push(ans(
        "이름을 바꿀게요",
        json!([{"op": "rename", "args": {"id": node, "label": "분류기"}}]),
    ));
    script.push(ans("바꿨어요", json!([])));
    let stages = Mutex::new(Vec::new());
    let turn = send(
        &env.sup,
        &builder(&env),
        &id,
        "이름을 분류기로",
        ai(),
        Limits::default(),
        &|s| stages.lock().unwrap().push(s),
    )
    .await
    .unwrap();
    assert_eq!(
        (turn.say.as_str(), turn.error, turn.cards.len()),
        ("이름을 바꿀게요\n바꿨어요", None, 0)
    );
    assert_eq!(
        *stages.lock().unwrap(),
        [Stage::Designing, Stage::Designing]
    );
    let wf = env.store.get_workflow(&id).unwrap();
    // 첫 편집에서 꺼진다
    assert_eq!(
        (wf.workflow.nodes[1].label.as_deref(), wf.enabled),
        (Some("분류기"), false)
    );
    let chat = env.store.get_chat(&id).unwrap();
    let got: Vec<(Role, &str)> = chat
        .messages
        .iter()
        .map(|m| (m.role, m.text.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (Role::User, "이름을 분류기로"),
            (Role::Assistant, "이름을 바꿀게요\n바꿨어요")
        ]
    );
    assert!(
        chat.messages
            .iter()
            .all(|m| chrono::DateTime::parse_from_rfc3339(&m.at).is_ok())
    );
    assert_eq!(chat.usage.len(), 2);
    // Builder가 쓴 state를 덮지 않았다
    assert!(chat.state.baseline.is_some());
    // 다음 턴의 프롬프트에 앞 대화가 실린다.
    script.push(ans("네", json!([])));
    send(
        &env.sup,
        &builder(&env),
        &id,
        "고마워",
        ai(),
        Limits::default(),
        &|_| {},
    )
    .await
    .unwrap();
    assert!(script.seen().last().unwrap().prompt.contains(
        "사용자: 이름을 분류기로\n어시스턴트: 이름을 바꿀게요\n바꿨어요\n사용자: 고마워"
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_turn_keeps_user_message_and_spent_usage() {
    let script = Script::new([
        ans("먼저 볼게요", json!([{"op": "status"}])),
        "ERR:키가 없어요".to_string(),
    ]);
    let env = env(script);
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    let turn = send(
        &env.sup,
        &builder(&env),
        &id,
        "정리해 줘",
        ai(),
        Limits::default(),
        &|_| {},
    )
    .await
    .unwrap();
    assert_eq!(turn.error.as_deref(), Some("키가 없어요"));
    let chat = env.store.get_chat(&id).unwrap();
    // 사용자 글 + 그때까지 한 말, 쓴 호출 1건
    assert_eq!((chat.messages.len(), chat.usage.len()), (2, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_before_touching_chat_file() {
    let env = env(Script::new([]));
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    let b = builder(&env);
    let go = |wf: &'static str, text: &'static str, ai: AiDefault| {
        let (sup, b) = (&env.sup, &b);
        async move { send(sup, b, wf, text, ai, Limits::default(), &|_| {}).await }
    };
    let id: &'static str = id.leak();
    assert_eq!(go(id, "  ", ai()).await.unwrap_err(), CHAT_EMPTY);
    let openai = AiDefault {
        provider: "api:openai".into(),
        credential: None,
        model: Some(" ".into()),
        effort: None,
    };
    assert_eq!(
        go(id, "안녕", openai).await.unwrap_err(),
        CHAT_MODEL_REQUIRED
    );
    // 없는 워크플로우
    assert!(
        go("22222222-2222-4222-8222-222222222222", "안녕", ai())
            .await
            .is_err()
    );
    assert!(!env.root.join("data/chats").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn second_turn_while_running_is_busy() {
    let env = env(Script::slow(300, [ans("끝", json!([]))]));
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    let b = builder(&env);
    let first = send(&env.sup, &b, &id, "하나", ai(), Limits::default(), &|_| {});
    let second = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        send(&env.sup, &b, &id, "둘", ai(), Limits::default(), &|_| {}).await
    };
    let (a, b2) = tokio::join!(first, second);
    assert!(a.is_ok());
    assert_eq!(b2.unwrap_err(), CHAT_BUSY);
    let users = env.store.get_chat(&id).unwrap();
    assert_eq!(
        users
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .count(),
        1
    );
    // 끝난 뒤에는 다시 된다.
    assert!(
        send(&env.sup, &b, &id, "셋", ai(), Limits::default(), &|_| {})
            .await
            .is_ok()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_over_limit_emits_key_limit() {
    let env = env(Script::new([ans("네", json!([]))]));
    env.store
        .set_key_limit("anthropic:k1", Some(0.000001))
        .unwrap();
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    // 기록되는 키 id는 채팅의 `credential`에서 온다.
    let ai = AiDefault {
        credential: Some("k1".into()),
        ..ai()
    };
    send(
        &env.sup,
        &builder(&env),
        &id,
        "안녕",
        ai,
        Limits::default(),
        &|_| {},
    )
    .await
    .unwrap();
    let got: Vec<_> = env
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            AppEvent::KeyLimit(n) => Some(n.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!((got[0].source.as_str(), got[0].level), ("anthropic", 100));
}

/// 이름 입력·감시 폴더 칸(watch)·제출 버튼의 설계서(판 1, Active). 칸 경로는 `path`.
fn blueprint(path: Option<&str>) -> Blueprint {
    let msg = json!({"createSurface": {
        "surfaceId": "blueprint", "catalogId": "ieum.chat.blueprint/v1",
        "components": [
            {"id": "root", "component": "Blueprint", "title": "정리", "children": ["name", "watch", "go"]},
            {"id": "name", "component": "TextInput", "label": "파일 이름", "value": {"path": "/name"}},
            {"id": "watch", "component": "AppGate", "slot": "watch",
                "target": {"event": "local.folder_new_file", "field": "folder"}},
            {"id": "go", "component": "SubmitButton"}
        ],
        "dataModel": {"name": "날짜.pdf"}
    }});
    let mut b = apply_ui(None, &[msg]).unwrap();
    b.slots.get_mut("watch").unwrap().path = path.map(Into::into);
    b
}

/// 제출된 뒤 만들기 턴을 기다리는 설계서.
fn submitted() -> Blueprint {
    Blueprint {
        status: BlueprintStatus::Submitted,
        build_pending: true,
        ..blueprint(Some("/x/in"))
    }
}

fn put(env: &Env, id: &str, b: Blueprint) {
    env.store
        .update_chat(id, |c| c.state.blueprint = Some(b))
        .unwrap();
}

fn stored_bp(env: &Env, id: &str) -> Blueprint {
    env.store.get_chat(id).unwrap().state.blueprint.unwrap()
}

/// 설계서 모드 켜짐.
fn on(data: Option<Value>) -> ChatOpts {
    ChatOpts {
        blueprint: true,
        data,
        ..ChatOpts::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn build_turn_clears_pending_only_on_clean_finish() {
    let env = env(Script::new([
        "ERR:x".to_string(),
        ans("만들었어요", json!([])),
    ]));
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    put(&env, &id, submitted());
    let b = builder(&env);
    let go = || {
        send_with(
            &env.sup,
            &b,
            &id,
            "설계서대로 만들어 주세요.",
            ai(),
            Limits::default(),
            &|_| {},
            on(None),
        )
    };
    assert_eq!(go().await.unwrap().error.as_deref(), Some("x"));
    let bp = stored_bp(&env, &id);
    assert_eq!((bp.build_pending, bp.build_attempts), (true, 1));
    assert_eq!(go().await.unwrap().error, None);
    let bp = stored_bp(&env, &id);
    assert_eq!(
        (bp.status, bp.build_pending, bp.build_attempts),
        (BlueprintStatus::Submitted, false, 2)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cut_short_build_turn_keeps_pending() {
    let env = env(Script::new([ans("볼게요", json!([{"op": "status"}]))]));
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    put(&env, &id, submitted());
    let limits = Limits {
        calls: 1,
        ..Limits::default()
    };
    let turn = send_with(
        &env.sup,
        &builder(&env),
        &id,
        "설계서대로 만들어 주세요.",
        ai(),
        limits,
        &|_| {},
        on(None),
    )
    .await
    .unwrap();
    // 잘린 턴은 오류가 없어도 끝난 것이 아니다.
    assert_eq!(turn.error, None);
    assert!(turn.say.contains(CUT_SHORT));
    let bp = stored_bp(&env, &id);
    assert_eq!((bp.build_pending, bp.build_attempts), (true, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn blueprint_data_with_slot_path_is_rejected_before_turn() {
    let script = Script::new([]);
    let env = env(script.clone());
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    put(&env, &id, blueprint(Some("/x/in")));
    let before = env.store.get_chat(&id).unwrap();
    let r = send_with(
        &env.sup,
        &builder(&env),
        &id,
        "바꿔 줘",
        ai(),
        Limits::default(),
        &|_| {},
        on(Some(json!({"name": "/x/in/a.pdf"}))),
    )
    .await;
    assert_eq!(r.unwrap_err(), CHAT_BAD_DATA);
    let after = env.store.get_chat(&id).unwrap();
    assert_eq!(after.messages.len(), before.messages.len());
    assert_eq!(after.state.blueprint, before.state.blueprint);
    assert!(script.seen().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn classic_send_ignores_blueprint() {
    let script = Script::new([ans("네", json!([])), ans("네", json!([]))]);
    let env = env(script.clone());
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    put(&env, &id, blueprint(None));
    let b = builder(&env);
    let go = |opts: ChatOpts| {
        send_with(
            &env.sup,
            &b,
            &id,
            "안녕",
            ai(),
            Limits::default(),
            &|_| {},
            opts,
        )
    };
    go(ChatOpts::default()).await.unwrap();
    go(on(None)).await.unwrap();
    let seen = script.seen();
    let has = |i: usize| {
        (
            seen[i]
                .system
                .as_deref()
                .unwrap()
                .contains(BLUEPRINT_INSTRUCTION),
            seen[i].prompt.contains("## 설계서"),
        )
    };
    assert_eq!(has(0), (false, false));
    assert_eq!(has(1), (true, true)); // 켜면 실린다 — 위 단언이 빈 검사가 아니다
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_while_turn_running_is_busy() {
    let env = env(Script::slow(300, [ans("끝", json!([]))]));
    let id = save(&env, ai_only_wf(&env.dir("watch")), false);
    put(&env, &id, blueprint(Some("/x/in")));
    let b = builder(&env);
    let data = json!({"name": "날짜.pdf"});
    let first = send(&env.sup, &b, &id, "하나", ai(), Limits::default(), &|_| {});
    let second = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        submit(&b, &id, 1, data.clone()).await
    };
    let (a, s) = tokio::join!(first, second);
    assert!(a.is_ok());
    assert_eq!(s.unwrap_err(), CHAT_BUSY);
    assert_eq!(stored_bp(&env, &id).status, BlueprintStatus::Active);
    // 턴이 끝난 뒤에는 제출된다.
    let r = submit(&b, &id, 1, data).await.unwrap();
    assert!(r.ok, "{r:?}");
    let bp = stored_bp(&env, &id);
    assert_eq!(
        (bp.status, bp.build_pending),
        (BlueprintStatus::Submitted, true)
    );
}

#[test]
fn tampered_blueprint_is_hidden_from_screen() {
    let mut f = ChatFile::default();
    f.state.blueprint = Some(blueprint(Some("/x/in")));
    assert_eq!(visible_chat(f.clone()), f);
    let gate = f
        .state
        .blueprint
        .as_mut()
        .unwrap()
        .components
        .iter_mut()
        .find(|c| c["id"] == "watch")
        .unwrap();
    gate["target"] = json!({"event": "local.ai", "field": "prompt"});
    assert_eq!(visible_chat(f).state.blueprint, None);
}
