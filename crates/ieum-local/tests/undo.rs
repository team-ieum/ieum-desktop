//! 실행 단위 되돌리기. 모든 파일은 임시 폴더 안에서만 만든다(휴지통도 임시 폴더로 주입).

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use ieum_engine::{Node, NodeCtx, NodeRunner};
use ieum_local::{
    AllowedRoots, FileId, FileMoveRunner, FileOp, FileRenameRunner, FileTrashRunner, Journal,
    RecentWrites, UndoSkip, undo,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

async fn run(runner: &dyn NodeRunner, config: Value) -> Map<String, Value> {
    let node: Node = serde_json::from_value(json!({"id": "n", "event": "x"})).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    runner
        .run(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            events: &tx,
            cancel: &cancel,
        })
        .await
        .unwrap()
}

/// 임시 폴더와 그 실제 경로(macOS `/var` → `/private/var`).
fn tmp() -> (TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    (t, root)
}

fn write(p: &Path, body: &str) -> PathBuf {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
    p.to_path_buf()
}

fn s(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

/// `root/in/<name>`을 `root/base/docs/`로 옮긴다. (기록, 새 경로)
async fn moved(root: &Path, name: &str) -> (Journal, PathBuf, PathBuf) {
    let j = Journal::default();
    let m = FileMoveRunner::new(
        j.clone(),
        RecentWrites::default(),
        AllowedRoots::new([root.to_path_buf()]),
    );
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    let src = write(&root.join("in").join(name), name);
    run(
        &m,
        json!({"path": s(&src), "baseFolder": s(&base), "destination": "docs"}),
    )
    .await;
    (j, src, base.join("docs").join(name))
}

#[tokio::test]
async fn undo_restores_in_reverse() {
    let (_t, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    let rn = FileRenameRunner::new(
        j.clone(),
        RecentWrites::default(),
        AllowedRoots::new([root.clone()]),
    );
    run(&rn, json!({"path": s(&at), "newName": "b"})).await;
    assert!(!src.exists());

    let report = undo(&j.ops());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(report.restored.len(), 2);
    assert_eq!(read(&src), "a.pdf");
    assert!(!at.exists() && !root.join("base/docs/b.pdf").exists());
}

#[tokio::test]
async fn undo_skips_conflicts() {
    // 그사이 파일이 또 옮겨짐 → MissingNow
    let (_t, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    fs::rename(&at, root.join("elsewhere.pdf")).unwrap();
    let report = undo(&j.ops());
    assert!(report.restored.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].1, UndoSkip::MissingNow);
    assert!(!src.exists());

    // 원래 자리에 같은 이름이 생김 → OriginalOccupied
    let (_t2, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    write(&src, "user");
    let report = undo(&j.ops());
    assert!(report.restored.is_empty());
    assert_eq!(report.skipped[0].1, UndoSkip::OriginalOccupied);
    assert_eq!(read(&src), "user");
    assert_eq!(read(&at), "a.pdf");
}

#[tokio::test]
async fn undo_never_overwrites() {
    let (_t, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    write(&src, "user's new file");

    let report = undo(&j.ops());
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].1, UndoSkip::OriginalOccupied);
    assert_eq!(read(&src), "user's new file");
    assert_eq!(read(&at), "a.pdf");
}

#[tokio::test]
async fn undo_skips_replaced_file() {
    let (_t, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    // 옮긴 파일은 다른 곳에 남겨 두어(inode 재사용 방지) 같은 이름의 다른 파일로 바꿔치기
    fs::rename(&at, root.join("kept.pdf")).unwrap();
    write(&at, "impostor");

    let report = undo(&j.ops());
    assert!(report.restored.is_empty());
    assert_eq!(report.skipped[0].1, UndoSkip::Replaced);
    assert!(!src.exists());
    assert_eq!(read(&at), "impostor");
}

#[tokio::test]
async fn undo_continues_after_skip() {
    let (_t, root) = tmp();
    let (j1, src1, _) = moved(&root, "a.pdf").await;
    let (j2, src2, at2) = moved(&root, "b.pdf").await;
    fs::rename(&at2, root.join("gone.pdf")).unwrap();
    // b가 나중 작업 → 먼저 건너뛰고, 그래도 a는 되돌린다
    let ops: Vec<FileOp> = j1.ops().into_iter().chain(j2.ops()).collect();

    let report = undo(&ops);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0.from, src2);
    assert_eq!(report.skipped[0].1, UndoSkip::MissingNow);
    assert_eq!(report.restored.len(), 1);
    assert_eq!(read(&src1), "a.pdf");
}

#[tokio::test]
async fn undo_restores_trashed_file() {
    let (_t, root) = tmp();
    let trash = root.join("Trash");
    fs::create_dir_all(&trash).unwrap();
    let src = write(&root.join("in/sub/a.pdf"), "a");
    let j = Journal::default();
    let t = FileTrashRunner::with_trash_dir(
        j.clone(),
        RecentWrites::default(),
        AllowedRoots::new([root.join("in")]),
        trash.clone(),
    );
    run(&t, json!({"path": s(&src)})).await;
    // 원래 폴더가 지워져도 다시 만든다
    fs::remove_dir(root.join("in/sub")).unwrap();

    let report = undo(&j.ops());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(read(&src), "a");
    assert!(!trash.join("a.pdf").exists());
}

#[tokio::test]
async fn file_id_recorded_by_runners() {
    let (_t, root) = tmp();
    let trash = root.join("Trash");
    fs::create_dir_all(&trash).unwrap();
    let (j, _, at) = moved(&root, "a.pdf").await;
    let r = RecentWrites::default();
    let rn = FileRenameRunner::new(j.clone(), r.clone(), AllowedRoots::new([root.clone()]));
    let t = FileTrashRunner::with_trash_dir(j.clone(), r, AllowedRoots::new([root.clone()]), trash);
    let out = run(&rn, json!({"path": s(&at), "newName": "b"})).await;
    let renamed = out["path"].as_str().unwrap().to_string();
    let out = run(&t, json!({"path": renamed})).await;
    let trashed = PathBuf::from(out["path"].as_str().unwrap());

    let ops = j.ops();
    assert_eq!(ops.len(), 3);
    let m = fs::symlink_metadata(&trashed).unwrap();
    let id = FileId {
        dev: m.dev(),
        ino: m.ino(),
    };
    // 옮겨도 같은 파일 → 세 기록 모두 같은 (dev, ino)
    for op in &ops {
        assert_eq!(op.file_id, Some(id), "{op:?}");
    }

    // 예전 기록(file_id 없음)도 읽힌다
    let old: FileOp =
        serde_json::from_value(json!({"kind": "move", "from": "/a", "to": "/b"})).unwrap();
    assert_eq!(old.file_id, None);
}
