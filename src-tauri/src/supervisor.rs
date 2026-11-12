//! 실행 감독: 켜진 폴더 트리거 워크플로우마다 감시 하나, 워크플로우별 실행 줄(한 번에 하나), 실행 기록·되돌리기.
//! Tauri 없이 돈다 — 저장소·휴지통·AI 러너·이벤트 송신을 `Deps`로 주입한다.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ieum_engine::{
    BuiltinRunners, Catalog, ExecEvent, NodeRunner, RunResult, RunStatus, Runners, ValidationError,
    Workflow, run_workflow, validate,
};
use ieum_local::ai::AiRunner;
use ieum_local::{
    AllowedRoots, FileMoveRunner, FileRenameRunner, FileTrashRunner, FolderWatcher, Journal,
    NewFile, RecentWrites, UndoReport, WatchTiming, roots_for_workflow, undo, user_folders,
    validate_user_folder,
};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::store::{Store, StoredRun, StoredWorkflow, now};

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
    /// 감시 폴더 재확인 간격(운영 30초).
    pub recheck: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    /// 꺼졌거나 폴더 트리거가 아님
    Off,
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
}

struct Watch {
    folder: PathBuf,
    /// 시작 때 폴더 ino — 지워졌다 다시 생긴 폴더를 알아본다.
    ino: u64,
    watcher: Option<FolderWatcher>,
    error: Option<String>,
}

type Reply = oneshot::Sender<Result<String, String>>;

struct Job {
    trigger: Map<String, Value>,
    /// 감시가 보낸 일이면 그 감시 폴더. 시험 실행(`run_now`)은 None.
    folder: Option<PathBuf>,
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
    /// `stop()` 뒤 취소됨 — 새 감시·실행을 받지 않고 재확인 루프가 끝난다.
    stopped: CancellationToken,
}

