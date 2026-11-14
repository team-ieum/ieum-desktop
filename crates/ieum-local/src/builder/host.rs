use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ieum_engine::{NodeRecord, RunStatus, Workflow};

use super::ChatFile;
use crate::UndoReport;

/// 저장본 한 벌. `updated_at`은 읽을 때의 값 — `Host::save`가 기대값으로 쓴다.
#[derive(Debug, Clone)]
pub struct Doc {
    pub name: String,
    pub enabled: bool,
    pub workflow: Workflow,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub enum SaveError {
    /// 읽은 뒤 저장본이 바뀌었다.
    Stale,
    /// 저장 검사(`check_folders`·`check_providers`)·쓰기 실패 문구(사용자 화면용).
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiDefault {
    pub provider: String,
    pub credential: Option<String>,
    pub model: Option<String>,
    /// 추론 수준. None이면 기본.
    pub effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestRun {
    pub run_id: String,
    pub status: RunStatus,
    pub nodes: BTreeMap<String, NodeRecord>,
    pub undo: UndoReport,
}

/// Builder가 앱에 기대는 일. 앱 구현은 `src-tauri`의 `AppHost`, 테스트는 메모리 가짜.
#[async_trait]
pub trait Host: Send + Sync {
    fn load(&self, workflow_id: &str) -> Result<Doc, String>;
    /// `doc.updated_at`이 저장본과 다르면 Stale. 저장 검사 + 저장 + 감시 sync + `workflow-changed` 알림까지 한다.
    fn save(&self, workflow_id: &str, doc: Doc) -> Result<Doc, SaveError>;
    /// 없으면 Default.
    fn load_chat(&self, workflow_id: &str) -> Result<ChatFile, String>;
    /// 읽기-고치기-쓰기 한 번(전역 잠금).
    fn update_chat(
        &self,
        workflow_id: &str,
        f: &mut dyn FnMut(&mut ChatFile),
    ) -> Result<ChatFile, String>;
    /// `validate_user_folder` 정책.
    fn check_folder(&self, path: &Path) -> Result<(), String>;
    /// 편집기에 저장 안 된 편집(dirty)이 있는가. 참이면 Builder가 고치는 동작을 `USER_EDITING`으로 거절한다(사람의 편집 우선).
    fn user_editing(&self, workflow_id: &str) -> bool;
    /// 앱 창을 앞으로 가져와 시스템 폴더 고르기 창. `title` = 창 제목(어느 노드의 어느 칸인지). 취소·5분 초과 = None.
    async fn pick_folder(&self, workflow_id: &str, title: &str) -> Option<PathBuf>;
    /// 터미널용 기본 AI 연결(설계 3절 순서의 첫 번째). 없으면 None.
    async fn ai_default(&self) -> Option<AiDefault>;
    async fn ai_ready(&self, provider: &str, credential: Option<&str>) -> bool;
    /// `wf`(대상까지 자른 그래프)를 `sample` 파일로 실제 실행하고 자동으로 되돌린 뒤 결과를 준다.
    async fn run_test(
        &self,
        workflow_id: &str,
        wf: Workflow,
        sample: &Path,
    ) -> Result<TestRun, String>;
    /// 완료 조건 통과 → 확인 카드 알림.
    fn finish_pending(&self, workflow_id: &str, workflow_name: &str);
}
