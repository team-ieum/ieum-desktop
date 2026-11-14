//! Builder 테스트용 메모리 가짜 Host. 테스트 파일에서 `mod builder_support;`로 쓴다.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ieum_engine::Workflow;
use ieum_local::builder::{AiDefault, ChatFile, Doc, Host, SaveError, TestRun};
use ieum_local::user_folders;

type OnPick = Box<dyn FnMut(&MemHost) + Send>;

#[derive(Default)]
pub struct MemHost {
    pub docs: Mutex<HashMap<String, Doc>>,
    pub chats: Mutex<HashMap<String, ChatFile>>,
    /// `pick_folder`가 앞에서 하나씩 꺼낸다. 비면 None(취소).
    pub picks: Mutex<VecDeque<Option<PathBuf>>>,
    /// 고르기를 기다리는 사이의 캔버스 편집 흉내 — `pick_folder`가 돌아가기 전에 부른다.
    pub on_pick: Mutex<Option<OnPick>>,
    /// `load`가 읽은 직후 한 번 — 읽기 사이에 끼어든 빌더 편집 흉내.
    pub on_load: Mutex<Option<OnPick>>,
    /// `check_folder`·`save`가 거부하는 폴더.
    pub bad_folders: Mutex<Vec<PathBuf>>,
    pub ai: Mutex<Option<AiDefault>>,
    pub ai_ok: AtomicBool,
    pub test_runs: Mutex<VecDeque<Result<TestRun, String>>>,
    pub tested: Mutex<Vec<(Workflow, PathBuf)>>,
    /// `finish_pending`이 받은 워크플로우 id.
    pub pending_notices: Mutex<Vec<String>>,
    /// 성공한 `Host::save` 수(`put_doc`은 세지 않는다).
    pub saves: AtomicUsize,
    /// 다음 `save` 한 번을 이 오류로 실패시킨다.
    pub next_save_error: Mutex<Option<SaveError>>,
    pub editing: Mutex<HashSet<String>>,
    /// `pick_folder`가 받은 창 제목.
    pub pick_titles: Mutex<Vec<String>>,
    /// `updated_at` = `"v<n>"`.
    version: AtomicUsize,
}

impl MemHost {
    /// 켜진 워크플로우 하나("정리", `updated_at` = `"v0"`).
    pub fn with(id: &str, wf: serde_json::Value) -> Arc<MemHost> {
        let host = MemHost::default();
        host.docs.lock().unwrap().insert(
            id.to_string(),
            Doc {
                name: "정리".into(),
                enabled: true,
                workflow: serde_json::from_value(wf).unwrap(),
                updated_at: "v0".into(),
            },
        );
        Arc::new(host)
    }

    pub fn doc(&self, id: &str) -> Doc {
        self.docs.lock().unwrap()[id].clone()
    }

