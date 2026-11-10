//! 실행 단위 되돌리기. 기록을 역순으로 `to → from` 옮긴다. 덮어쓰지 않고, 못 되돌리는 작업만 건너뛴다.

use std::fs;
use std::io;

use serde::{Deserialize, Serialize};

use crate::files::rename_excl;
use crate::journal::{FileId, FileOp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum UndoSkip {
    /// 바뀐 경로에 파일이 없음(그사이 또 옮겨지거나 지워짐).
    MissingNow,
    /// 원래 자리에 이미 무언가 있음.
    OriginalOccupied,
    /// 바뀐 경로에 같은 이름의 다른 파일이 있음.
    Replaced,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReport {
    pub restored: Vec<FileOp>,
    pub skipped: Vec<(FileOp, UndoSkip)>,
}

pub fn undo(ops: &[FileOp]) -> UndoReport {
    let mut report = UndoReport::default();
    for op in ops.iter().rev() {
        match undo_one(op) {
            Ok(()) => report.restored.push(op.clone()),
            Err(why) => report.skipped.push((op.clone(), why)),
        }
    }
    report
}

fn undo_one(op: &FileOp) -> Result<(), UndoSkip> {
    let failed = |e: io::Error| UndoSkip::Failed {
        message: e.to_string(),
    };
    match FileId::of(&op.to) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(UndoSkip::MissingNow),
        Err(e) => return Err(failed(e)),
        Ok(id) if op.file_id.is_some_and(|rec| rec != id) => return Err(UndoSkip::Replaced),
        Ok(_) => {}
    }
    if let Some(dir) = op.from.parent() {
        fs::create_dir_all(dir).map_err(failed)?;
    }
    rename_excl(&op.to, &op.from).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => UndoSkip::OriginalOccupied,
        io::ErrorKind::NotFound => UndoSkip::MissingNow,
        _ => failed(e),
    })
}
