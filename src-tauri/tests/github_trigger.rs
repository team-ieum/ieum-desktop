//! GitHub 트리거 확인 루프. GitHub은 가짜 서버, 폴링 토큰은 가짜, 시계도 가짜.
//! 실행 확인은 트리거 뒤 `transform.map` 한 노드로 — 트리거 출력이 기록에 남는지 본다.

mod github_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use github_support::{FakeWatch, Log, Req, bind, fake_watch, resp, serve};
use ieum_desktop_lib::store::{RunTrigger, Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::{AppEvent, Deps, Supervisor, WatchState};
use ieum_engine::Workflow;
use ieum_local::WatchTiming;
use ieum_local::ai::{AiAnswer, AiError, AiProvider, AiRequest, AiRunner, ChunkFn};
use ieum_local::github::watch::{CreatedIssues, Seen, WatchEntry};
use ieum_local::github::{GithubApi, GithubRunner};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Semaphore;

/// 허가를 받아야 답하는 가짜 AI — 실행 하나를 붙잡아 다음 실행을 줄에 세운다.
struct GateAi {
    gate: Semaphore,
    calls: AtomicUsize,
}

#[async_trait]
impl AiProvider for GateAi {
    async fn ask(&self, _req: AiRequest, _chunk: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.gate.acquire().await.unwrap().forget();
        Ok(AiAnswer {
            text: r#"{"folder": "x"}"#.into(),
            usage: None,
        })
    }
}

type Hook = Box<dyn FnOnce() + Send>;

/// 가짜 GitHub 상태. 모든 저장소가 같은 이슈 목록을 준다.
#[derive(Default)]
struct Gh {
    /// 최신 먼저.
    issues: Vec<Value>,
    /// 목록 대신 이 상태·추가 헤더로 답한다.
    status: Option<(u16, String)>,
    /// 200·304 응답에 붙일 헤더(GitHub는 304에도 한도 헤더를 준다).
    extra: String,
    /// 이 토큰으로 오면 401.
    bad_token: Option<String>,
    /// 다음 응답을 만든 뒤·보내기 전에 한 번 부른다.
    hook: Option<Hook>,
}

fn respond(gh: Arc<Mutex<Gh>>) -> impl Fn(&Req) -> Vec<u8> + Send + 'static {
    move |req| {
        let (res, hook) = {
            let mut g = gh.lock().unwrap();
            let auth = req.header("authorization").unwrap_or_default();
            let res = if g
                .bad_token
                .as_ref()
                .is_some_and(|b| auth == format!("Bearer {b}"))
            {
                resp(401, "", b"{}")
            } else if let Some((s, extra)) = &g.status {
                resp(*s, extra, b"{}")
            } else if req.path.contains("/issues") {
                let etag = format!("\"{}\"", serde_json::to_string(&g.issues).unwrap().len());
                if req.header("if-none-match") == Some(etag.clone()) {
                    resp(304, &format!("etag: {etag}\r\n{}", g.extra), b"")
                } else {
                    let body = serde_json::to_vec(&g.issues).unwrap();
                    resp(200, &format!("etag: {etag}\r\n{}", g.extra), &body)
                }
            } else {
                resp(200, &g.extra, b"[]")
            };
            (res, g.hook.take())
        };
        if let Some(h) = hook {
            h();
        }
        res
    }
}

struct Env {
    _t: TempDir,
    sup: Supervisor,
    store: Store,
    clock: Arc<Mutex<DateTime<Utc>>>,
    created: Arc<CreatedIssues>,
    watch: Arc<FakeWatch>,
    gh: Arc<Mutex<Gh>>,
    log: Log,
    ai: Arc<GateAi>,
    events: Arc<Mutex<Vec<AppEvent>>>,
}

impl Env {
    fn now(&self) -> DateTime<Utc> {
        *self.clock.lock().unwrap()
    }

    fn advance(&self, d: chrono::Duration) {
        *self.clock.lock().unwrap() += d;
    }

    fn set_issues(&self, numbers_and_age: &[(u64, chrono::Duration)]) {
        let now = self.now();
        self.gh.lock().unwrap().issues = numbers_and_age
            .iter()
            .map(|&(n, age)| issue(n, now - age))
            .collect();
    }

