//! 설계서 실측 기록(`metrics`). 줄에는 코드·숫자만 — 사용자 글·AI 글·오류 원문·경로·키 id는 싣지 않는다.

mod github_support;

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use github_support::{no_github, no_watch};
use ieum_desktop_lib::builder_host::{AppHost, no_pick};
use ieum_desktop_lib::chat::{ChatOpts, send_with, submit};
use ieum_desktop_lib::credentials::AccountCredentials;
use ieum_desktop_lib::metrics::{ai_error_kind, append, turn_lines};
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{Deps, GITHUB_POLL, Supervisor};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{
    AiAnswer, AiError, AiProvider, AiRequest, AiRunner, AiUsage, ChunkFn, KeyStore,
};
use ieum_local::builder::{AiDefault, Blueprint, BlueprintStatus, Builder, apply_ui};
use ieum_local::chat::{CallOutcome, CallStat, Limits, OpStat, TurnResult};
use ieum_local::github::watch::CreatedIssues;
use serde_json::{Value, json};
use tempfile::TempDir;

/// 답 글자를 차례로 낸다. 떨어지면 빈 답.
struct Script(Mutex<VecDeque<String>>);

impl Script {
    fn push(&self, answer: String) {
        self.0.lock().unwrap().push_back(answer);
    }
}

#[async_trait]
impl AiProvider for Script {
    async fn ask(&self, _req: AiRequest, _c: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
        let text = self
            .0
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ans("", json!([])));
        Ok(AiAnswer {
            text,
            usage: Some(usage()),
        })
    }
}

fn usage() -> AiUsage {
    AiUsage {
        source: "anthropic".into(),
        model: Some("claude-opus-5-5".into()),
        input_tokens: 10,
        output_tokens: 2,
        cache_read_tokens: Some(3),
        cache_write_tokens: None,
        credential: Some("cafe1234".into()),
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
}

/// 임시 폴더 = 가짜 홈. 앱 데이터는 `data/`.
fn env(script: Arc<Script>) -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    fs::create_dir_all(root.join("watch")).unwrap();
    let store = Store::new(root.join("data"));
    let creds = AccountCredentials::new(root.join("data"), "0123456789abcdef", Arc::new(NoKeys));
    let ai = AiRunner::empty().with_provider("fake", script);
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
        emit: Arc::new(|_| {}),
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
    }
}

/// 트리거 → AI만. 설계서(Active, 감시 폴더 칸 경로 `/h/in`)를 붙여 둔다.
fn save_with_blueprint(env: &Env) -> String {
    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": env.root.join("watch")}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "분류", "outputFields": [{"key": "folder", "type": "text"}]}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap();
    let id = env
        .sup
        .save_workflow(StoredWorkflow {
            id: String::new(),
            name: "정리".into(),
            enabled: false,
            workflow: wf,
            updated_at: String::new(),
        })
        .unwrap()
        .id;
    env.store
        .update_chat(&id, |c| c.state.blueprint = Some(blueprint()))
        .unwrap();
    id
}

