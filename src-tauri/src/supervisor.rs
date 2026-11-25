//! 실행 감독: 켜진 폴더 트리거 워크플로우마다 감시 하나, 워크플로우별 실행 줄(한 번에 하나), 실행 기록·되돌리기.
//! Tauri 없이 돈다 — 저장소·휴지통·AI 러너·이벤트 송신을 `Deps`로 주입한다.
//! GitHub 트리거 확인 루프는 `supervisor/github.rs`.

mod github;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use ieum_engine::schedule::{
    Decision, GRACE, Schedule, decide, schedule_cron, schedule_output, to_quartz, triggered_at,
};
use ieum_engine::{
    BuiltinRunners, Catalog, ExecEvent, GITHUB_EVENTS, GITHUB_TRIGGERS, Node, NodeRecord,
    NodeRunner, NodeStatus, Outputs, RunResult, RunStatus, Runners, ValidationError, Workflow,
    node_errors, referenced_node_ids, run_node, run_workflow, validate,
};
use ieum_local::ai::AiRunner;
use ieum_local::builder::{HttpLine, TestRun, TestTrigger};
use ieum_local::github::watch::{CreatedIssues, WatchEntry, WatchKind, config_hash};
use ieum_local::github::{GithubRunner, WatchToken, is_path_name};
use ieum_local::{
    AllowedRoots, FileMoveRunner, FileOp, FileRenameRunner, FileTrashRunner, FolderWatcher,
    HttpRunner, Journal, NewFile, RecentWrites, UndoReport, WatchTiming, roots_for_workflow, undo,
    user_folders, validate_user_folder,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::runtime::Handle;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::http_confirm::HttpConfirmGate;
use crate::key_limits::{KeyLimitNotice, after_use, used_keys};
use crate::store::{
    RunTrigger, Sample, SampleStatus, Store, StoredRun, StoredWorkflow, clip_http_bodies, now,
};

/// 워크플로우 하나의 대기 실행 상한. 넘으면 가장 오래된 것부터 버린다.
pub const QUEUE_CAP: usize = 100;
/// GitHub 트리거 확인 간격(운영값, `Deps.github_poll`).
pub const GITHUB_POLL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub enum AppEvent {
    /// 화면 이벤트 `exec-event`
    Exec {
        workflow_id: String,
        event: ExecEvent,
    },
    /// 화면 이벤트 `watch-status`
    WatchStatus {
        workflow_id: String,
        ok: bool,
        message: String,
    },
    /// 화면 이벤트 `queue-dropped` — 줄이 넘쳐 버린 실행. `label`은 폴더면 트리거 파일 경로,
    /// GitHub면 `owner/repo #번호`.
    QueueDropped { workflow_id: String, label: String },
    /// 시스템 알림 재료(화면 이벤트 아님) — 실행 시작·끝.
    RunNotice(RunNotice),
    /// 화면 이벤트 `workflow-changed` — 채팅 빌더가 저장본을 고쳤다.
    WorkflowChanged {
        workflow_id: String,
        updated_at: String,
    },
    /// 화면 이벤트 `finish-pending` — 완료 조건 통과, 사람의 [켜기]를 기다린다.
    FinishPending {
        workflow_id: String,
        workflow_name: String,
    },
    /// 화면 이벤트 `http-test-confirm` — 빌더 테스트가 HTTP 요청을 보내기 전 사람 확인을 기다린다.
    HttpTestConfirm {
        workflow_id: String,
        lines: Vec<HttpLine>,
    },
    /// 화면 이벤트 `test-undo-failed` — 테스트가 옮긴 파일 일부를 되돌리지 못했다.
    /// `at_start` = 앱 시작 때 되돌리다 실패했으면 그 워크플로우 이름(지워졌으면 빈 글). 그때는 화면이
    /// 아직 듣지 않으므로 받는 쪽이 화면에 넘기지 않고 바로 시스템 알림을 보낸다.
    TestUndoFailed {
        workflow_id: String,
        run_id: String,
        at_start: Option<String>,
    },
    /// 시스템 알림 재료(화면 이벤트 아님) — API 키가 이번 달 한도의 80%·100%에 닿았다. 실행 알림 토글과 무관.
    KeyLimit(KeyLimitNotice),
}

/// `save_workflow_expecting`이 기대한 `updated_at`과 저장본이 다를 때의 오류 문자열.
pub const STALE: &str = "STALE";

#[derive(Debug, Clone)]
pub struct RunNotice {
    pub workflow_id: String,
    pub workflow_name: String,
    pub run_id: String,
    pub trigger: RunTrigger,
    pub kind: NoticeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeKind {
    Started,
    /// `failed_node` = 실패한 첫 노드의 label(없으면 id). 오류 문자열은 싣지 않는다.
    Finished {
        ok: bool,
        secs: u64,
        failed_node: Option<String>,
    },
}

pub type Emit = Arc<dyn Fn(AppEvent) + Send + Sync>;

pub struct Deps {
    pub store: Store,
    /// 앱 데이터 폴더 전체 — 사용자 폴더 정책의 금지 폴더(계정 폴더만이 아니라).
    pub app_data: PathBuf,
    /// 사용자 폴더 정책의 홈(운영 = 실제 홈).
    pub home: PathBuf,
    pub trash_dir: PathBuf,
    /// `local.ai` 러너(운영 = 키체인을 쓰는 `AiRunner::new()`). 실행마다 그 실행의 허용 폴더를 붙인다.
    pub ai: Arc<AiRunner>,
    /// GitHub 이슈 노드 7개가 함께 쓰는 러너(운영 = gh에서 토큰을 읽는 `GhToken`).
    pub github: Arc<GithubRunner>,
    /// GitHub 트리거 확인 루프의 토큰(운영 = 10분 캐시하는 `CachedGhToken`).
    pub github_watch: Arc<dyn WatchToken>,
    /// 이 앱이 만든 이슈 기억 — `github` 러너와 같은 것을 넣는다(새 이슈 트리거가 자기 이슈를 거른다).
    pub github_created: Arc<CreatedIssues>,
    /// GitHub 트리거 확인 간격(운영 `GITHUB_POLL`).
    pub github_poll: Duration,
    pub emit: Emit,
    pub timing: WatchTiming,
    /// 감시 폴더 재확인·스케줄 확인 간격(운영 30초).
    pub recheck: Duration,
    /// 벽시계(운영 = `Utc::now`). 스케줄 차례 판정·트리거 출력에 쓴다.
    pub now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    /// 꺼졌거나 폴더·스케줄·GitHub 트리거가 아님
    Off,
    /// 폴더 감시 중이거나 스케줄·GitHub 트리거가 켜져 있음
    Watching,
    WatchError,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEntry {
    #[serde(flatten)]
    pub workflow: StoredWorkflow,
    pub watch: WatchState,
    pub watch_message: Option<String>,
    /// 다음 스케줄 차례(Asia/Seoul "YYYY-MM-DDTHH:MM:SS"). 꺼짐·스케줄 아님이면 None.
    pub next_run: Option<String>,
}

/// 첫 TRIGGER 노드로 정한 트리거 종류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerKind {
    Folder(PathBuf),
    /// `quartz` = 칸·cron으로 만든 Quartz 식, 만들 수 없으면 이유.
    Schedule {
        event: String,
        quartz: Result<String, String>,
    },
    Manual,
    /// `github.new_issue`·`github.new_pull_request` — 칸의 owner·repo(빈 글일 수 있음).
    Github {
        event: String,
        owner: String,
        repo: String,
    },
    Unsupported,
}

pub fn trigger_kind(wf: &StoredWorkflow) -> TriggerKind {
    let Some(node) = wf.workflow.nodes.iter().find(|n| {
        n.r#type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("TRIGGER"))
    }) else {
        return TriggerKind::Unsupported;
    };
    match node.event.as_str() {
        "local.folder_new_file" => {
            let folder = node.config.get("folder").and_then(Value::as_str);
            TriggerKind::Folder(PathBuf::from(folder.unwrap_or("")))
        }
        "manual.run" => TriggerKind::Manual,
        e if GITHUB_TRIGGERS.contains(&e) => {
            let text = |k: &str| {
                let v = node.config.get(k).and_then(Value::as_str);
                v.unwrap_or("").to_string()
            };
            TriggerKind::Github {
                event: e.to_string(),
                owner: text("owner"),
                repo: text("repo"),
            }
        }
        e if e.starts_with("schedule.") => TriggerKind::Schedule {
            event: e.to_string(),
            quartz: schedule_cron(e, &node.config)
                .and_then(|c| to_quartz(&c))
                .map_err(|e| e.0),
        },
        _ => TriggerKind::Unsupported,
    }
}

/// 켜져 있고 폴더 트리거면 그 폴더.
fn trigger_folder(wf: &StoredWorkflow) -> Option<PathBuf> {
    match trigger_kind(wf) {
        TriggerKind::Folder(f) if wf.enabled => Some(f),
        _ => None,
    }
}

/// 켜진 스케줄 워크플로우의 파싱된 식. 꺼짐·스케줄 아님 = None, 계산 못 함 = Some(Err(이유)).
fn enabled_schedule(wf: &StoredWorkflow) -> Option<Result<Schedule, String>> {
    let TriggerKind::Schedule { quartz, .. } = trigger_kind(wf) else {
        return None;
    };
    if !wf.enabled {
        return None;
    }
    Some(
        quartz
            .and_then(|q| Schedule::parse(&q).map_err(|e| e.0))
            .map_err(|e| format!("스케줄을 계산하지 못했어요: {e}")),
    )
}

fn first_trigger(wf: &Workflow) -> Option<&ieum_engine::Node> {
    wf.nodes.iter().find(|n| {
        n.r#type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("TRIGGER"))
    })
}

fn is_github(wf: &StoredWorkflow) -> bool {
    matches!(trigger_kind(wf), TriggerKind::Github { .. })
}

/// 켜진 GitHub 트리거의 (종류, owner, repo). 꺼짐·비었음·참조식(`{{`)·경로에 못 쓰는 이름이면 None —
/// 요청 경로에 그대로 들어가므로 노드와 같은 검사(`is_path_name`)를 거친다.
fn enabled_github(wf: &StoredWorkflow) -> Option<(WatchKind, String, String)> {
    let TriggerKind::Github { event, owner, repo } = trigger_kind(wf) else {
        return None;
    };
    if !wf.enabled || github_repo_error(&owner, &repo).is_some() {
        return None;
    }
    let kind = if event == "github.new_issue" {
        WatchKind::Issue
    } else {
        WatchKind::Pr
    };
    Some((kind, owner, repo))
}

/// owner·repo를 요청 경로에 못 쓰면 목록 카드에 보일 문구 — 비었거나 참조식이면 고르기 안내, 아니면 형식 안내.
fn github_repo_error(owner: &str, repo: &str) -> Option<&'static str> {
    let unpicked = |s: &str| s.is_empty() || s.contains("{{");
    if unpicked(owner) || unpicked(repo) {
        Some("저장소를 먼저 골라 주세요.")
    } else if !is_path_name(&Value::from(owner)) || !is_path_name(&Value::from(repo)) {
        Some("소유자·저장소 이름이 올바르지 않아요.")
    } else {
        None
    }
}

/// 켜진 GitHub 트리거의 지금 `config_hash`.
fn github_hash(wf: &StoredWorkflow) -> Option<String> {
    enabled_github(wf)?;
    let (event, config) = trigger_sig(wf)?;
    Some(config_hash(&event, &config))
}

/// `now` 다음 차례(Asia/Seoul 로컬 글). 다음 차례가 없으면(지난 연도만) 계산 못 함.
fn next_slot(s: &Schedule, now: DateTime<Utc>) -> Result<String, String> {
    s.next_after(now)
        .map(triggered_at)
        .ok_or_else(|| "스케줄을 계산하지 못했어요: 다음 실행 시각이 없어요".to_string())
}

/// 첫 TRIGGER 노드의 event·칸 — 저장 때 바뀌었는지 본다.
fn trigger_sig(wf: &StoredWorkflow) -> Option<(String, Map<String, Value>)> {
    wf.workflow
        .nodes
        .iter()
        .find(|n| {
            n.r#type
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case("TRIGGER"))
        })
        .map(|n| (n.event.clone(), n.config.clone()))
}

