//! 실행 감독: 켜진 폴더 트리거 워크플로우마다 감시 하나, 워크플로우별 실행 줄(한 번에 하나), 실행 기록·되돌리기.
//! Tauri 없이 돈다 — 저장소·휴지통·AI 러너·이벤트 송신을 `Deps`로 주입한다.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use ieum_engine::schedule::{
    Decision, GRACE, Schedule, decide, schedule_cron, schedule_output, to_quartz, triggered_at,
};
use ieum_engine::{
    BuiltinRunners, Catalog, ExecEvent, NodeRunner, NodeStatus, RunResult, RunStatus, Runners,
    ValidationError, Workflow, run_workflow, validate,
};
use ieum_local::ai::AiRunner;
use ieum_local::builder::{HttpLine, TestRun, TestTrigger};
use ieum_local::{
    AllowedRoots, FileMoveRunner, FileRenameRunner, FileTrashRunner, FolderWatcher, HttpRunner,
    Journal, NewFile, RecentWrites, UndoReport, WatchTiming, roots_for_workflow, undo,
    user_folders, validate_user_folder,
};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::http_confirm::HttpConfirmGate;
use crate::store::{RunTrigger, Store, StoredRun, StoredWorkflow, clip_http_bodies, now};

/// 워크플로우 하나의 대기 실행 상한. 넘으면 가장 오래된 것부터 버린다.
pub const QUEUE_CAP: usize = 100;

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
    /// 화면 이벤트 `queue-dropped` — 줄이 넘쳐 버린 실행(트리거 파일 경로).
    QueueDropped { workflow_id: String, path: String },
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
    /// 꺼졌거나 폴더·스케줄 트리거가 아님
    Off,
    /// 폴더 감시 중이거나 스케줄이 켜져 있음
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
    /// 지금 실행(`run_now`).
    Manual,
    /// 채팅 빌더의 노드 테스트 — 저장본 대신 돌릴 그래프(대상까지 자른 것).
    Test(Workflow),
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
            let restart =
                before.is_none_or(|b| !b.enabled || trigger_sig(&b) != trigger_sig(&saved));
            if restart
                && enabled_schedule(&saved).is_some()
                && let Err(e) = self
                    .store()
                    .set_schedule_slot(&saved.id, (self.0.deps.now)())
            {
                eprintln!("[ieum] 스케줄 기록을 쓰지 못했어요: {e}");
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
        let (list, slots) = match (
            self.store().list_workflows(),
            self.store().get_schedule_slots(),
        ) {
            (Ok(l), Ok(s)) => (l, s),
            (Err(e), _) | (_, Err(e)) => {
                return eprintln!("[ieum] 스케줄을 확인하지 못했어요: {e}");
            }
        };
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
            let set = |slot| {
                if let Err(e) = self.store().set_schedule_slot(&wf.id, slot) {
                    eprintln!("[ieum] 스케줄 기록을 쓰지 못했어요: {e}");
                }
            };
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

    fn push(&self, workflow_id: &str, job: Job) {
        let mut lanes = lock(&self.0.lanes);
        self.push_locked(&mut lanes, workflow_id, job);
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
            let path = old.trigger.get("path").and_then(Value::as_str);
            (self.0.deps.emit)(AppEvent::QueueDropped {
                workflow_id: workflow_id.to_string(),
                path: path.unwrap_or_default().to_string(),
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
            let result = self.run_one(workflow_id, job.source, job.trigger).await;
            if let Err(e) = &result {
                eprintln!("[ieum] 실행하지 못했어요: 워크플로우 {workflow_id}: {e}");
            }
            if let Some(reply) = job.reply {
                let _ = reply.send(result);
            }
        }
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
            JobSource::Test(_) => (RunTrigger::Test, None),
            JobSource::Watch(_) => (RunTrigger::Watch, None),
            JobSource::Manual => (RunTrigger::Manual, None),
            JobSource::Schedule { slot } => (RunTrigger::Schedule, Some(triggered_at(*slot))),
        };
        match source {
            // 노드 테스트는 저장본 대신 대상까지 자른 그래프를 돌린다.
            JobSource::Test(test) => wf.workflow = test,
            // 감시가 보낸 뒤 꺼졌거나 폴더가 바뀐 워크플로우의 늦은 트리거는 버린다.
            JobSource::Watch(f) if trigger_folder(&wf).as_ref() != Some(&f) => {
                return Err("꺼졌거나 감시 폴더가 바뀌어 늦게 온 트리거를 버렸어요.".into());
            }
            JobSource::Schedule { .. } if enabled_schedule(&wf).is_none() => {
                return Err("꺼졌거나 스케줄이 아니게 되어 차례를 버렸어요.".into());
            }
            _ => {}
        }
        // 저장 때 검사했지만 저장소에 직접 심은 정의도 막는다.
        self.check_folders(&wf.workflow)?;
        let roots = roots_for_workflow(&wf.workflow);
        let (journal, recent) = (Journal::default(), self.0.recent.clone());
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
        saved?;
        Ok(stored.run.run_id)
    }

    /// 지금 실행(같은 줄을 탄다, 기록 trigger = Manual). 끝나면 run_id.
    /// 폴더 트리거는 `file`이 필수 — 그 워크플로우의 허용 폴더 안 일반 파일(심볼릭 링크 아님).
    /// 스케줄은 지금 시각 출력, 수동은 빈 출력으로 `file` 없이 돈다.
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
            TriggerKind::Unsupported => return Err("이 트리거는 지금 실행할 수 없어요.".into()),
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
        };
        // 되돌리기는 실행 줄(`run_one`)이 한다 — 이 future가 버려져도 끝까지 간다.
        let run_id = self.queue(workflow_id, trigger, Some(wf)).await?;
        let stored = self.store().get_run(&run_id)?;
        Ok(TestRun {
            run_id,
            status: stored.run.status,
            nodes: stored.run.nodes,
            undo: stored.undo.unwrap_or_default(),
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