#[derive(Clone)]
pub struct Supervisor(Arc<Inner>);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 켜져 있고 폴더 트리거가 있으면 그 폴더.
fn trigger_folder(wf: &StoredWorkflow) -> Option<PathBuf> {
    if !wf.enabled {
        return None;
    }
    let node = wf
        .workflow
        .nodes
        .iter()
        .find(|n| n.event == "local.folder_new_file")?;
    let folder = node.config.get("folder").and_then(Value::as_str);
    Some(PathBuf::from(folder.unwrap_or("")))
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
        let catalog = Catalog::embedded().map_err(|e| format!("노드 정의를 읽지 못했어요: {e}"))?;
        let rt = Handle::try_current().map_err(|e| e.to_string())?;
        let every = deps.recheck;
        if let Err(e) = deps.store.close_unfinished_runs() {
            eprintln!("[ieum] 끝나지 않은 실행 기록을 닫지 못했어요: {e}");
        }
        let sup = Supervisor(Arc::new(Inner {
            deps,
            catalog,
            recent: RecentWrites::default(),
            rt,
            watches: Mutex::default(),
            lanes: Mutex::default(),
            undo_lock: Mutex::default(),
            stopped: CancellationToken::new(),
        }));
        sup.sync();
        let (weak, stopped) = (Arc::downgrade(&sup.0), sup.0.stopped.clone());
        sup.0.rt.spawn(async move {
            loop {
                tokio::select! {
                    () = stopped.cancelled() => break,
                    () = tokio::time::sleep(every) => {}
                }
                match weak.upgrade() {
                    Some(inner) => Supervisor(inner).recheck(),
                    None => break,
                }
            }
        });
        Ok(sup)
    }

    /// 감시를 모두 풀고 대기 실행을 버린다. 진행 중인 실행은 끝까지 돌아 기록을 남긴다.
    /// 사본(`Clone`)이 남아 drop되지 않을 수 있어 명시적으로 부른다. 되돌릴 수 없다.
    pub fn stop(&self) {
        self.0.stopped.cancel();
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
        let watches = lock(&self.0.watches);
        Ok(list
            .into_iter()
            .map(|workflow| {
                let (watch, watch_message) = match watches.get(&workflow.id) {
                    None => (WatchState::Off, None),
                    Some(w) if w.watcher.is_some() => (WatchState::Watching, None),
                    Some(w) => (WatchState::WatchError, w.error.clone()),
                };
                WorkflowEntry {
                    workflow,
                    watch,
                    watch_message,
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

    pub fn save_workflow(&self, wf: StoredWorkflow) -> Result<StoredWorkflow, String> {
        self.check_folders(&wf.workflow)?;
        self.check_providers(&wf.workflow)?;
        let saved = self.store().save_workflow(wf)?;
        self.sync();
        Ok(saved)
    }

    pub fn delete_workflow(&self, id: &str) -> Result<(), String> {
        self.store().delete_workflow(id)?;
        self.sync();
        Ok(())
    }

    /// 저장된 워크플로우에 맞춰 감시를 다시 건다. 폴더가 그대로이고 살아 있는 감시는 유지하고,
    /// 오류 상태면 바로 다시 건다. 꺼지거나·지워지거나·폴더가 바뀐 워크플로우의 대기 실행은 버린다.
    pub fn sync(&self) {
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

    /// 감시 폴더 `folder`에서 온 트리거를 실행 줄에 넣는다. 같은 워크플로우는 한 번에 하나씩 돈다.
    pub fn enqueue(&self, workflow_id: &str, folder: &Path, trigger: Map<String, Value>) {
        self.push(
            workflow_id,
            Job {
                trigger,
                folder: Some(folder.to_path_buf()),
                reply: None,
            },
        );
    }

    fn push(&self, workflow_id: &str, job: Job) {
        let mut lanes = lock(&self.0.lanes);
        if self.is_stopped() {
            if let Some(reply) = job.reply {
                let _ = reply.send(Err("계정이 바뀌거나 로그아웃되어 취소됐어요.".into()));
            }
            return;
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
            let result = self.run_one(workflow_id, job.folder, job.trigger).await;
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
        folder: Option<PathBuf>,
        trigger: Map<String, Value>,
    ) -> Result<String, String> {
        let wf = self.store().get_workflow(workflow_id)?;
        // 감시가 보낸 뒤 꺼졌거나 폴더가 바뀐 워크플로우의 늦은 트리거는 버린다.
        if let Some(f) = folder
            && trigger_folder(&wf) != Some(f)
        {
            return Err("꺼졌거나 감시 폴더가 바뀌어 늦게 온 트리거를 버렸어요.".into());
        }
        // 저장 때 검사했지만 저장소에 직접 심은 정의도 막는다.
        self.check_folders(&wf.workflow)?;
        let roots = roots_for_workflow(&wf.workflow);
        let (journal, recent) = (Journal::default(), self.0.recent.clone());
        let mut runners = Runners::with_builtin();
        let local: [(&str, Arc<dyn NodeRunner>); 4] = [
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
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let emit = self.0.deps.emit.clone();
        let forward = async {
            let (mut run_id, mut saved_ops, mut finished) = (String::new(), 0, None);
            while let Some(event) = rx.recv().await {
                if let ExecEvent::RunStarted { run_id: r } = &event {
                    run_id = r.clone();
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
        let stored = stored(run, now());
        let saved = self.store().save_run(&stored);
        if let Some(event) = finished {
            emit(AppEvent::Exec {
                workflow_id: workflow_id.to_string(),
                event,
            });
        }
        saved?;
        Ok(stored.run.run_id)
    }

    /// 고른 파일로 시험 실행(같은 줄을 탄다). 끝나면 run_id.
    /// 파일은 그 워크플로우의 허용 폴더 안 일반 파일(심볼릭 링크 아님)이어야 한다.
    pub async fn run_now(&self, workflow_id: &str, file: &Path) -> Result<String, String> {
        let wf = self.store().get_workflow(workflow_id)?;
        self.check_folders(&wf.workflow)?;
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
        if !roots_for_workflow(&wf.workflow).contains(&real) {
            return Err(format!(
                "이 워크플로우가 다루는 폴더 밖 파일이에요: {}",
                real.display()
            ));
        }
        let lossy = |s: Option<&std::ffi::OsStr>| {
            s.map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let trigger = trigger_output(&NewFile {
            name: lossy(real.file_name()),
            extension: lossy(real.extension()),
            path: real.clone(),
            size: meta.len(),
        });
        let (tx, rx) = oneshot::channel();
        self.push(
            workflow_id,
            Job {
                trigger,
                folder: None,
                reply: Some(tx),
            },
        );
        rx.await.map_err(|_| "실행이 취소됐어요.".to_string())?
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
        // 기록이 심어졌어도 정책 밖 폴더에서는 움직이지 않는다.
        let roots = AllowedRoots::new(
            run.allowed_roots
                .iter()
                .filter(|r| self.check_folder(r).is_ok())
                .cloned(),
        );
        let report = undo(
            &run.file_ops,
            &roots,
            &self.0.deps.trash_dir,
            &self.0.recent,
        );
        run.undo = Some(report.clone());
        self.store().save_run(&run)?;
        Ok(report)
    }
}