struct Watch {
    folder: PathBuf,
    /// 시작 때 폴더 ino — 지워졌다 다시 생긴 폴더를 알아본다.
    ino: u64,
    watcher: Option<FolderWatcher>,
    error: Option<String>,
}

type Reply = oneshot::Sender<Result<String, String>>;

/// 실행이 어디서 왔는가.
enum JobSource {
    /// 감시가 보냈다 — 그 감시 폴더.
    Watch(PathBuf),
    /// 스케줄 차례.
    Schedule { slot: DateTime<Utc> },
    /// GitHub 확인 루프가 찾은 새 항목 — 넣을 때의 트리거 `config_hash`.
    Github { config_hash: String },
    /// 지금 실행(`run_now`).
    Manual,
    /// 채팅 빌더의 노드 테스트 — 저장본 대신 돌릴 그래프(대상까지 자른 것).
    Test(Workflow),
    /// 노드 패널 테스트 — 편집본의 노드 하나만 돌리고 결과는 `reply`로(실행 이벤트 없음).
    NodeTest {
        wf: Workflow,
        node_id: String,
        file: Option<PathBuf>,
        reply: oneshot::Sender<Result<NodeTestResult, String>>,
    },
}

/// 노드 패널 테스트 결과. 화면 타입은 `src/api.ts` `NodeTestResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeTestStatus {
    Succeeded,
    Failed,
    /// 노드 단위 검증 실패 — 실행하지 않았다.
    Invalid,
    /// 참조하는 앞 노드의 성공 샘플이 없다 — 실행하지 않았다.
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTestResult {
    pub status: NodeTestStatus,
    pub output: Option<Map<String, Value>>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub tested_at: Option<String>,
    /// `Missing`일 때 성공 샘플이 없는 앞 노드 id.
    pub missing: Vec<String>,
    /// 테스트가 한 파일 작업을 되돌렸다(뒤 노드는 원래 경로로 테스트된다).
    pub undone: bool,
    /// 되돌리지 못한 파일 작업 수.
    pub undo_failed: usize,
    /// 실행 기록 id(실제로 실행했을 때만 — 트리거 테스트는 기록이 없다).
    pub run_id: Option<String>,
}

impl NodeTestResult {
    fn new(status: NodeTestStatus) -> Self {
        Self {
            status,
            output: None,
            error: None,
            error_code: None,
            tested_at: None,
            missing: Vec::new(),
            undone: false,
            undo_failed: 0,
            run_id: None,
        }
    }
}

/// 샘플의 노드 스냅숏 — 화면이 지금 편집본과 비교해 낡음을 보인다.
fn snapshot(n: &Node) -> Value {
    json!({"event": n.event, "config": n.config})
}

/// 출력의 글 값 중 되돌린 작업의 `to`(경로)와 같은 것은 `from`으로, `to`의 파일 이름과 같은 것은 `from`의
/// 파일 이름으로 바꾼다(중첩 객체·목록도). 테스트 뒤 파일은 원래 자리라 뒤 노드가 있는 파일로 테스트되게.
/// 같은 파일을 여러 번 옮겼으면 맨 처음 자리까지 따라간다.
pub fn rewrite_undone_paths(output: &mut Map<String, Value>, restored: &[FileOp]) {
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    let mut pairs = Vec::new();
    for op in restored {
        pairs.push((text(&op.to), text(&op.from)));
        if let (Some(to), Some(from)) = (name(&op.to), name(&op.from)) {
            pairs.push((to, from));
        }
    }
    // 이름 그대로 옮긴 짝(b.pdf→b.pdf)은 뺀다 — 남기면 그 짝에 걸려 다음 작업(이름 바꾸기)까지 못 간다.
    pairs.retain(|(to, from)| to != from);
    output.values_mut().for_each(|v| rewrite_value(v, &pairs));
}

fn rewrite_value(v: &mut Value, pairs: &[(String, String)]) {
    match v {
        Value::String(s) => {
            // 되돌린 작업이 서로 이어지면(a→b→c) 끝까지 — 고리가 있어도 작업 수만큼만 돈다.
            for _ in 0..pairs.len() {
                match pairs.iter().find(|(to, _)| to == s) {
                    Some((_, from)) if from != s => *s = from.clone(),
                    _ => break,
                }
            }
        }
        Value::Array(list) => list.iter_mut().for_each(|v| rewrite_value(v, pairs)),
        Value::Object(m) => m.values_mut().for_each(|v| rewrite_value(v, pairs)),
        _ => {}
    }
}

enum Pushed {
    Yes,
    /// 그 워크플로우가 돌거나 기다리는 중(`push_if_idle`만).
    Busy,
    /// 감독이 멈춰 받지 않았다.
    Stopped,
}

struct Job {
    trigger: Map<String, Value>,
    source: JobSource,
    reply: Option<Reply>,
}

#[derive(Default)]
struct Lane {
    jobs: VecDeque<Job>,
    running: bool,
}