fn blueprint() -> Blueprint {
    let msg = json!({"createSurface": {
        "surfaceId": "blueprint", "catalogId": "ieum.chat.blueprint/v1",
        "components": [
            {"id": "root", "component": "Blueprint", "title": "정리", "children": ["watch", "go"]},
            {"id": "watch", "component": "AppGate", "slot": "watch",
                "target": {"event": "local.folder_new_file", "field": "folder"}},
            {"id": "go", "component": "SubmitButton"}
        ],
        "dataModel": {}
    }});
    let mut b = apply_ui(None, &[msg]).unwrap();
    b.slots.get_mut("watch").unwrap().path = Some("/h/in".into());
    b
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

/// 한 턴: `pre` 동작 뒤 노드 이름을 바꾸고(설계서를 거치지 않은 변경 → 설계서 Cancelled) 끝낸다.
async fn bypass_turn(
    env: &Env,
    script: &Script,
    blueprint: bool,
    metrics: Option<PathBuf>,
    pre: &[Value],
) -> String {
    let id = save_with_blueprint(env);
    let node = env.store.get_workflow(&id).unwrap().workflow.nodes[1]
        .id
        .clone();
    let mut actions = pre.to_vec();
    actions.push(json!({"op": "rename", "args": {"id": node, "label": "분류기"}}));
    script.push(ans("바꿀게요", Value::Array(actions)));
    script.push(ans("바꿨어요", json!([])));
    let turn = send_with(
        &env.sup,
        &builder(env),
        &id,
        "비밀 계획대로 이름을 바꿔 줘",
        ai(),
        Limits::default(),
        &|_| {},
        ChatOpts {
            blueprint,
            metrics,
            ..ChatOpts::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(turn.error, None);
    id
}

fn events(lines: &[Value]) -> Vec<&str> {
    lines.iter().filter_map(|l| l["event"].as_str()).collect()
}

fn read_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn keys(v: &Value) -> BTreeSet<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_off_writes_nothing() {
    let script = Arc::new(Script(Mutex::default()));
    let env = env(script.clone());
    bypass_turn(&env, &script, true, None, &[]).await;
    assert!(env.root.join("data/chats").exists()); // 턴은 돌았다
    assert!(!env.root.join("data/metrics").exists());
}

#[test]
fn turn_lines_hold_codes_not_text() {
    let err = "Anthropic API에 연결하지 못했어요: secret.example";
    let t = TurnResult {
        say: "사용자 글: 비밀 계획".into(),
        cards: Vec::new(),
        usage: vec![usage()],
        error: Some(err.into()),
        cut_short: true,
        calls: vec![
            CallStat {
                ms: 12,
                outcome: CallOutcome::Ok,
                usage: Some(usage()),
            },
            CallStat {
                ms: 5,
                outcome: CallOutcome::AiError(err.into()),
                usage: None,
            },
        ],
        ops: vec![OpStat {
            op: "set".into(),
            ok: false,
            code: Some(ieum_local::builder::Code::Invalid),
            rule: None,
            errors: 2,
            ui: None,
        }],
    };
    let lines = turn_lines("2026-11-24T00:00:00.000Z", "wf", "blueprint", &t);
    let all = serde_json::to_string(&lines).unwrap();
    for leak in ["secret.example", "비밀 계획", "cafe1234", "credential"] {
        assert!(!all.contains(leak), "{leak}: {all}");
    }
    assert_eq!(
        keys(&lines[0]),
        BTreeSet::from([
            "at",
            "workflowId",
            "mode",
            "kind",
            "source",
            "model",
            "ms",
            "in",
            "out",
            "cacheRead",
            "cacheWrite",
            "outcome"
        ])
    );
    assert_eq!(
        (
            &lines[0]["outcome"],
            &lines[0]["in"],
            &lines[0]["cacheRead"]
        ),
        (&json!("ok"), &json!(10), &json!(3))
    );
    assert_eq!(lines[1]["outcome"], "ai:connect");
    assert_eq!(lines[1]["source"], Value::Null);
    assert_eq!(
        keys(&lines[2]),
        BTreeSet::from([
            "at",
            "workflowId",
            "mode",
            "kind",
            "op",
            "ok",
            "code",
            "rule",
            "errors",
            "ui"
        ])
    );
    assert_eq!(
        (&lines[2]["code"], &lines[2]["errors"]),
        (&json!("INVALID"), &json!(2))
    );
    assert_eq!(
        (&lines[3]["kind"], &lines[3]["event"]),
        (&json!("event"), &json!("turn_cut_short"))
    );
    assert_eq!(
        keys(&lines[3]),
        BTreeSet::from(["at", "workflowId", "mode", "kind", "event"])
    );
    assert_eq!(lines.len(), 4);
}

#[test]
fn ai_error_kind_classifies_known_messages() {
    let cases = [
        (
            "Anthropic API가 300초 안에 답하지 않아 멈췄어요(시간 초과).",
            "timeout",
        ),
        ("API 키를 확인해 주세요. (Anthropic HTTP 401)", "http_4xx"),
        (
            "AI 서비스에 문제가 있어요. (OpenAI HTTP 503: overloaded)",
            "http_5xx",
        ),
        ("Gemini API에 연결하지 못했어요: dns error", "connect"),
        ("키가 없어요", "other"),
    ];
    for (m, kind) in cases {
        assert_eq!(ai_error_kind(m), kind, "{m}");
    }
}

#[test]
fn append_creates_folder_and_appends_lines() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("metrics/chat.jsonl");
    append(&path, &[json!({"kind": "event", "event": "a"})]).unwrap();
    append(&path, &[json!({"kind": "event", "event": "b"})]).unwrap();
    let got: Vec<Value> = read_lines(&path)
        .into_iter()
        .map(|v| v["event"].clone())
        .collect();
    assert_eq!(got, [json!("a"), json!("b")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_on_records_calls_ops_and_events() {
    let script = Arc::new(Script(Mutex::default()));
    let env = env(script.clone());
    let path = env.root.join("data/metrics/chat.jsonl");
    bypass_turn(&env, &script, true, Some(path.clone()), &[]).await;
    let lines = read_lines(&path);
    let kinds: BTreeSet<&str> = lines.iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, BTreeSet::from(["call", "op", "event"]));
    assert!(lines.iter().all(|l| l["mode"] == "blueprint"));
    // 카드를 안 고치고 보냈고, 설계서 대신 바로 고쳤다.
    assert_eq!(events(&lines), ["blueprint_ignored", "blueprint_bypassed"]);
    let raw = fs::read_to_string(&path).unwrap();
    for leak in ["/h/in", "비밀 계획", "바꿨어요", "분류기", "cafe1234"] {
        assert!(!raw.contains(leak), "{leak}: {raw}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn classic_turn_records_no_blueprint_events() {
    let script = Arc::new(Script(Mutex::default()));
    let env = env(script.clone());
    let path = env.root.join("data/metrics/chat.jsonl");
    let id = bypass_turn(&env, &script, false, Some(path.clone()), &[]).await;
    // 설계서는 Active·touched false였고 직접 편집으로 Cancelled됐다 — 설계서 모드였다면 사건 둘.
    let status = env
        .store
        .get_chat(&id)
        .unwrap()
        .state
        .blueprint
        .unwrap()
        .status;
    assert_eq!(status, BlueprintStatus::Cancelled);
    let lines = read_lines(&path);
    assert!(lines.iter().any(|l| l["kind"] == "op"), "{lines:?}");
    assert!(lines.iter().all(|l| l["mode"] == "classic"));
    assert_eq!(events(&lines), Vec::<&str>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_create_still_counts_as_bypass() {
    let script = Arc::new(Script(Mutex::default()));
    let env = env(script.clone());
    let path = env.root.join("data/metrics/chat.jsonl");
    let bad = json!({"op": "ui", "args": {"messages": [{"createSurface": {
        "surfaceId": "blueprint", "catalogId": "bad/v0", "components": []
    }}]}});
    bypass_turn(&env, &script, true, Some(path.clone()), &[bad]).await;
    let lines = read_lines(&path);
    assert!(
        lines
            .iter()
            .any(|l| l["op"] == "ui" && l["ui"] == "create" && l["ok"] == false),
        "{lines:?}"
    );
    assert_eq!(events(&lines), ["blueprint_ignored", "blueprint_bypassed"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_records_event_when_metrics_on() {
    let env = env(Arc::new(Script(Mutex::default())));
    let id = save_with_blueprint(&env);
    let path = env.root.join("data/metrics/chat.jsonl");
    let r = submit(&builder(&env), &id, 1, json!({}), Some(&path))
        .await
        .unwrap();
    assert!(r.ok, "{r:?}");
    let lines = read_lines(&path);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (&lines[0]["kind"], &lines[0]["event"], &lines[0]["mode"]),
        (
            &json!("event"),
            &json!("blueprint_submitted"),
            &json!("blueprint")
        )
    );
    assert!(!fs::read_to_string(&path).unwrap().contains("/h/in"));
}

#[tokio::test(flavor = "multi_thread")]
async fn tampered_blueprint_counts_no_attempt_and_records_no_events() {
    let script = Arc::new(Script(Mutex::default()));
    let env = env(script.clone());
    let id = save_with_blueprint(&env);
    let path = env.root.join("data/metrics/chat.jsonl");
    // 고아 컴포넌트를 더한 변조본 — `validate_stored`를 못 넘어 화면·프롬프트에는 없는 설계서다.
    let lost =
        json!({"id": "lost", "component": "TextInput", "label": "고아", "value": {"@path": "/x"}});
    for (status, pending) in [
        (BlueprintStatus::Submitted, true),
        (BlueprintStatus::Submitted, false),
        (BlueprintStatus::Active, false),
    ] {
        env.store
            .update_chat(&id, |c| {
                let b = c.state.blueprint.as_mut().unwrap();
                b.status = status;
                b.build_pending = pending;
                if b.components.len() == 3 {
                    b.components.push(lost.clone());
                }
            })
            .unwrap();
        script.push(ans("네", json!([])));
        send_with(
            &env.sup,
            &builder(&env),
            &id,
            "만들어 줘",
            ai(),
            Limits::default(),
            &|_| {},
            ChatOpts {
                blueprint: true,
                metrics: Some(path.clone()),
                ..ChatOpts::default()
            },
        )
        .await
        .unwrap();
    }
    let b = env.store.get_chat(&id).unwrap().state.blueprint.unwrap();
    assert_eq!(b.build_attempts, 0);
    assert_eq!(events(&read_lines(&path)), Vec::<&str>::new());
}