    fn next_version(&self) -> String {
        format!("v{}", self.version.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// 캔버스 저장 흉내 — `updated_at`도 올린다.
    pub fn put_doc(&self, id: &str, f: impl FnOnce(&mut Doc)) {
        let mut docs = self.docs.lock().unwrap();
        let doc = docs.get_mut(id).unwrap();
        f(doc);
        doc.updated_at = self.next_version();
    }

    // ── 테스트가 칸을 만지는 한 줄짜리 도우미 ──

    pub fn saves(&self) -> usize {
        self.saves.load(Ordering::SeqCst)
    }

    pub fn set_ai(&self, ai: Option<AiDefault>) {
        *self.ai.lock().unwrap() = ai;
    }

    pub fn set_ai_ok(&self, ok: bool) {
        self.ai_ok.store(ok, Ordering::SeqCst);
    }

    pub fn push_pick(&self, pick: Option<PathBuf>) {
        self.picks.lock().unwrap().push_back(pick);
    }

    pub fn on_pick(&self, f: impl FnMut(&MemHost) + Send + 'static) {
        *self.on_pick.lock().unwrap() = Some(Box::new(f));
    }

    pub fn on_load(&self, f: impl FnMut(&MemHost) + Send + 'static) {
        *self.on_load.lock().unwrap() = Some(Box::new(f));
    }

    pub fn bad_folder(&self, path: impl Into<PathBuf>) {
        self.bad_folders.lock().unwrap().push(path.into());
    }

    pub fn fail_next_save(&self, e: SaveError) {
        *self.next_save_error.lock().unwrap() = Some(e);
    }

    pub fn set_editing(&self, id: &str, editing: bool) {
        let mut set = self.editing.lock().unwrap();
        if editing {
            set.insert(id.to_string());
        } else {
            set.remove(id);
        }
    }

    pub fn pick_titles(&self) -> Vec<String> {
        self.pick_titles.lock().unwrap().clone()
    }

    pub fn push_test_run(&self, run: Result<TestRun, String>) {
        self.test_runs.lock().unwrap().push_back(run);
    }

    pub fn tested(&self) -> Vec<(Workflow, PathBuf)> {
        self.tested.lock().unwrap().clone()
    }

    pub fn pending_notices(&self) -> Vec<String> {
        self.pending_notices.lock().unwrap().clone()
    }

    fn is_bad(&self, path: &Path) -> bool {
        self.bad_folders.lock().unwrap().iter().any(|b| b == path)
    }
}

#[async_trait]
impl Host for MemHost {
    fn load(&self, workflow_id: &str) -> Result<Doc, String> {
        let doc = self.docs.lock().unwrap().get(workflow_id).cloned();
        let hook = self.on_load.lock().unwrap().take();
        if let Some(mut f) = hook {
            f(self);
        }
        doc.ok_or("없음".into())
    }

    fn save(&self, workflow_id: &str, mut doc: Doc) -> Result<Doc, SaveError> {
        if let Some(e) = self.next_save_error.lock().unwrap().take() {
            return Err(e);
        }
        let mut docs = self.docs.lock().unwrap();
        if docs.get(workflow_id).map(|d| &d.updated_at) != Some(&doc.updated_at) {
            return Err(SaveError::Stale);
        }
        if user_folders(&doc.workflow).iter().any(|f| self.is_bad(f)) {
            return Err(SaveError::Rejected("쓸 수 없는 폴더".into()));
        }
        doc.updated_at = self.next_version();
        docs.insert(workflow_id.to_string(), doc.clone());
        self.saves.fetch_add(1, Ordering::SeqCst);
        Ok(doc)
    }

    fn load_chat(&self, workflow_id: &str) -> Result<ChatFile, String> {
        let chats = self.chats.lock().unwrap();
        Ok(chats.get(workflow_id).cloned().unwrap_or_default())
    }

    fn update_chat(
        &self,
        workflow_id: &str,
        f: &mut dyn FnMut(&mut ChatFile),
    ) -> Result<ChatFile, String> {
        let mut chats = self.chats.lock().unwrap();
        let chat = chats.entry(workflow_id.to_string()).or_default();
        f(chat);
        Ok(chat.clone())
    }

    fn check_folder(&self, path: &Path) -> Result<(), String> {
        if self.is_bad(path) {
            return Err("쓸 수 없는 폴더".into());
        }
        Ok(())
    }

    fn user_editing(&self, workflow_id: &str) -> bool {
        self.editing.lock().unwrap().contains(workflow_id)
    }

    async fn pick_folder(&self, _workflow_id: &str, title: &str) -> Option<PathBuf> {
        self.pick_titles.lock().unwrap().push(title.to_string());
        let picked = self.picks.lock().unwrap().pop_front().flatten();
        // 콜백이 이 Host를 다시 부르므로 잠금 밖에서 부른다.
        let hook = self.on_pick.lock().unwrap().take();
        if let Some(mut hook) = hook {
            hook(self);
            *self.on_pick.lock().unwrap() = Some(hook);
        }
        picked
    }

    async fn ai_default(&self) -> Option<AiDefault> {
        self.ai.lock().unwrap().clone()
    }

    async fn ai_ready(&self, _provider: &str, _credential: Option<&str>) -> bool {
        self.ai_ok.load(Ordering::SeqCst)
    }

    async fn run_test(
        &self,
        _workflow_id: &str,
        wf: Workflow,
        sample: &Path,
    ) -> Result<TestRun, String> {
        self.tested.lock().unwrap().push((wf, sample.to_path_buf()));
        let next = self.test_runs.lock().unwrap().pop_front();
        next.unwrap_or_else(|| Err("준비된 테스트 결과가 없음".into()))
    }

    fn finish_pending(&self, workflow_id: &str, _workflow_name: &str) {
        self.pending_notices
            .lock()
            .unwrap()
            .push(workflow_id.to_string());
    }
}