struct Inner {
    deps: Deps,
    catalog: Catalog,
    recent: RecentWrites,
    rt: Handle,
    watches: Mutex<HashMap<String, Watch>>,
    lanes: Mutex<HashMap<String, Lane>>,
    undo_lock: Mutex<()>,
    /// 워크플로우 저장을 차례로 — `updated_at` 비교와 쓰기 사이에 다른 저장이 끼지 않게.
    save_lock: Mutex<()>,
    /// 편집기에 저장 안 된 편집이 있는 워크플로우 id(메모리만 — 로그아웃·앱 종료로 비워진다).
    editing: Mutex<HashSet<String>>,
    /// 빌더 HTTP 테스트 확인 대기 — 계정마다 하나라 다른 계정의 답을 받지 않는다.
    http_gate: HttpConfirmGate,
    /// `stop()` 뒤 취소됨 — 새 감시·실행을 받지 않고 재확인 루프가 끝난다.
    stopped: CancellationToken,
    /// 계산하지 못한 스케줄(워크플로우 id → 화면에 보일 이유).
    schedule_errors: Mutex<HashMap<String, String>>,
    /// 스케줄 확인을 차례로 — 루프·저장이 같은 차례를 두 번 넣지 않게.
    tick_lock: Mutex<()>,
    /// 저장소(`schedule.json`)에 쓰지 못한 차례(워크플로우 id → 차례). 저장소 값보다 앞선다.
    slots: Mutex<HashMap<String, DateTime<Utc>>>,
    /// GitHub 확인 루프를 바로 깨운다(기준점 없는 트리거가 생김). 루프가 `Inner`를 붙잡지 않게 따로 `Arc`.
    github_wake: Arc<Notify>,
    /// 워크플로우별 GitHub 상태 세대 — 초기화마다 +1. 확인 도중 껐다 켠 워크플로우의 결과를 버린다.
    github_gen: Mutex<HashMap<String, u64>>,
    /// (소문자 owner, 소문자 repo, 종류) → (ETag, 마지막 목록). 304면 이 목록을 다시 쓴다.
    github_cache: Mutex<HashMap<github::RepoKey, (String, Vec<Value>)>>,
    /// GitHub 트리거 오류(워크플로우 id → 화면 문구).
    github_errors: Mutex<HashMap<String, String>>,
    /// 한도 때문에 이 시각까지 확인을 쉰다.
    github_pause: Mutex<Option<DateTime<Utc>>>,
    /// `github_watch.json`에 쓰지 못한 상태. 저장소 값보다 앞선다 — 같은 항목이 매분 다시 돌지 않게.
    github_unsaved: Mutex<HashMap<String, WatchEntry>>,
}