    fn requests(&self) -> Vec<Req> {
        self.log.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.log.lock().unwrap().clear();
        self.watch.calls.lock().unwrap().clear();
    }

    fn state(&self, id: &str) -> Option<WatchEntry> {
        self.store.get_github_watch().unwrap().remove(id)
    }

    fn issue_max(&self, id: &str) -> u64 {
        match self.state(id).expect("상태 없음").seen {
            Seen::Issue { issue_max } => issue_max,
            s => panic!("이슈 상태가 아님: {s:?}"),
        }
    }

    fn runs(&self, id: &str) -> usize {
        self.store.list_runs(Some(id)).unwrap().len()
    }

    /// 기록된 실행들의 트리거 `issueNumber`(오래된 것부터).
    fn run_numbers(&self, id: &str) -> Vec<u64> {
        let mut runs = self.store.list_runs(Some(id)).unwrap();
        runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        runs.iter()
            .map(|s| {
                let run = self.store.get_run(&s.run_id).unwrap();
                run.run.nodes["t"].output.as_ref().unwrap()["issueNumber"]
                    .as_u64()
                    .unwrap()
            })
            .collect()
    }

    fn entry(&self, id: &str) -> (WatchState, Option<String>) {
        let e = self
            .sup
            .list_workflows()
            .unwrap()
            .into_iter()
            .find(|e| e.workflow.id == id)
            .unwrap();
        (e.watch, e.watch_message)
    }

    /// 감독을 거치지 않고 저장소에 바로 넣는다 — 저장이 루프를 깨우지 않아 확인을 테스트가 직접 부른다.
    fn add(&self, wf: Workflow) -> String {
        self.store
            .save_workflow(StoredWorkflow {
                id: String::new(),
                name: "깃허브".into(),
                enabled: true,
                workflow: wf,
                updated_at: String::new(),
            })
            .unwrap()
            .id
    }

    /// 감독으로 저장한다(켜면 확인 루프를 바로 깨운다).
    fn save(&self, wf: Workflow, enabled: bool) -> String {
        self.sup
            .save_workflow(StoredWorkflow {
                id: String::new(),
                name: "깃허브".into(),
                enabled,
                workflow: wf,
                updated_at: String::new(),
            })
            .unwrap()
            .id
    }

    fn set_enabled(&self, id: &str, enabled: bool) {
        let mut wf = self.sup.store().get_workflow(id).unwrap();
        wf.enabled = enabled;
        self.sup.save_workflow(wf).unwrap();
    }
}

fn issue(n: u64, created: DateTime<Utc>) -> Value {
    json!({
        "number": n,
        "title": format!("이슈 {n}"),
        "body": "",
        "html_url": format!("https://github.com/o/r/issues/{n}"),
        "repository_url": "https://api.github.com/repos/o/r",
        "user": {"login": "u"},
        "labels": [],
        "created_at": created.to_rfc3339(),
    })
}

/// 새 이슈 트리거 → 데이터 변환.
fn issue_wf(owner: &str, repo: &str) -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "github.new_issue", "config": {"owner": owner, "repo": repo}},
            {"id": "m", "type": "TRANSFORM", "event": "transform.map", "config": {"mappings": {"n": "{{nodes.t.output.issueNumber}}"}}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "m"}]
    }))
    .unwrap()
}

/// 새 이슈 트리거 → 가짜 AI(허가 대기).
fn issue_ai_wf() -> Workflow {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "github.new_issue", "config": {"owner": "o", "repo": "r"}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "fake", "prompt": "분류", "outputFields": [{"key": "folder", "type": "text"}]}}
        ],
        "edges": [{"id": "e1", "source": "t", "target": "a"}]
    }))
    .unwrap()
}

fn mins(m: i64) -> chrono::Duration {
    chrono::Duration::minutes(m)
}

/// 확인 간격 1시간 — 루프는 저장(깨우기) 때만 돈다.
async fn env() -> Env {
    env_with(Duration::from_secs(3600)).await
}

