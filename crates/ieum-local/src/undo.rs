//! 실행 단위 되돌리기. 기록을 역순으로 `to → from` 옮긴다. 덮어쓰지 않고, 못 되돌리는 작업만 건너뛴다.
//! 기록은 변조됐을 수 있으므로 경로가 허용 폴더(휴지통 작업은 휴지통) 안일 때만 움직인다.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::files::rename_excl;
use crate::journal::{FileId, FileOp, FileOpKind, RecentWrites};
use crate::paths::AllowedRoots;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum UndoSkip {
    /// 바뀐 경로에 파일이 없음(그사이 또 옮겨지거나 지워짐).
    MissingNow,
    /// 원래 자리에 이미 무언가 있음.
    OriginalOccupied,
    /// 바뀐 경로의 파일이 그사이 다른 파일로 바뀜(같은 이름의 다른 파일, 편집기가 새 파일로 저장한 경우도 포함).
    Replaced,
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub restored: Vec<FileOp>,
    pub skipped: Vec<(FileOp, UndoSkip)>,
}

/// 되돌린 `from`은 `recent`에 mark한다(감시 폴더로 돌아온 파일이 트리거를 다시 일으키지 않게).
pub fn undo(
    ops: &[FileOp],
    roots: &AllowedRoots,
    trash_dir: &Path,
    recent: &RecentWrites,
) -> UndoReport {
    let trash = trash_dir.canonicalize().ok();
    let mut report = UndoReport::default();
    for op in ops.iter().rev() {
        match undo_one(op, roots, trash.as_deref()) {
            Ok(()) => {
                recent.mark(&op.from);
                report.restored.push(op.clone())
            }
            Err(why) => report.skipped.push((op.clone(), why)),
        }
    }
    report
}

fn failed(message: impl Into<String>) -> UndoSkip {
    UndoSkip::Failed {
        message: message.into(),
    }
}

/// 절대 경로이고 `..`가 없어야 한다.
fn plain_abs(p: &Path) -> bool {
    p.is_absolute() && !p.components().any(|c| c == Component::ParentDir)
}

/// `p`(포함)에서 위로 올라가며 처음 존재하는 조상의 실제 경로.
fn real_ancestor(p: &Path) -> Option<PathBuf> {
    p.ancestors().find_map(|a| a.canonicalize().ok())
}

fn in_scope(op: &FileOp, roots: &AllowedRoots, trash: Option<&Path>) -> bool {
    if !plain_abs(&op.from) || !plain_abs(&op.to) {
        return false;
    }
    // from 자체와 그 폴더는 없을 수 있다 — 존재하는 가장 가까운 조상으로 판단(재생성도 이 아래만).
    let from_ok = op
        .from
        .parent()
        .and_then(real_ancestor)
        .is_some_and(|r| roots.contains(&r));
    let to_ok = op.to.parent().and_then(real_ancestor).is_some_and(|r| {
        roots.contains(&r)
            || (op.kind == FileOpKind::Trash && trash.is_some_and(|t| r.starts_with(t)))
    });
    from_ok && to_ok
}

fn undo_one(op: &FileOp, roots: &AllowedRoots, trash: Option<&Path>) -> Result<(), UndoSkip> {
    if !in_scope(op, roots, trash) {
        return Err(failed("기록이 허용 범위를 벗어나 되돌리지 않았어요."));
    }
    let Some(recorded) = op.file_id else {
        return Err(failed("파일 확인 정보가 없어 되돌리지 않았어요."));
    };
    let io_failed = |e: io::Error| failed(e.to_string());
    // dev는 재마운트·재부팅으로 바뀔 수 있어 ino만 본다(같은 볼륨 안 작업뿐).
    match FileId::of(&op.to) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(UndoSkip::MissingNow),
        Err(e) => return Err(io_failed(e)),
        Ok(id) if id.ino != recorded.ino => return Err(UndoSkip::Replaced),
        Ok(_) => {}
    }
    if let Some(dir) = op.from.parent() {
        fs::create_dir_all(dir).map_err(io_failed)?;
    }
    rename_excl(&op.to, &op.from).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => UndoSkip::OriginalOccupied,
        io::ErrorKind::NotFound => UndoSkip::MissingNow,
        _ => io_failed(e),
    })
}