#[derive(Clone)]
pub struct Supervisor(Arc<Inner>);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 트리거 출력(`path`·`name`·`extension`·`size`).
fn trigger_output(f: &NewFile) -> Map<String, Value> {
    match serde_json::to_value(f) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

impl Supervisor {
    /// tokio 런타임 안에서 부른다. 시작 시 폴더의 기존 파일은 보지 않는다.
    pub fn start(deps: Deps) -> Result<Self, String> {
        Self::start_with(deps, true)
    }

    /// `close_unfinished` = 끝 시각 없는 기록을 중단됨으로 닫는다. 이 프로세스에서 그 저장소를
    /// 처음 열 때만 참 — 다시 열 때는 앞선 감독의 실행이 아직 돌며 그 기록을 쓰고 있을 수 있다.
    pub fn start_with(deps: Deps, close_unfinished: bool) -> Result<Self, String> {
        let catalog = Catalog::embedded().map_err(|e| format!("노드 정의를 읽지 못했어요: {e}"))?;
        let rt = Handle::try_current().map_err(|e| e.to_string())?;
        let every = deps.recheck;
        let sup = Supervisor(Arc::new(Inner {
            deps,
            catalog,
            recent: RecentWrites::default(),
            rt,
            watches: Mutex::default(),
            lanes: Mutex::default(),
            undo_lock: Mutex::default(),
            save_lock: Mutex::default(),
            editing: Mutex::default(),
            http_gate: HttpConfirmGate::default(),
            stopped: CancellationToken::new(),
            schedule_errors: Mutex::default(),
            tick_lock: Mutex::default(),
            slots: Mutex::default(),
            github_wake: Arc::default(),
            github_gen: Mutex::default(),
            github_cache: Mutex::default(),
            github_errors: Mutex::default(),
            github_pause: Mutex::default(),
            github_unsaved: Mutex::default(),
        }));
        // 감시(`sync`)보다 먼저 — 되돌려진 파일이 새 파일로 잡히지 않는다.
        if close_unfinished {
            sup.close_unfinished();
        }
        sup.sync();
        let (weak, stopped) = (Arc::downgrade(&sup.0), sup.0.stopped.clone());
        sup.0.rt.spawn(async move {
            loop {
                tokio::select! {
                    () = stopped.cancelled() => break,
                    () = tokio::time::sleep(every) => {}
                }
                match weak.upgrade() {
                    Some(inner) => {
                        let sup = Supervisor(inner);
                        sup.recheck();
                        sup.schedule_tick();
                    }
                    None => break,
                }
            }
        });
        sup.spawn_github_loop();
        Ok(sup)
    }

    /// 끝 시각 없는 기록(실행 도중 앱이 꺼짐)을 닫는다. 그중 노드 테스트는 먼저 되돌려 결과를 기록에 남긴다 —
    /// 테스트가 파일을 옮긴 채로 남기지 않는다. 못 되돌린 것이 있으면 기록을 닫은 뒤 `TestUndoFailed`(`at_start`).
    fn close_unfinished(&self) {
        let mut failed = Vec::new();
        match self.store().all_runs() {
            Ok(runs) => {
                let _one_at_a_time = lock(&self.0.undo_lock);
                let tests = runs.into_iter().filter(|r| {
                    r.finished_at.is_empty()
                        && r.trigger == Some(RunTrigger::Test)
                        && r.undo.is_none()
                });
                for mut run in tests {
                    let report = self.undo_ops(&run);
                    if !report.skipped.is_empty() {
                        failed.push((run.workflow_id.clone(), run.run.run_id.clone()));
                    }
                    run.undo = Some(report);
                    if let Err(e) = self.store().save_run(&run) {
                        eprintln!("[ieum] 테스트 되돌리기 결과를 쓰지 못했어요: {e}");
                    }
                }
            }
            Err(e) => eprintln!("[ieum] 실행 기록을 읽지 못했어요: {e}"),
        }
        if let Err(e) = self.store().close_unfinished_runs() {
            eprintln!("[ieum] 끝나지 않은 실행 기록을 닫지 못했어요: {e}");
        }
        for (workflow_id, run_id) in failed {
            let name = self.store().get_workflow(&workflow_id).map(|w| w.name);
            (self.0.deps.emit)(AppEvent::TestUndoFailed {
                at_start: Some(name.unwrap_or_default()),
                workflow_id,
                run_id,
            });
        }
    }

    /// 감시를 모두 풀고 대기 실행을 버린다. 진행 중인 실행은 끝까지 돌아 기록을 남긴다.
    /// 사본(`Clone`)이 남아 drop되지 않을 수 있어 명시적으로 부른다. 되돌릴 수 없다.
    pub fn stop(&self) {
        self.0.stopped.cancel();
        self.0.http_gate.close();
        lock(&self.0.watches).clear();
        let ids: Vec<String> = lock(&self.0.lanes).keys().cloned().collect();
        self.drop_queued(&ids);
    }

    fn is_stopped(&self) -> bool {
        self.0.stopped.is_cancelled()
    }

    pub fn store(&self) -> &Store {
        &self.0.deps.store
    }

    pub fn validate(&self, wf: &Workflow) -> Vec<ValidationError> {
        validate(wf, &self.0.catalog)
    }

    fn emit_status(&self, workflow_id: &str, ok: bool, message: String) {
        (self.0.deps.emit)(AppEvent::WatchStatus {
            workflow_id: workflow_id.to_string(),
            ok,
            message,
        });
    }

    pub fn list_workflows(&self) -> Result<Vec<WorkflowEntry>, String> {
        let list = self.store().list_workflows()?;
        let now = (self.0.deps.now)();
        let github_errors = lock(&self.0.github_errors).clone();
        let watches = lock(&self.0.watches);
        Ok(list
            .into_iter()
            .map(|workflow| {
                let (mut watch, mut watch_message, mut next_run) = match watches.get(&workflow.id) {
                    None => (WatchState::Off, None, None),
                    Some(w) if w.watcher.is_some() => (WatchState::Watching, None, None),
                    Some(w) => (WatchState::WatchError, w.error.clone(), None),
                };
                match enabled_schedule(&workflow).map(|s| s.and_then(|s| next_slot(&s, now))) {
                    None => {}
                    Some(Ok(next)) => (watch, next_run) = (WatchState::Watching, Some(next)),
                    Some(Err(e)) => (watch, watch_message) = (WatchState::WatchError, Some(e)),
                }
                if let TriggerKind::Github { owner, repo, .. } = trigger_kind(&workflow)
                    && workflow.enabled
                {
                    // 저장소를 못 쓰면 확인을 건너뛰므로 "켜짐"으로 보이지 않게 여기서 오류로.
                    let repo_error = github_repo_error(&owner, &repo).map(String::from);
                    (watch, watch_message) =
                        match repo_error.or_else(|| github_errors.get(&workflow.id).cloned()) {
                            Some(e) => (WatchState::WatchError, Some(e)),
                            None => (WatchState::Watching, None),
                        };
                }
                WorkflowEntry {
                    workflow,
                    watch,
                    watch_message,
                    next_run,
                }
            })
            .collect())
    }

    /// 사용자 폴더 정책(`validate_user_folder`) — 홈·앱 데이터 폴더는 주입값.
    fn check_folder(&self, folder: &Path) -> Result<(), String> {
        validate_user_folder(folder, &self.0.deps.home, &self.0.deps.app_data)
    }

    fn check_folders(&self, wf: &Workflow) -> Result<(), String> {
        user_folders(wf)
            .iter()
            .try_for_each(|f| self.check_folder(f))
    }

    /// AI 공급원은 지원 목록(빈 칸은 아직 안 고름)만.
    fn check_providers(&self, wf: &Workflow) -> Result<(), String> {
        for n in wf.nodes.iter().filter(|n| n.event == "local.ai") {
            match n.config.get("provider") {
                None | Some(Value::Null) => {}
                Some(Value::String(p)) if p.is_empty() || self.0.deps.ai.has_provider(p) => {}
                Some(p) => return Err(format!("지원하지 않는 AI 공급원이에요: {p}")),
            }
        }
        Ok(())
    }

    /// `save_lock`을 잡은 채 부른다.
    fn save_checked(&self, wf: StoredWorkflow) -> Result<StoredWorkflow, String> {
        self.check_folders(&wf.workflow)?;
        self.check_providers(&wf.workflow)?;
        let before = (!wf.id.is_empty())
            .then(|| self.store().get_workflow(&wf.id).ok())
            .flatten();
        // 저장과 차례 기록을 스케줄 확인과 같은 잠금 안에서 — 그 사이 배경 확인이 새 저장본을
        // 옛 차례 기록으로 돌리지 않게.
        let saved = {
            let _one = lock(&self.0.tick_lock);
            let saved = self.store().save_workflow(wf)?;
            // 꺼짐→켜짐, 트리거 칸이 바뀜 = 지난 차례를 놓친 것으로 치지 않고 지금부터.
            let restart = before
                .as_ref()
                .is_none_or(|b| !b.enabled || trigger_sig(b) != trigger_sig(&saved));
            if restart && enabled_schedule(&saved).is_some() {
                self.set_slot(&saved.id, (self.0.deps.now)());
            }
            // GitHub는 켜짐→꺼짐도 — 꺼져 있던 동안의 항목이 다시 켤 때 돌지 않게 기준점부터.
            let turned_off = before.as_ref().is_some_and(|b| b.enabled) && !saved.enabled;
            if (restart || turned_off)
                && (before.as_ref().is_some_and(is_github) || is_github(&saved))
            {
                self.github_reset(&saved.id);
                if enabled_github(&saved).is_some() {
                    self.0.github_wake.notify_one();
                }
            }
            saved
        };
        self.sync();
        Ok(saved)
    }

    pub fn save_workflow(&self, wf: StoredWorkflow) -> Result<StoredWorkflow, String> {
        let _g = lock(&self.0.save_lock);
        self.save_checked(wf)
    }

    /// 저장본의 `updated_at`이 `expected_updated_at`과 다르면 `Err(STALE)` — 그사이 다른 쪽이 고쳤다.
    pub fn save_workflow_expecting(
        &self,
        wf: StoredWorkflow,
        expected_updated_at: &str,
    ) -> Result<StoredWorkflow, String> {
        let _g = lock(&self.0.save_lock);
        if self.store().get_workflow(&wf.id)?.updated_at != expected_updated_at {
            return Err(STALE.into());
        }
        self.save_checked(wf)
    }

    pub fn emit(&self, e: AppEvent) {
        (self.0.deps.emit)(e);
    }

    /// 사용자 폴더 정책 — 채팅 빌더의 `pick-folder`가 고른 폴더를 미리 본다.
    pub fn check_user_folder(&self, folder: &Path) -> Result<(), String> {
        self.check_folder(folder)
    }

    /// 편집기의 미저장 편집 표시.
    pub fn set_editing(&self, workflow_id: &str, editing: bool) {
        let mut set = lock(&self.0.editing);
        if editing {
            set.insert(workflow_id.to_string());
        } else {
            set.remove(workflow_id);
        }
    }

    pub fn user_editing(&self, workflow_id: &str) -> bool {
        lock(&self.0.editing).contains(workflow_id)
    }

    pub fn http_gate(&self) -> &HttpConfirmGate {
        &self.0.http_gate
    }

    /// 저장과 같은 잠금 안에서 지운다 — `save_workflow_expecting`의 확인과 쓰기 사이에 끼면 지운 것이 되살아난다.
    pub fn delete_workflow(&self, id: &str) -> Result<(), String> {
        let _g = lock(&self.0.save_lock);
        self.store().delete_workflow(id)?;
        {
            let _one = lock(&self.0.tick_lock);
            lock(&self.0.slots).remove(id);
            if let Err(e) = self.store().remove_schedule_slot(id) {
                eprintln!("[ieum] 스케줄 기록을 지우지 못했어요: {e}");
            }
            self.github_reset(id);
        }
        self.sync();
        Ok(())
    }

    /// 저장된 워크플로우에 맞춰 감시를 다시 걸고 스케줄을 한 번 확인한다.
    pub fn sync(&self) {
        self.sync_watches();
        self.schedule_tick();
    }

    /// 폴더가 그대로이고 살아 있는 감시는 유지하고, 오류 상태면 바로 다시 건다.
    /// 꺼지거나·지워지거나·폴더가 바뀐 워크플로우의 대기 실행은 버린다.
    fn sync_watches(&self) {
        let mut watches = lock(&self.0.watches);
        // stop()은 취소 뒤 이 잠금을 잡고 비우므로, 잠금 안에서 보면 멈춘 뒤 감시를 새로 걸지 않는다.
        if self.is_stopped() {
            return;
        }
        // 목록을 잠금 안에서 읽어 동시 저장이 낡은 목록으로 감시를 지우지 않게.
        let list = match self.store().list_workflows() {
            Ok(l) => l,
            Err(e) => return eprintln!("[ieum] 워크플로우 목록을 읽지 못했어요: {e}"),
        };
        let wanted: HashMap<String, PathBuf> = list
            .iter()
            .filter_map(|w| Some((w.id.clone(), trigger_folder(w)?)))
            .collect();
        let mut stopped = Vec::new();
        watches.retain(|id, w| {
            let same = wanted.get(id) == Some(&w.folder);
            if !same {
                stopped.push(id.clone());
            }
            same && w.watcher.is_some()
        });
        self.drop_queued(&stopped);
        for (id, folder) in wanted {
            if watches.contains_key(&id) {
                continue;
            }
            let mut w = Watch {
                folder,
                ino: 0,
                watcher: None,
                error: None,
            };
            match self.open_watch(&id, &w.folder) {
                Ok((watcher, ino)) => (w.watcher, w.ino) = (Some(watcher), ino),
                Err(e) => {
                    self.emit_status(&id, false, e.clone());
                    w.error = Some(e);
                }
            }
            watches.insert(id, w);
        }
    }

    /// 대기 중인 실행만 버린다(진행 중인 실행은 끝까지).
    fn drop_queued(&self, workflow_ids: &[String]) {
        let mut lanes = lock(&self.0.lanes);
        for id in workflow_ids {
            let Some(lane) = lanes.get_mut(id) else {
                continue;
            };
            for job in lane.jobs.drain(..) {
                if let Some(reply) = job.reply {
                    let _ = reply.send(Err("워크플로우가 꺼지거나 바뀌어 취소됐어요.".into()));
                }
            }
        }
    }

    fn open_watch(&self, id: &str, folder: &Path) -> Result<(FolderWatcher, u64), String> {
        if !folder.is_absolute() {
            return Err(format!(
                "감시 폴더는 절대 경로여야 해요: {:?}",
                folder.display()
            ));
        }
        let cannot =
            |e: std::io::Error| format!("감시 폴더를 열 수 없어요: {}: {e}", folder.display());
        fs::read_dir(folder).map_err(cannot)?;
        let ino = fs::metadata(folder).map_err(cannot)?.ino();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let watcher = FolderWatcher::start_with(
            folder.to_path_buf(),
            self.0.recent.clone(),
            tx,
            self.0.deps.timing,
        )
        .map_err(|e| e.to_string())?;
        // 감시를 drop하면 송신자가 사라져 이 작업도 끝난다.
        let (weak, id, dir) = (
            Arc::downgrade(&self.0),
            id.to_string(),
            folder.to_path_buf(),
        );
        self.0.rt.spawn(async move {
            while let Some(f) = rx.recv().await {
                let Some(inner) = weak.upgrade() else { break };
                Supervisor(inner).enqueue(&id, &dir, trigger_output(&f));
            }
        });
        Ok((watcher, ino))
    }

    /// 감시 폴더가 사라졌거나 열 수 없으면 `watch_error`, 다시 생기면 감시를 새로 건다.
    fn recheck(&self) {
        let mut watches = lock(&self.0.watches);
        for (id, w) in watches.iter_mut() {
            if w.watcher.is_some() {
                let alive = fs::read_dir(&w.folder).is_ok()
                    && fs::metadata(&w.folder).is_ok_and(|m| m.ino() == w.ino);
                if !alive {
                    w.watcher = None;
                    // 지웠다 다시 만든 폴더면 다음 주기를 기다리지 않고 바로 다시 건다.
                    match self.open_watch(id, &w.folder) {
                        Ok((watcher, ino)) => (w.watcher, w.ino) = (Some(watcher), ino),
                        Err(_) => {
                            let msg = format!(
                                "감시 폴더가 사라졌거나 열 수 없어요: {}",
                                w.folder.display()
                            );
                            w.error = Some(msg.clone());
                            self.emit_status(id, false, msg);
                        }
                    }
                }
            } else {
                match self.open_watch(id, &w.folder) {
                    Ok((watcher, ino)) => {
                        (w.watcher, w.ino, w.error) = (Some(watcher), ino, None);
                        self.emit_status(id, true, String::new());
                    }
                    Err(e) => w.error = Some(e),
                }
            }
        }
    }

    /// 차례 기록: 메모리에 먼저 넘기고 저장소에 쓴다. 쓰기가 되면 메모리 값은 지운다(저장소가 기준).
    /// 쓰기가 실패하면 메모리 값이 남아 같은 프로세스 안에선 같은 차례를 다시 돌지 않는다 — 재시작하면
    /// 그 차례가 한 번 더 돌 수 있다(로그만 남긴다). `tick_lock`을 잡은 채 부른다.
    fn set_slot(&self, workflow_id: &str, slot: DateTime<Utc>) {
        let mut pending = lock(&self.0.slots);
        pending.insert(workflow_id.to_string(), slot);
        match self.store().set_schedule_slot(workflow_id, slot) {
            Ok(()) => {
                pending.remove(workflow_id);
            }
            Err(e) => eprintln!("[ieum] 스케줄 기록을 쓰지 못했어요: {e}"),
        }
    }

    #[doc(hidden)]
    pub fn tick_for_test(&self) {
        self.schedule_tick();
    }

    /// 켜진 스케줄 워크플로우마다 지난 차례를 판정한다(설계 "스케줄 루프"). 처음 보는 워크플로우는
    /// 지금부터, 놓친 차례는 가장 최근 하나만 유예(2시간) 안이면 한 번, 실행 중이면 그 차례는 건너뛴다.
    fn schedule_tick(&self) {
        let _one = lock(&self.0.tick_lock);
        if self.is_stopped() {
            return;
        }
        let (list, mut slots) = match (
            self.store().list_workflows(),
            self.store().get_schedule_slots(),
        ) {
            (Ok(l), Ok(s)) => (l, s),
            (Err(e), _) | (_, Err(e)) => {
                return eprintln!("[ieum] 스케줄을 확인하지 못했어요: {e}");
            }
        };
        // 저장소에 쓰지 못한 차례가 이긴다 — 같은 차례를 다시 돌지 않게.
        slots.extend(lock(&self.0.slots).iter().map(|(k, v)| (k.clone(), *v)));
        let now = (self.0.deps.now)();
        let mut errors = HashMap::new();
        for wf in &list {
            let Some(schedule) = enabled_schedule(wf) else {
                continue;
            };
            let schedule = match schedule {
                Ok(s) => s,
                Err(e) => {
                    errors.insert(wf.id.clone(), e);
                    continue;
                }
            };
            let set = |slot| self.set_slot(&wf.id, slot);
            let decision = match slots.get(&wf.id) {
                None => {
                    set(now);
                    Decision::Nothing
                }
                Some(&last) => decide(&schedule, last, now),
            };
            match decision {
                // 지난 차례를 다 처리했는데 다음 차례도 없을 때만 오류(연도 식의 마지막 차례는 돈다).
                Decision::Nothing => {
                    if let Err(e) = next_slot(&schedule, now) {
                        errors.insert(wf.id.clone(), e);
                    }
                }
                // 2시간 안으로 조금 뒤로 간 시계는 기록을 둔다 — 지금으로 덮으면 이미 돈 차례가 다시 돈다.
                Decision::ClockBack => {
                    if slots.get(&wf.id).is_some_and(|&last| last - now > GRACE) {
                        set(now);
                    }
                }
                Decision::Expired { slot } => set(slot),
                Decision::Run { slot } => {
                    // 차례를 먼저 넘긴다 — 기록을 못 쓰면 다음 확인에서 같은 차례를 다시 돌 수 있다.
                    set(slot);
                    let TriggerKind::Schedule { event, .. } = trigger_kind(wf) else {
                        continue;
                    };
                    let config = trigger_sig(wf).map(|(_, c)| c).unwrap_or_default();
                    let job = Job {
                        trigger: schedule_output(&event, &config, now),
                        source: JobSource::Schedule { slot },
                        reply: None,
                    };
                    match self.push_if_idle(&wf.id, job) {
                        Pushed::Yes => {}
                        Pushed::Busy => eprintln!(
                            "[ieum] 실행 중이라 스케줄 차례를 건너뛰었어요: 워크플로우 {} {}",
                            wf.id,
                            triggered_at(slot)
                        ),
                        // 멈춘 감독이 차례를 삼키지 않게 되돌린다 — 다음 감독(재시작·계정 복귀)이 돌린다.
                        Pushed::Stopped => {
                            if let Some(&last) = slots.get(&wf.id) {
                                set(last);
                            }
                        }
                    }
                }
            }
        }
        let mut known = lock(&self.0.schedule_errors);
        for (id, e) in &errors {
            if known.get(id) != Some(e) {
                self.emit_status(id, false, e.clone());
            }
        }
        for id in known.keys() {
            if !errors.contains_key(id) {
                self.emit_status(id, true, String::new());
            }
        }
        *known = errors;
    }

    /// 감시 폴더 `folder`에서 온 트리거를 실행 줄에 넣는다. 같은 워크플로우는 한 번에 하나씩 돈다.
    pub fn enqueue(&self, workflow_id: &str, folder: &Path, trigger: Map<String, Value>) {
        self.push(
            workflow_id,
            Job {
                trigger,
                source: JobSource::Watch(folder.to_path_buf()),
                reply: None,
            },
        );
    }

    /// 그 워크플로우가 돌거나 기다리는 실행이 없을 때만 넣는다(스케줄 차례는 줄 크기 1).
    /// 확인과 넣기를 같은 잠금 안에서 한다.
    fn push_if_idle(&self, workflow_id: &str, job: Job) -> Pushed {
        let mut lanes = lock(&self.0.lanes);
        if lanes
            .get(workflow_id)
            .is_some_and(|l| l.running || !l.jobs.is_empty())
        {
            return Pushed::Busy;
        }
        self.push_locked(&mut lanes, workflow_id, job)
    }

    fn push(&self, workflow_id: &str, job: Job) -> Pushed {
        let mut lanes = lock(&self.0.lanes);
        self.push_locked(&mut lanes, workflow_id, job)
    }

    fn push_locked(
        &self,
        lanes: &mut HashMap<String, Lane>,
        workflow_id: &str,
        job: Job,
    ) -> Pushed {
        if self.is_stopped() {
            if let Some(reply) = job.reply {
                let _ = reply.send(Err("계정이 바뀌거나 로그아웃되어 취소됐어요.".into()));
            }
            return Pushed::Stopped;
        }
        let lane = lanes.entry(workflow_id.to_string()).or_default();
        lane.jobs.push_back(job);
        while lane.jobs.len() > QUEUE_CAP {
            let Some(old) = lane.jobs.pop_front() else {
                break;
            };
            let text = |k: &str| {
                old.trigger
                    .get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            };
            let label = match &old.source {
                JobSource::Github { .. } => {
                    let t = &old.trigger;
                    let n = t.get("issueNumber").or_else(|| t.get("prNumber"));
                    format!(
                        "{} #{}",
                        text("repo"),
                        n.and_then(Value::as_u64).unwrap_or(0)
                    )
                }
                _ => text("path").to_string(),
            };
            (self.0.deps.emit)(AppEvent::QueueDropped {
                workflow_id: workflow_id.to_string(),
                label,
            });
            if let Some(reply) = old.reply {
                let _ = reply.send(Err("실행 줄이 넘쳐 취소됐어요.".into()));
            }
        }
        if !lane.running {
            lane.running = true;
            let (sup, id) = (self.clone(), workflow_id.to_string());
            self.0.rt.spawn(async move { sup.drain(&id).await });
        }
        Pushed::Yes
    }

    async fn drain(&self, workflow_id: &str) {
        loop {
            let job = {
                let mut lanes = lock(&self.0.lanes);
                let lane = lanes.entry(workflow_id.to_string()).or_default();
                match lane.jobs.pop_front() {
                    Some(j) => j,
                    None => {
                        lane.running = false;
                        return;
                    }
                }
            };
            if let JobSource::NodeTest {
                wf,
                node_id,
                file,
                reply,
            } = job.source
            {
                let result = self
                    .node_test(workflow_id, wf, &node_id, file.as_deref())
                    .await;
                if let Err(e) = &result {
                    eprintln!("[ieum] 노드를 테스트하지 못했어요: 워크플로우 {workflow_id}: {e}");
                }
                let _ = reply.send(result);
                continue;
            }
            let result = self.run_one(workflow_id, job.source, job.trigger).await;
            if let Err(e) = &result {
                eprintln!("[ieum] 실행하지 못했어요: 워크플로우 {workflow_id}: {e}");
            }
            if let Some(reply) = job.reply {
                let _ = reply.send(result);
            }
        }
    }

    /// 실행 하나의 러너 — 파일 노드는 `roots` 안에서만 움직이고 작업을 `journal`에 남긴다.
    fn runners(&self, roots: &AllowedRoots, journal: &Journal) -> Runners {
        let recent = self.0.recent.clone();
        let mut runners = Runners::with_builtin();
        let local: [(&str, Arc<dyn NodeRunner>); 5] = [
            (
                "local.ai",
                Arc::new(self.0.deps.ai.with_roots(roots.clone())),
            ),
            (
                "local.file_move",
                Arc::new(FileMoveRunner::new(
                    journal.clone(),
                    recent.clone(),
                    roots.clone(),
                )),
            ),
            (
                "local.file_rename",
                Arc::new(FileRenameRunner::new(
                    journal.clone(),
                    recent.clone(),
                    roots.clone(),
                )),
            ),
            (
                "local.file_trash",
                Arc::new(FileTrashRunner::with_trash_dir(
                    journal.clone(),
                    recent,
                    roots.clone(),
                    self.0.deps.trash_dir.clone(),
                )),
            ),
            ("http.request", Arc::new(HttpRunner::new())),
        ];
        runners.extend(local.into_iter().map(|(k, v)| (k.to_string(), v)));
        let github: Arc<dyn NodeRunner> = self.0.deps.github.clone();
        runners.extend(GITHUB_EVENTS.map(|e| (e.to_string(), github.clone())));
        runners
    }

    /// 실행 하나 → 기록 저장. run_id를 돌려준다. 파일 작업이 생길 때마다 기록을 먼저 쓴다.
    async fn run_one(
        &self,
        workflow_id: &str,
        source: JobSource,
        trigger: Map<String, Value>,
    ) -> Result<String, String> {
        let mut wf = self.store().get_workflow(workflow_id)?;
        let (run_trigger, scheduled_for) = match &source {
            JobSource::Test(_) | JobSource::NodeTest { .. } => (RunTrigger::Test, None),
            JobSource::Watch(_) => (RunTrigger::Watch, None),
            JobSource::Manual => (RunTrigger::Manual, None),
            JobSource::Schedule { slot } => (RunTrigger::Schedule, Some(triggered_at(*slot))),
            JobSource::Github { .. } => (RunTrigger::Github, None),
        };
        match source {
            // 노드 테스트는 저장본 대신 대상까지 자른 그래프를 돌린다.
            JobSource::Test(test) => wf.workflow = test,
            // 노드 패널 테스트는 `drain`이 `node_test`로 돌린다.
            JobSource::NodeTest { .. } => return Err("노드 테스트는 따로 돌아요.".into()),
            // 감시가 보낸 뒤 꺼졌거나 폴더가 바뀐 워크플로우의 늦은 트리거는 버린다.
            JobSource::Watch(f) if trigger_folder(&wf).as_ref() != Some(&f) => {
                return Err("꺼졌거나 감시 폴더가 바뀌어 늦게 온 트리거를 버렸어요.".into());
            }
            JobSource::Schedule { .. } if enabled_schedule(&wf).is_none() => {
                return Err("꺼졌거나 스케줄이 아니게 되어 차례를 버렸어요.".into());
            }
            JobSource::Github { config_hash }
                if github_hash(&wf).as_ref() != Some(&config_hash) =>
            {
                return Err("꺼졌거나 트리거가 바뀌어 늦게 온 GitHub 항목을 버렸어요.".into());
            }
            _ => {}
        }
        // 저장 때 검사했지만 저장소에 직접 심은 정의도 막는다.
        self.check_folders(&wf.workflow)?;
        let roots = roots_for_workflow(&wf.workflow);
        let journal = Journal::default();
        let runners = self.runners(&roots, &journal);

        let started_at = now();
        let stored = |run: RunResult, finished_at: String| StoredRun {
            run,
            workflow_id: workflow_id.to_string(),
            started_at: started_at.clone(),
            finished_at,
            file_ops: journal.ops(),
            allowed_roots: roots.paths().to_vec(),
            undo: None,
            trigger: Some(run_trigger),
            scheduled_for: scheduled_for.clone(),
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let emit = self.0.deps.emit.clone();
        let clock = std::time::Instant::now();
        let notice = |run_id: &str, kind| {
            emit(AppEvent::RunNotice(RunNotice {
                workflow_id: workflow_id.to_string(),
                workflow_name: wf.name.clone(),
                run_id: run_id.to_string(),
                trigger: run_trigger,
                kind,
            }))
        };
        let forward = async {
            let (mut run_id, mut saved_ops, mut finished) = (String::new(), 0, None);
            while let Some(event) = rx.recv().await {
                if let ExecEvent::RunStarted { run_id: r } = &event {
                    run_id = r.clone();
                    notice(r, NoticeKind::Started);
                }
                // 파일 작업이 생기면 끝나지 않은 기록(끝 시각 없음)으로 바로 쓴다 — 도중에 앱이 꺼져도 되돌릴 수 있게.
                if journal.ops().len() > saved_ops {
                    let partial = stored(
                        RunResult {
                            run_id: run_id.clone(),
                            status: RunStatus::Cancelled,
                            validation_errors: Vec::new(),
                            reject_reason: None,
                            nodes: Default::default(),
                        },
                        String::new(),
                    );
                    saved_ops = partial.file_ops.len();
                    if let Err(e) = self.store().save_run(&partial) {
                        eprintln!("[ieum] 실행 도중 기록을 쓰지 못했어요: {e}");
                    }
                }
                // 끝 이벤트는 최종 저장 뒤에 내보낸다 — 화면이 받자마자 기록을 다시 읽는다.
                if matches!(event, ExecEvent::RunFinished { .. }) {
                    finished = Some(event);
                    continue;
                }
                emit(AppEvent::Exec {
                    workflow_id: workflow_id.to_string(),
                    event,
                });
            }
            finished
        };
        let (run, finished) = tokio::join!(
            run_workflow(
                &wf.workflow,
                &self.0.catalog,
                &runners,
                trigger,
                tx,
                CancellationToken::new()
            ),
            forward
        );
        let mut stored = stored(run, now());
        // 노드 테스트는 여기서(실행 줄 안에서) 바로 되돌린다 — 요청한 쪽이 사라졌거나 기록 저장이
        // 실패해도 파일이 옮겨진 채 남지 않고, 줄 선 다음 실행은 되돌려진 상태에서 시작한다.
        if run_trigger == RunTrigger::Test {
            let _one_at_a_time = lock(&self.0.undo_lock);
            stored.undo = Some(self.undo_ops(&stored));
        } else {
            // 테스트 기록은 줄이지 않는다 — `run_test`가 이 기록에서 출력 타입(객체·글)을 읽어 빌더에 준다.
            clip_http_bodies(&mut stored.run, &wf.workflow);
        }
        let saved = self.store().save_run(&stored);
        // 못 되돌린 경로는 화면이 기록에서 읽는다 — 기록을 쓴 뒤에 알린다.
        if stored.undo.as_ref().is_some_and(|u| !u.skipped.is_empty()) {
            emit(AppEvent::TestUndoFailed {
                workflow_id: workflow_id.to_string(),
                run_id: stored.run.run_id.clone(),
                at_start: None,
            });
        }
        if let Some(event) = finished {
            emit(AppEvent::Exec {
                workflow_id: workflow_id.to_string(),
                event,
            });
            let run = &stored.run;
            let failed_node = wf
                .workflow
                .nodes
                .iter()
                .find(|n| {
                    run.nodes
                        .get(&n.id)
                        .is_some_and(|r| r.status == NodeStatus::Failed)
                })
                .map(|n| n.label.clone().unwrap_or_else(|| n.id.clone()));
            notice(
                &run.run_id,
                NoticeKind::Finished {
                    ok: run.status == RunStatus::Succeeded,
                    secs: clock.elapsed().as_secs(),
                    failed_node,
                },
            );
        }
        // 한도 알림은 기록을 쓴 뒤에 본다 — 이번 실행 지출이 이번 달 합계에 들어가야 한다.
        // 이번 달 기록을 다 읽으므로 실행 끝 이벤트 뒤에 둔다 — 화면의 끝 표시가 늦어지지 않게.
        if saved.is_ok() {
            let used = used_keys(crate::usage::run_usages(&stored));
            for n in after_use(self.store(), &used, Local::now()) {
                emit(AppEvent::KeyLimit(n));
            }
        }
        saved?;
        Ok(stored.run.run_id)
    }

    /// 지금 실행(같은 줄을 탄다, 기록 trigger = Manual). 끝나면 run_id.
    /// 폴더 트리거는 `file`이 필수 — 그 워크플로우의 허용 폴더 안 일반 파일(심볼릭 링크 아님).
    /// 스케줄은 지금 시각 출력, 수동은 빈 출력으로 `file` 없이 돈다 — 이때 `file`이 넘어와도 무시한다.
    pub async fn run_now(&self, workflow_id: &str, file: Option<&Path>) -> Result<String, String> {
        let wf = self.store().get_workflow(workflow_id)?;
        let trigger = match trigger_kind(&wf) {
            TriggerKind::Folder(_) => {
                let file = file.ok_or("파일을 골라 주세요")?;
                self.file_trigger(&wf.workflow, file)?
            }
            TriggerKind::Schedule { event, .. } => {
                let config = trigger_sig(&wf).map(|(_, c)| c).unwrap_or_default();
                schedule_output(&event, &config, (self.0.deps.now)())
            }
            TriggerKind::Manual => Map::new(),
            TriggerKind::Github { .. } => {
                let node = first_trigger(&wf.workflow).ok_or("트리거가 없어요.")?;
                self.github_sample(node).await?
            }
            TriggerKind::Unsupported => {
                return Err("이 트리거는 지금 실행할 수 없어요.".into());
            }
        };
        self.queue(workflow_id, trigger, None).await
    }

    /// 실행 줄에 넣고 끝나길 기다린다. 끝나면 run_id.
    async fn queue(
        &self,
        workflow_id: &str,
        trigger: Map<String, Value>,
        test: Option<Workflow>,
    ) -> Result<String, String> {
        let (tx, rx) = oneshot::channel();
        self.push(
            workflow_id,
            Job {
                trigger,
                source: test.map_or(JobSource::Manual, JobSource::Test),
                reply: Some(tx),
            },
        );
        rx.await.map_err(|_| "실행이 취소됐어요.".to_string())?
    }

    /// `wf`를 `trigger`(샘플 파일 또는 트리거 출력)로 실행 줄에서 한 번 돌리고(기록 trigger = Test) 바로 되돌린다.
    /// 되돌리기는 파일 작업이 없어도 항상 한다. 못 되돌린 것이 있으면 화면에 알린다(`TestUndoFailed`).
    pub async fn run_test(
        &self,
        workflow_id: &str,
        wf: Workflow,
        trigger: TestTrigger,
    ) -> Result<TestRun, String> {
        let trigger = match trigger {
            TestTrigger::File(sample) => self.file_trigger(&wf, &sample)?,
            TestTrigger::Output(output) => output,
            TestTrigger::Github => {
                let node = first_trigger(&wf).ok_or("트리거가 없어요.")?;
                self.github_sample(node).await?
            }
        };
        // 되돌리기는 실행 줄(`run_one`)이 한다 — 이 future가 버려져도 끝까지 간다.
        let run_id = self.queue(workflow_id, trigger, Some(wf.clone())).await?;
        let stored = self.store().get_run(&run_id)?;
        self.save_test_samples(workflow_id, &wf, &stored);
        Ok(TestRun {
            run_id,
            status: stored.run.status,
            nodes: stored.run.nodes,
            undo: stored.undo.unwrap_or_default(),
        })
    }

    /// 빌더 테스트에서 실제로 실행된 노드(성공·실패)마다 샘플을 남긴다(경로는 되돌린 뒤 경로로).
    /// 쓰지 못해도 테스트 결과는 그대로 돌려준다.
    fn save_test_samples(&self, workflow_id: &str, wf: &Workflow, stored: &StoredRun) {
        let restored = stored.undo.as_ref().map_or(&[][..], |u| &u.restored[..]);
        let tested_at = now();
        let samples = wf
            .nodes
            .iter()
            .filter_map(|n| {
                let rec = stored.run.nodes.get(&n.id)?;
                let status = match rec.status {
                    NodeStatus::Succeeded => SampleStatus::Succeeded,
                    NodeStatus::Failed => SampleStatus::Failed,
                    _ => return None,
                };
                let mut output = rec.output.clone();
                if let Some(o) = output.as_mut() {
                    rewrite_undone_paths(o, restored);
                }
                let sample = Sample {
                    status,
                    output,
                    error: rec.error.clone(),
                    error_code: rec.error_code.clone(),
                    tested_at: tested_at.clone(),
                    node: snapshot(n),
                };
                Some((n.id.clone(), sample))
            })
            .collect();
        if let Err(e) = self.store().put_samples(workflow_id, samples) {
            eprintln!("[ieum] 테스트 결과를 샘플로 쓰지 못했어요: {e}");
        }
    }

    /// 노드 패널 테스트: 편집본 `wf`의 노드 `node_id` 하나만 같은 워크플로우 실행 줄에서 돌린다 — 전체 실행·다른
    /// 테스트와 겹치지 않는다. 앞 노드 값은 앞 노드의 성공 샘플. 실행 이벤트(`exec-event`)는 내지 않는다.
    /// 검증 실패(`Invalid`)·앞 노드 샘플 없음(`Missing`)은 실행하지 않고 샘플도 덮지 않는다.
    /// HTTP·GitHub 쓰기 확인은 화면이 이 명령을 부르기 전에 받는다(`useTestRun`·`HttpConfirm`).
    pub async fn test_node(
        &self,
        workflow_id: &str,
        wf: Workflow,
        node_id: &str,
        file: Option<&Path>,
    ) -> Result<NodeTestResult, String> {
        // 샘플을 둘 곳 — 한 번도 저장하지 않은 워크플로우는 테스트하지 않는다.
        if self.store().get_workflow(workflow_id).is_err() {
            return Err("먼저 저장해 주세요".into());
        }
        if !wf.nodes.iter().any(|n| n.id == node_id) {
            return Err(format!("노드를 찾을 수 없어요: {node_id}"));
        }
        if let Some(e) = node_errors(&wf, &self.0.catalog, node_id)
            .into_iter()
            .next()
        {
            return Ok(NodeTestResult {
                error: Some(e.message),
                ..NodeTestResult::new(NodeTestStatus::Invalid)
            });
        }
        let (tx, rx) = oneshot::channel();
        self.push(
            workflow_id,
            Job {
                trigger: Map::new(),
                source: JobSource::NodeTest {
                    wf,
                    node_id: node_id.to_string(),
                    file: file.map(Path::to_path_buf),
                    reply: tx,
                },
                reply: None,
            },
        );
        rx.await.map_err(|_| "실행이 취소됐어요.".to_string())?
    }

    /// 실행 줄 안에서 도는 `test_node` 본문.
    async fn node_test(
        &self,
        workflow_id: &str,
        wf: Workflow,
        node_id: &str,
        file: Option<&Path>,
    ) -> Result<NodeTestResult, String> {
        // 줄을 서는 동안 지워졌으면 샘플·기록을 남기지 않는다.
        self.store().get_workflow(workflow_id)?;
        let node = wf
            .nodes
            .iter()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("노드를 찾을 수 없어요: {node_id}"))?;
        let save_sample = |sample: Sample| {
            self.store()
                .put_samples(workflow_id, BTreeMap::from([(node_id.to_string(), sample)]))
        };
        // 트리거는 지금 실행과 같은 출력을 만들어 샘플로만 남긴다(실행 기록 없음).
        if node
            .r#type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("TRIGGER"))
        {
            let output = match node.event.as_str() {
                "manual.run" => Map::new(),
                "local.folder_new_file" => {
                    self.file_trigger(&wf, file.ok_or("파일을 골라 주세요")?)?
                }
                e if e.starts_with("schedule.") => {
                    schedule_output(e, &node.config, (self.0.deps.now)())
                }
                e if GITHUB_TRIGGERS.contains(&e) => self.github_sample(node).await?,
                _ => return Err("이 트리거는 테스트할 수 없어요.".into()),
            };
            let tested_at = now();
            save_sample(Sample {
                status: SampleStatus::Succeeded,
                output: Some(output.clone()),
                error: None,
                error_code: None,
                tested_at: tested_at.clone(),
                node: snapshot(node),
            })?;
            return Ok(NodeTestResult {
                output: Some(output),
                tested_at: Some(tested_at),
                ..NodeTestResult::new(NodeTestStatus::Succeeded)
            });
        }

        // 앞 노드 값 = 참조하는 앞 노드의 성공 샘플. 실패 샘플은 없는 것으로 본다.
        let samples = self.store().get_samples(workflow_id)?;
        let (mut outputs, mut missing) = (Outputs::new(), Vec::new());
        for id in referenced_node_ids(&Value::Object(node.config.clone())) {
            // 트리거를 같은 id로 바꾸면(팔레트) 옛 트리거 샘플이 남는다 — event가 다르면 없는 것으로. GAPS.md 노드별 테스트
            let event = wf
                .nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.event.as_str());
            match samples.get(&id) {
                // event가 없는 쪽끼리(None == None) 같다고 보지 않는다.
                Some(s)
                    if s.status == SampleStatus::Succeeded
                        && event.is_some()
                        && s.node["event"].as_str() == event =>
                {
                    outputs.insert(id, s.output.clone().unwrap_or_default());
                }
                _ => missing.push(id),
            }
        }
        if !missing.is_empty() {
            return Ok(NodeTestResult {
                missing,
                ..NodeTestResult::new(NodeTestStatus::Missing)
            });
        }

        self.check_folders(&wf)?;
        let roots = roots_for_workflow(&wf);
        let journal = Journal::default();
        let runners = self.runners(&roots, &journal);
        let started_at = now();
        // 이벤트는 버린다 — 캔버스·실행 표시는 노드 테스트를 모른다.
        let (result, usage) = run_node(
            node,
            &runners,
            &outputs,
            &mpsc::unbounded_channel().0,
            &CancellationToken::new(),
        )
        .await;
        let (status, output, error, error_code) = match result {
            Ok(o) => (NodeStatus::Succeeded, Some(o), None, None),
            Err(e) => (NodeStatus::Failed, None, Some(e.message), e.code),
        };
        let ok = status == NodeStatus::Succeeded;
        let record = NodeRecord {
            status,
            output: output.clone(),
            error: error.clone(),
            usage,
            error_code: error_code.clone(),
            output_clipped: false,
        };
        let mut stored = StoredRun {
            run: RunResult {
                run_id: uuid::Uuid::new_v4().to_string(),
                status: if ok {
                    RunStatus::Succeeded
                } else {
                    RunStatus::Failed
                },
                validation_errors: Vec::new(),
                reject_reason: None,
                nodes: BTreeMap::from([(node_id.to_string(), record)]),
            },
            workflow_id: workflow_id.to_string(),
            started_at,
            finished_at: String::new(),
            file_ops: journal.ops(),
            allowed_roots: roots.paths().to_vec(),
            undo: None,
            trigger: Some(RunTrigger::Test),
            scheduled_for: None,
        };
        // 파일 작업이 있으면 되돌리기 전에 끝나지 않은 기록으로 먼저 쓴다 — 도중에 앱이 꺼져도 다음 시작 때 되돌린다.
        if !stored.file_ops.is_empty()
            && let Err(e) = self.store().save_run(&stored)
        {
            eprintln!("[ieum] 실행 도중 기록을 쓰지 못했어요: {e}");
        }
        let report = {
            let _one_at_a_time = lock(&self.0.undo_lock);
            self.undo_ops(&stored)
        };
        let (undone, undo_failed) = (!report.restored.is_empty(), report.skipped.len());
        let mut output = output;
        if let Some(o) = output.as_mut() {
            rewrite_undone_paths(o, &report.restored);
        }
        stored.undo = Some(report);
        stored.finished_at = now();
        clip_http_bodies(&mut stored.run, &wf);
        let saved = self.store().save_run(&stored);
        let run_id = stored.run.run_id.clone();
        if undo_failed > 0 {
            self.emit(AppEvent::TestUndoFailed {
                workflow_id: workflow_id.to_string(),
                run_id: run_id.clone(),
                at_start: None,
            });
        }
        // 키 한도·지출은 실행 기록의 `usage`에서 센다 — 기록을 쓴 뒤에 본다.
        if saved.is_ok() {
            let used = used_keys(crate::usage::run_usages(&stored));
            for n in after_use(self.store(), &used, Local::now()) {
                self.emit(AppEvent::KeyLimit(n));
            }
        }
        saved?;
        let tested_at = stored.finished_at;
        save_sample(Sample {
            status: if ok {
                SampleStatus::Succeeded
            } else {
                SampleStatus::Failed
            },
            output: output.clone(),
            error: error.clone(),
            error_code: error_code.clone(),
            tested_at: tested_at.clone(),
            node: snapshot(node),
        })?;
        Ok(NodeTestResult {
            output,
            error,
            error_code,
            tested_at: Some(tested_at),
            undone,
            undo_failed,
            run_id: Some(run_id),
            ..NodeTestResult::new(if ok {
                NodeTestStatus::Succeeded
            } else {
                NodeTestStatus::Failed
            })
        })
    }

    pub fn ai(&self) -> &AiRunner {
        &self.0.deps.ai
    }

    /// `file`로 만든 트리거 출력. 파일은 `wf`의 허용 폴더 안 일반 파일(심볼릭 링크 아님)이어야 한다.
    fn file_trigger(&self, wf: &Workflow, file: &Path) -> Result<Map<String, Value>, String> {
        self.check_folders(wf)?;
        let meta = fs::symlink_metadata(file).map_err(|e| format!("{}: {e}", file.display()))?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "일반 파일이 아니에요(폴더·심볼릭 링크): {}",
                file.display()
            ));
        }
        let real = file
            .canonicalize()
            .map_err(|e| format!("{}: {e}", file.display()))?;
        if !roots_for_workflow(wf).contains(&real) {
            return Err(format!(
                "이 워크플로우가 다루는 폴더 밖 파일이에요: {}",
                real.display()
            ));
        }
        let lossy = |s: Option<&std::ffi::OsStr>| {
            s.map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        Ok(trigger_output(&NewFile {
            name: lossy(real.file_name()),
            extension: lossy(real.extension()),
            path: real.clone(),
            size: meta.len(),
        }))
    }

    /// 기록의 `allowed_roots`·휴지통·앱 전역 `RecentWrites`로 되돌린다. 이미 되돌렸으면 에러.
    pub fn undo_run(&self, run_id: &str) -> Result<UndoReport, String> {
        let _one_at_a_time = lock(&self.0.undo_lock);
        let mut run = self.store().get_run(run_id)?;
        if run.undo.is_some() {
            return Err("이미 되돌린 실행이에요.".into());
        }
        // 도는 중인 실행은 끝날 때 기록을 다시 쓴다(앱이 꺼졌던 기록은 시작 때 닫힌다).
        if run.finished_at.is_empty() {
            return Err("아직 끝나지 않은 실행이에요.".into());
        }
        let report = self.undo_ops(&run);
        run.undo = Some(report.clone());
        self.store().save_run(&run)?;
        Ok(report)
    }

    /// 기록의 파일 작업을 되돌린다(기록은 쓰지 않는다). `undo_lock`을 잡은 채 부른다.
    fn undo_ops(&self, run: &StoredRun) -> UndoReport {
        // 기록이 심어졌어도 정책 밖 폴더에서는 움직이지 않는다.
        let roots = AllowedRoots::new(
            run.allowed_roots
                .iter()
                .filter(|r| self.check_folder(r).is_ok())
                .cloned(),
        );
        undo(
            &run.file_ops,
            &roots,
            &self.0.deps.trash_dir,
            &self.0.recent,
        )
    }
}