async fn env_with(poll: Duration) -> Env {
    let (l, base) = bind().await;
    let gh = Arc::new(Mutex::new(Gh::default()));
    let log = serve(l, respond(gh.clone()));
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("trash")).unwrap();
    let store = Store::new(root.join("data"));
    let clock = Arc::new(Mutex::new("2026-11-16T01:30:00Z".parse().unwrap()));
    let created = Arc::new(CreatedIssues::in_memory());
    let watch = fake_watch("t");
    let ai = Arc::new(GateAi {
        gate: Semaphore::new(0),
        calls: AtomicUsize::new(0),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let (c, ev) = (clock.clone(), events.clone());
    let sup = Supervisor::start(Deps {
        store: store.clone(),
        app_data: root.join("data"),
        home: root.clone(),
        trash_dir: root.join("trash"),
        ai: Arc::new(AiRunner::empty().with_provider("fake", ai.clone())),
        github: Arc::new(
            GithubRunner::new(GithubApi::with_base(&base), github_support::no_token())
                .with_created(created.clone()),
        ),
        github_watch: watch.clone(),
        github_created: created.clone(),
        github_poll: poll,
        emit: Arc::new(move |e| ev.lock().unwrap().push(e)),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck: Duration::from_secs(3600),
        now: Arc::new(move || *c.lock().unwrap()),
    })
    .unwrap();
    Env {
        _t: t,
        sup,
        store,
        clock,
        created,
        watch,
        gh,
        log,
        ai,
        events,
    }
}

async fn wait_until(max: Duration, f: impl Fn() -> bool) -> bool {
    let end = Instant::now() + max;
    while Instant::now() < end {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    f()
}

const WAIT: Duration = Duration::from_secs(5);

#[tokio::test(flavor = "multi_thread")]
async fn github_first_enable_baselines_without_run() {
    let env = env().await;
    env.set_issues(&[(3, mins(3)), (2, mins(4)), (1, mins(5))]);
    let id = env.save(issue_wf("o", "r"), true);
    assert!(wait_until(WAIT, || env.state(&id).is_some()).await);
    assert_eq!(env.issue_max(&id), 3);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.runs(&id), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_new_issue_runs_once() {
    let env = env_with(Duration::from_millis(100)).await;
    env.set_issues(&[(3, mins(3)), (2, mins(4)), (1, mins(5))]);
    let id = env.save(issue_wf("o", "r"), true);
    assert!(wait_until(WAIT, || env.state(&id).is_some()).await);

    env.set_issues(&[(4, mins(1)), (3, mins(3)), (2, mins(4)), (1, mins(5))]);
    assert!(wait_until(WAIT, || env.runs(&id) == 1).await);
    let summary = &env.store.list_runs(Some(&id)).unwrap()[0];
    assert_eq!(summary.trigger, Some(RunTrigger::Github));
    assert_eq!(env.run_numbers(&id), [4]);
    assert_eq!(env.issue_max(&id), 4);

    // 다음 확인들은 304(바뀐 것 없음) — 더 돌지 않는다.
    env.clear();
    assert!(
        wait_until(WAIT, || env
            .requests()
            .iter()
            .any(|r| r.header("if-none-match").is_some()))
        .await
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.runs(&id), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_issue_created_by_app_skipped() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.add(issue_wf("o", "r"));
    env.sup.github_tick_for_test().await;
    assert_eq!(env.issue_max(&id), 3);

    env.created.record("o", "r", 4, env.now());
    env.set_issues(&[(4, mins(1)), (3, mins(3))]);
    env.sup.github_tick_for_test().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.runs(&id), 0);
    assert_eq!(env.issue_max(&id), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_restart_runs_only_within_grace() {
    let env = env().await;
    env.set_issues(&[(3, mins(300))]);
    let id = env.add(issue_wf("o", "r"));
    env.sup.github_tick_for_test().await;
    // 앱이 3시간 꺼져 있었던 것처럼.
    let mut entry = env.state(&id).unwrap();
    entry.last_checked_at = env.now() - chrono::Duration::hours(3);
    env.store.set_github_watch(&id, &entry).unwrap();

    env.set_issues(&[(4, mins(60)), (5, mins(121)), (3, mins(300))]);
    env.sup.github_tick_for_test().await;
    assert!(wait_until(WAIT, || env.runs(&id) == 1).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.run_numbers(&id), [4]);
    assert_eq!(env.issue_max(&id), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_reenable_resets_baseline() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.save(issue_wf("o", "r"), true);
    assert!(wait_until(WAIT, || env.state(&id).is_some()).await);

    env.set_enabled(&id, false);
    assert!(env.state(&id).is_none());
    env.set_issues(&[(4, mins(1)), (3, mins(3))]);
    env.set_enabled(&id, true);
    assert!(wait_until(WAIT, || env.state(&id).is_some()).await);
    assert_eq!(env.issue_max(&id), 4);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.runs(&id), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_toggle_during_poll_discards() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.add(issue_wf("o", "r"));
    env.sup.github_tick_for_test().await;
    assert_eq!(env.issue_max(&id), 3);

    // 응답이 가기 전에 사용자가 껐다 켠다 — 그 뒤 요청(깨운 확인)은 500이라 상태를 쓰지 않는다.
    env.set_issues(&[(4, mins(1)), (3, mins(3))]);
    let sup: Arc<OnceLock<Supervisor>> = Arc::new(OnceLock::from(env.sup.clone()));
    let (gh, wid) = (env.gh.clone(), id.clone());
    env.gh.lock().unwrap().hook = Some(Box::new(move || {
        let sup = sup.get().unwrap();
        for enabled in [false, true] {
            let mut wf = sup.store().get_workflow(&wid).unwrap();
            wf.enabled = enabled;
            sup.save_workflow(wf).unwrap();
        }
        gh.lock().unwrap().status = Some((500, String::new()));
    }));
    env.sup.github_tick_for_test().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(env.state(&id).is_none(), "옛 확인 결과가 상태를 썼다");
    assert_eq!(env.runs(&id), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_zero_workflows_no_token_call() {
    let env = env().await;
    env.add(
        serde_json::from_value(json!({
            "nodes": [{"id": "t", "type": "TRIGGER", "event": "manual.run", "config": {}}],
            "edges": []
        }))
        .unwrap(),
    );
    env.sup.github_tick_for_test().await;
    assert!(env.watch.calls.lock().unwrap().is_empty());
    assert!(env.requests().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn github_errors_show_watch_error() {
    let env = env().await;
    let id = env.add(issue_wf("o", "r"));
    env.gh.lock().unwrap().status = Some((404, String::new()));
    env.sup.github_tick_for_test().await;
    assert_eq!(
        env.entry(&id),
        (
            WatchState::WatchError,
            Some("저장소를 찾을 수 없어요.".into())
        )
    );
    let failed = env.events.lock().unwrap().iter().any(
        |e| matches!(e, AppEvent::WatchStatus { workflow_id, ok: false, .. } if *workflow_id == id),
    );
    assert!(failed);

    env.gh.lock().unwrap().status = None;
    env.sup.github_tick_for_test().await;
    assert_eq!(env.entry(&id), (WatchState::Watching, None));
    let recovered = env.events.lock().unwrap().iter().any(
        |e| matches!(e, AppEvent::WatchStatus { workflow_id, ok: true, .. } if *workflow_id == id),
    );
    assert!(recovered);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_rate_limit_pauses() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.add(issue_wf("o", "r"));
    env.sup.github_tick_for_test().await;

    // 01:30Z + 1시간 = 서울 11:30
    let reset = (env.now() + chrono::Duration::hours(1)).timestamp();
    env.gh.lock().unwrap().status = Some((
        403,
        format!("x-ratelimit-remaining: 0\r\nx-ratelimit-reset: {reset}\r\n"),
    ));
    env.sup.github_tick_for_test().await;
    assert_eq!(
        env.entry(&id),
        (
            WatchState::WatchError,
            Some("GitHub 한도 초과 — 11:30에 다시 확인".into())
        )
    );
    env.clear();
    env.sup.github_tick_for_test().await;
    assert!(env.requests().is_empty(), "쉬는 동안 요청했다");

    // 풀린 뒤 남은 한도가 10% 아래 → 예방 휴식.
    env.advance(chrono::Duration::hours(2));
    let reset = (env.now() + mins(30)).timestamp();
    {
        let mut g = env.gh.lock().unwrap();
        g.status = None;
        g.extra = format!(
            "x-ratelimit-remaining: 400\r\nx-ratelimit-limit: 5000\r\nx-ratelimit-reset: {reset}\r\n"
        );
    }
    env.sup.github_tick_for_test().await;
    assert_eq!(env.requests().len(), 1);
    // 03:30Z + 30분 = 서울 13:00
    assert_eq!(
        env.entry(&id),
        (
            WatchState::WatchError,
            Some("GitHub 요청 여유가 적어 13:00에 다시 확인해요".into())
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn github_shared_repo_one_request() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let a = env.add(issue_wf("O", "r"));
    let b = env.add(issue_wf("o", "R"));
    env.sup.github_tick_for_test().await;
    assert_eq!(env.requests().len(), 1);
    assert_eq!((env.issue_max(&a), env.issue_max(&b)), (3, 3));

    env.clear();
    env.set_issues(&[(4, mins(1)), (3, mins(3))]);
    env.sup.github_tick_for_test().await;
    assert_eq!(env.requests().len(), 1);
    assert!(wait_until(WAIT, || env.runs(&a) == 1 && env.runs(&b) == 1).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_401_rechecks_then_errors() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let a = env.add(issue_wf("o", "a"));
    let b = env.add(issue_wf("o", "b"));
    env.sup.github_tick_for_test().await;
    assert_eq!(env.requests().len(), 2);

    env.clear();
    env.gh.lock().unwrap().bad_token = Some("t".into());
    *env.watch.forced.lock().unwrap() = None;
    env.sup.github_tick_for_test().await;
    assert_eq!(env.requests().len(), 1, "다시 확인 실패 뒤에도 요청했다");
    assert_eq!(*env.watch.calls.lock().unwrap(), [false, true]);
    let need = (
        WatchState::WatchError,
        Some("GitHub 연결이 필요해요.".to_string()),
    );
    assert_eq!(env.entry(&a), need);
    assert_eq!(env.entry(&b), need);
}

#[tokio::test(flavor = "multi_thread")]
async fn github_401_retries_with_new_token() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.add(issue_wf("o", "r"));
    env.sup.github_tick_for_test().await;

    env.clear();
    env.gh.lock().unwrap().bad_token = Some("t".into());
    *env.watch.forced.lock().unwrap() = Some("new".into());
    env.set_issues(&[(4, mins(1)), (3, mins(3))]);
    env.sup.github_tick_for_test().await;
    let auth: Vec<String> = env
        .requests()
        .iter()
        .map(|r| r.header("authorization").unwrap())
        .collect();
    assert_eq!(auth, ["Bearer t", "Bearer new"]);
    assert_eq!(*env.watch.calls.lock().unwrap(), [false, true]);
    assert!(wait_until(WAIT, || env.runs(&id) == 1).await);
    assert_eq!(env.entry(&id), (WatchState::Watching, None));
}

#[tokio::test(flavor = "multi_thread")]
async fn github_304_updates_last_checked() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.add(issue_wf("o", "r"));
    env.sup.github_tick_for_test().await;
    assert_eq!(env.state(&id).unwrap().last_checked_at, env.now());

    env.advance(mins(1));
    env.clear();
    env.sup.github_tick_for_test().await;
    assert!(env.requests()[0].header("if-none-match").is_some());
    assert_eq!(env.state(&id).unwrap().last_checked_at, env.now());
}

#[tokio::test(flavor = "multi_thread")]
async fn github_late_job_dropped_after_disable() {
    let env = env().await;
    env.set_issues(&[(3, mins(3))]);
    let id = env.add(issue_ai_wf());
    env.sup.github_tick_for_test().await;

    env.set_issues(&[(5, mins(1)), (4, mins(2)), (3, mins(3))]);
    env.sup.github_tick_for_test().await;
    // 4번이 AI에서 붙잡힌 동안 5번은 줄에서 기다린다.
    assert!(wait_until(WAIT, || env.ai.calls.load(Ordering::SeqCst) == 1).await);
    env.set_enabled(&id, false);
    env.ai.gate.add_permits(10);
    assert!(wait_until(WAIT, || env.runs(&id) == 1).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(env.ai.calls.load(Ordering::SeqCst), 1);
    assert_eq!(env.run_numbers(&id), [4]);
}
