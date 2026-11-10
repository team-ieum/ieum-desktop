use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOpKind {
    Move,
    Rename,
    Trash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOp {
    pub kind: FileOpKind,
    pub from: PathBuf,
    pub to: PathBuf,
    /// 작업 직후 `to`의 (dev, ino). 되돌릴 때 바꿔치기 감지용(비교는 ino만).
    #[serde(default, alias = "file_id")]
    pub file_id: Option<FileId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

impl FileId {
    pub(crate) fn of(p: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(p)?;
        Ok(Self {
            dev: m.dev(),
            ino: m.ino(),
        })
    }
}

/// 실행 하나의 파일 작업 기록.
#[derive(Debug, Clone, Default)]
pub struct Journal(Arc<Mutex<Vec<FileOp>>>);

impl Journal {
    pub(crate) fn push(&self, op: FileOp) {
        // 러너 panic 뒤 poison이 다음 작업을 연쇄 실패시키지 않게.
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(op);
    }

    pub fn ops(&self) -> Vec<FileOp> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// 파일 노드가 쓴 목적지 경로. 폴더 감시가 자기 루프를 막는 데 쓴다.
// ponytail: 항목을 지우지 않음 — 상주 앱에서 쌓이는 양이 문제 되면 mark 때 오래된 항목 정리
#[derive(Debug, Clone, Default)]
pub struct RecentWrites(Arc<Mutex<HashMap<PathBuf, Instant>>>);

impl RecentWrites {
    pub fn mark(&self, path: &Path) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_path_buf(), Instant::now());
    }

    pub fn is_recent(&self, path: &Path, within: Duration) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .is_some_and(|t| t.elapsed() <= within)
    }
}
