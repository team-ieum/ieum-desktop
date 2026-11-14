//! 앱 데이터 폴더 저장: `workflows/<id>.json`, `runs/<run_id>.json`, `settings.json`.
//! id는 UUID(소문자 하이픈 형식)만 — 파일 이름이 되므로 `../` 같은 값으로 폴더 밖에 쓰지 못하게.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ieum_engine::{RunResult, RunStatus, Workflow};
use ieum_local::ai::AiUsage;
use ieum_local::builder::ChatFile;
use ieum_local::{FileOp, UndoReport};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredWorkflow {
    /// 비어 있으면 저장할 때 새 UUID를 붙인다.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub workflow: Workflow,
    /// UTC ISO 8601. 저장할 때 채운다.
    #[serde(default)]
    pub updated_at: String,
}

/// 실행 기록. 앱 데이터 폴더에만 둔다 — 되돌리기는 여기 적힌 `allowed_roots` 안에서만 움직인다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRun {
    pub run: RunResult,
    pub workflow_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub file_ops: Vec<FileOp>,
    /// 그 실행의 `roots_for_workflow` 결과(실제 경로).
    pub allowed_roots: Vec<PathBuf>,
    pub undo: Option<UndoReport>,
    /// 옛 기록에는 없다.
    #[serde(default)]
    pub trigger: Option<RunTrigger>,
}

/// 채팅 패널의 AI 호출 한 건(채팅 파일의 `usage` 항목 + 어느 워크플로우의 채팅인지).
#[derive(Debug, Clone)]
pub struct ChatUsage {
    pub workflow_id: String,
    pub at: String,
    pub usage: AiUsage,
}

/// 폴더 감시로 돌았는지, 지금 실행(수동)인지, 채팅 빌더의 노드 테스트인지.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunTrigger {
    Watch,
    Manual,
    /// 끝나면 바로 되돌린다. 대시보드 실행 건수·알림에서 뺀다.
    Test,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub workflow_id: String,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: String,
    pub file_ops: usize,
    pub undone: bool,
    pub trigger: Option<RunTrigger>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    Ko,
    En,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: Language,
    pub autostart: bool,
    /// 옛 설정 호환용으로만 남긴다(안 씀).
    pub first_run_done: bool,
    /// 온보딩을 끝냈는가(기기 단위). 옛 설정엔 없어 false.
    pub onboarding_done: bool,
    /// 내 PC 실행 시스템 알림 토글. 옛 설정엔 없어 `Default`의 true.
    pub notify_manual_start: bool,
    pub notify_manual_end: bool,
    pub notify_watch_end: bool,
    /// 이음 BE 주소. 바꾸면 세션을 지운다(`web::save_settings`).
    pub server_url: String,
    /// 로그인된(마지막 활성) 계정. 없으면 로그인 관문.
    pub current_account: Option<crate::account::AccountId>,
    /// 알림을 마지막으로 연 시각(RFC3339). 빈 문자열 = 한 번도 안 열어 봄.
    pub notifications_seen_at: String,
    /// 확인 창 토글. 옛 설정엔 없어 `Default`의 true.
    pub confirm_delete_workflow: bool,
    pub confirm_discard_unsaved: bool,
}

pub const DEFAULT_SERVER_URL: &str = "https://green.kiacms.co.kr";

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            autostart: false,
            first_run_done: false,
            onboarding_done: false,
            notify_manual_start: true,
            notify_manual_end: true,
            notify_watch_end: true,
            server_url: DEFAULT_SERVER_URL.into(),
            current_account: None,
            notifications_seen_at: String::new(),
            confirm_delete_workflow: true,
            confirm_discard_unsaved: true,
        }
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// UUID 정규형(`Uuid::to_string`과 같은 글자)만 통과.
fn check_id(id: &str) -> Result<(), String> {
    match uuid::Uuid::try_parse(id) {
        Ok(u) if u.to_string() == id => Ok(()),
        _ => Err(format!("잘못된 id예요: {id:?}")),
    }
}

/// 채팅 상태 파일 읽기·쓰기를 한 프로세스 안에서 차례로.
static CHAT_LOCK: Mutex<()> = Mutex::new(());

/// 임시 파일에 쓰고 rename — 쓰다 죽어도 이전 파일이 남는다.
pub(crate) fn write_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let dir = path.parent().ok_or("저장 경로가 잘못됐어요.")?;
    fs::create_dir_all(dir).map_err(err)?;
    let tmp = dir.join(format!(".{}.tmp", new_id()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    // 실패 때 남는 `.<uuid>.tmp`는 목록에서 무시된다(삭제는 delete_workflow에서만).
    fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(&bytes).and_then(|()| f.sync_all()))
        .and_then(|()| fs::rename(&tmp, path))
        .map_err(err)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// 폴더 안 `<uuid>.json`들을 읽는다. 없는 폴더는 빈 목록, 깨진 파일은 건너뛴다.
fn read_all<T: DeserializeOwned>(dir: &Path) -> Result<Vec<T>, String> {
    Ok(read_named(dir)?.into_iter().map(|(_, v)| v).collect())
}

/// `read_all`과 같되 파일 이름의 id도 같이 준다.
fn read_named<T: DeserializeOwned>(dir: &Path) -> Result<Vec<(String, T)>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    Ok(entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let id = p.file_stem()?.to_str()?.to_string();
            check_id(&id).ok()?;
            let value = read_json(&p)
                .inspect_err(|e| eprintln!("[ieum] 기록을 읽지 못해 건너뜀: {e}"))
                .ok()?;
            Some((id, value))
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 앱 데이터 폴더.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn workflow_path(&self, id: &str) -> Result<PathBuf, String> {
        check_id(id)?;
        Ok(self.root.join("workflows").join(format!("{id}.json")))
    }

    fn run_path(&self, id: &str) -> Result<PathBuf, String> {
        check_id(id)?;
        Ok(self.root.join("runs").join(format!("{id}.json")))
    }

    fn chat_path(&self, id: &str) -> Result<PathBuf, String> {
        check_id(id)?;
        Ok(self.root.join("chats").join(format!("{id}.json")))
    }

    /// 채팅 상태 파일(`chats/<워크플로우 id>.json`). 없으면 빈 값.
    pub fn get_chat(&self, workflow_id: &str) -> Result<ChatFile, String> {
        let path = self.chat_path(workflow_id)?;
        if !path.exists() {
            return Ok(ChatFile::default());
        }
        read_json(&path)
    }

    /// `CHAT_LOCK`을 잡은 채 부른다. 지워진 워크플로우에 늦게 도착한 쓰기가 고아 채팅 파일을 남기지 않게
    /// (`delete_workflow`는 워크플로우 파일을 먼저 지우고 이 잠금 안에서 채팅 파일을 지운다).
    fn chat_owner_exists(&self, workflow_id: &str) -> Result<(), String> {
        if self.workflow_path(workflow_id)?.exists() {
            Ok(())
        } else {
            Err(format!("없는 워크플로우예요: {workflow_id}"))
        }
    }

    pub fn save_chat(&self, workflow_id: &str, chat: &ChatFile) -> Result<(), String> {
        let _g = CHAT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.chat_owner_exists(workflow_id)?;
        write_atomic(&self.chat_path(workflow_id)?, chat)
    }

    /// 읽기-고치기-쓰기 한 번. 여러 곳(빌더·채팅 패널)이 같은 파일을 고치므로 늘 이걸로 고친다.
    pub fn update_chat(
        &self,
        workflow_id: &str,
        f: impl FnOnce(&mut ChatFile),
    ) -> Result<ChatFile, String> {
        let _g = CHAT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.chat_owner_exists(workflow_id)?;
        let mut chat = self.get_chat(workflow_id)?;
        f(&mut chat);
        write_atomic(&self.chat_path(workflow_id)?, &chat)?;
        Ok(chat)
    }

    /// 모든 채팅 파일의 사용량(`chats/<워크플로우 id>.json`). 폴더가 없으면 빈 목록, 깨진 파일은 건너뛴다.
    pub fn all_chat_usage(&self) -> Result<Vec<ChatUsage>, String> {
        let chats: Vec<(String, ChatFile)> = read_named(&self.root.join("chats"))?;
        Ok(chats
            .into_iter()
            .flat_map(|(workflow_id, chat)| {
                chat.usage.into_iter().map(move |e| ChatUsage {
                    workflow_id: workflow_id.clone(),
                    at: e.at,
                    usage: e.usage,
                })
            })
            .collect())
    }

    pub fn list_workflows(&self) -> Result<Vec<StoredWorkflow>, String> {
        let mut list: Vec<StoredWorkflow> = read_all(&self.root.join("workflows"))?;
        list.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        Ok(list)
    }

    pub fn get_workflow(&self, id: &str) -> Result<StoredWorkflow, String> {
        read_json(&self.workflow_path(id)?)
    }

    pub fn save_workflow(&self, mut wf: StoredWorkflow) -> Result<StoredWorkflow, String> {
        if wf.id.is_empty() {
            wf.id = new_id();
        }
        let path = self.workflow_path(&wf.id)?;
        wf.updated_at = now();
        write_atomic(&path, &wf)?;
        Ok(wf)
    }

    /// 워크플로우 정의 파일과 그 채팅 상태 파일만 지운다(사용자 파일 아님).
    pub fn delete_workflow(&self, id: &str) -> Result<(), String> {
        let path = self.workflow_path(id)?;
        if !path.starts_with(self.root.join("workflows")) {
            return Err("앱 데이터 폴더 밖이에요.".into());
        }
        fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let chat = self.chat_path(id)?;
        let _g = CHAT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match fs::remove_file(&chat) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", chat.display()))
            }
            _ => Ok(()),
        }
    }

    pub fn save_run(&self, run: &StoredRun) -> Result<(), String> {
        write_atomic(&self.run_path(&run.run.run_id)?, run)
    }

    pub fn get_run(&self, run_id: &str) -> Result<StoredRun, String> {
        read_json(&self.run_path(run_id)?)
    }

    /// 끝 시각 없는 기록(실행 도중 앱이 꺼짐)을 중단됨으로 닫는다. 앱 시작 때 실행 전에 한 번.
    pub fn close_unfinished_runs(&self) -> Result<(), String> {
        let runs: Vec<StoredRun> = read_all(&self.root.join("runs"))?;
        for mut r in runs.into_iter().filter(|r| r.finished_at.is_empty()) {
            r.run.status = RunStatus::Cancelled;
            r.finished_at = now();
            self.save_run(&r)?;
        }
        Ok(())
    }

    /// 모든 실행 기록(순서 없음). 깨진 파일은 건너뛴다.
    pub fn all_runs(&self) -> Result<Vec<StoredRun>, String> {
        read_all(&self.root.join("runs"))
    }

    /// 최근 실행부터.
    // ponytail: 매번 기록 파일을 전부 읽는다 — 기록이 수천 개가 되면 요약 색인 파일
    pub fn list_runs(&self, workflow_id: Option<&str>) -> Result<Vec<RunSummary>, String> {
        let runs = self.all_runs()?;
        let mut list: Vec<RunSummary> = runs
            .into_iter()
            .filter(|r| workflow_id.is_none_or(|w| r.workflow_id == w))
            .map(|r| RunSummary {
                run_id: r.run.run_id,
                workflow_id: r.workflow_id,
                status: r.run.status,
                started_at: r.started_at,
                finished_at: r.finished_at,
                file_ops: r.file_ops.len(),
                undone: r.undo.is_some(),
                trigger: r.trigger,
            })
            .collect();
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(list)
    }

    pub fn get_settings(&self) -> Result<Settings, String> {
        let path = self.root.join("settings.json");
        if !path.exists() {
            return Ok(Settings::default());
        }
        read_json(&path)
    }

    pub fn set_settings(&self, s: &Settings) -> Result<(), String> {
        write_atomic(&self.root.join("settings.json"), s)
    }
}
