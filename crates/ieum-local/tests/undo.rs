//! 실행 단위 되돌리기. 모든 파일은 임시 폴더 안에서만 만든다(휴지통도 임시 폴더로 주입).

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use ieum_engine::{Node, NodeCtx, NodeRunner};
use ieum_local::{
    AllowedRoots, FileId, FileMoveRunner, FileOp, FileOpKind, FileRenameRunner, FileTrashRunner,
    Journal, RecentWrites, UndoReport, UndoSkip, undo,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const OUT_OF_SCOPE: &str = "기록이 허용 범위를 벗어나 되돌리지 않았어요.";
const NO_FILE_ID: &str = "파일 확인 정보가 없어 되돌리지 않았어요.";

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

/// 허용 폴더 = `root`, 휴지통 = `root/Trash`.
fn u(ops: &[FileOp], root: &Path) -> UndoReport {
    undo(
        ops,
        &AllowedRoots::new([root.to_path_buf()]),
        &root.join("Trash"),
    )
}

fn failed(message: &str) -> UndoSkip {
    UndoSkip::Failed {
        message: message.into(),
    }
}

/// 실제 파일 `to`를 가리키는 (변조됐을 수 있는) 기록. file_id는 실제 값으로 맞춘다.
fn op(kind: FileOpKind, from: &str, to: &Path) -> FileOp {
    FileOp {
        kind,
        from: PathBuf::from(from),
        to: to.to_path_buf(),
        file_id: Some(id_of(to)),
    }
}

fn id_of(p: &Path) -> FileId {
    let m = fs::symlink_metadata(p).unwrap();
    FileId {
        dev: m.dev(),
        ino: m.ino(),
    }
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

/// `moved` 뒤 `docs/<name>`을 `newName`으로 바꾼다. (기록, 원래 경로, 중간 경로, 마지막 경로)
async fn moved_renamed(root: &Path) -> (Journal, PathBuf, PathBuf, PathBuf) {
    let (j, src, at) = moved(root, "a.pdf").await;
    let rn = FileRenameRunner::new(
        j.clone(),
        RecentWrites::default(),
        AllowedRoots::new([root.to_path_buf()]),
    );
    run(&rn, json!({"path": s(&at), "newName": "b"})).await;
    (j, src, at, root.join("base/docs/b.pdf"))
}

#[tokio::test]
async fn undo_restores_in_reverse() {
    let (_t, root) = tmp();
    let (j, src, at, last) = moved_renamed(&root).await;
    assert!(!src.exists());

    let report = u(&j.ops(), &root);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let kinds: Vec<_> = report.restored.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, vec![FileOpKind::Rename, FileOpKind::Move]);
    assert_eq!(read(&src), "a.pdf");
    assert!(!at.exists() && !last.exists());
}

#[tokio::test]
async fn undo_skips_conflicts() {
    // 그사이 파일이 또 옮겨짐 → MissingNow
    let (_t, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    fs::rename(&at, root.join("elsewhere.pdf")).unwrap();
    let report = u(&j.ops(), &root);
    assert!(report.restored.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].1, UndoSkip::MissingNow);
    assert!(!src.exists());

    // 원래 자리에 같은 이름이 생김 → OriginalOccupied
    let (_t2, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    write(&src, "user");
    let report = u(&j.ops(), &root);
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

    let report = u(&j.ops(), &root);
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

    let report = u(&j.ops(), &root);
    assert!(report.restored.is_empty());
    assert_eq!(report.skipped[0].1, UndoSkip::Replaced);
    assert!(!src.exists());
    assert_eq!(read(&at), "impostor");
}

#[tokio::test]
async fn replaced_ignores_dev() {
    // dev는 재마운트로 바뀔 수 있어 ino만 비교한다.
    let (_t, root) = tmp();
    let (j, src, _) = moved(&root, "a.pdf").await;
    let mut ops = j.ops();
    ops[0].file_id.as_mut().unwrap().dev ^= 1;

    let report = u(&ops, &root);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(read(&src), "a.pdf");
}

#[tokio::test]
async fn undo_continues_after_skip() {
    let (_t, root) = tmp();
    let (j1, src1, _) = moved(&root, "a.pdf").await;
    let (j2, src2, at2) = moved(&root, "b.pdf").await;
    let (j3, src3, _) = moved(&root, "c.pdf").await;
    fs::rename(&at2, root.join("gone.pdf")).unwrap();
    let ops: Vec<FileOp> = [j1, j2, j3].iter().flat_map(|j| j.ops()).collect();

    let report = u(&ops, &root);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0.from, src2);
    assert_eq!(report.skipped[0].1, UndoSkip::MissingNow);
    // 역순: c → (b 건너뜀) → a
    let froms: Vec<_> = report.restored.iter().map(|o| o.from.clone()).collect();
    assert_eq!(froms, vec![src3.clone(), src1.clone()]);
    assert_eq!(read(&src1), "a.pdf");
    assert_eq!(read(&src3), "c.pdf");
}

#[tokio::test]
async fn chain_skip_in_middle_leaves_user_file() {
    // 이동 → 이름 바꾸기 뒤, 사용자가 중간 자리(docs/a.pdf)에 새 파일을 둠.
    let (_t, root) = tmp();
    let (j, src, at, last) = moved_renamed(&root).await;
    write(&at, "user new");

    let report = u(&j.ops(), &root);
    assert!(report.restored.is_empty());
    let why: Vec<_> = report.skipped.iter().map(|(_, w)| w.clone()).collect();
    assert_eq!(why, vec![UndoSkip::OriginalOccupied, UndoSkip::Replaced]);
    assert_eq!(read(&at), "user new");
    assert_eq!(read(&last), "a.pdf");
    assert!(!src.exists());
}

#[tokio::test]
async fn io_error_is_failed_and_others_continue() {
    let (_t, root) = tmp();
    let (j1, src1, at1) = moved(&root, "a.pdf").await;
    // 두 번째 파일은 다른 원래 폴더에서 옮긴다
    let j2 = Journal::default();
    let m = FileMoveRunner::new(
        j2.clone(),
        RecentWrites::default(),
        AllowedRoots::new([root.clone()]),
    );
    let src2 = write(&root.join("in2/b.pdf"), "b");
    run(
        &m,
        json!({"path": s(&src2), "baseFolder": s(&root.join("base")), "destination": "docs"}),
    )
    .await;
    let ops: Vec<FileOp> = j1.ops().into_iter().chain(j2.ops()).collect();
    // 첫 원래 폴더에 쓰기 금지
    let ro = root.join("in");
    fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();

    let report = u(&ops, &root);
    fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0.from, src1);
    assert!(
        matches!(&report.skipped[0].1, UndoSkip::Failed { message } if !message.is_empty()),
        "{:?}",
        report.skipped
    );
    assert_eq!(read(&at1), "a.pdf");
    assert_eq!(read(&src2), "b");
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

    let report = undo(&j.ops(), &AllowedRoots::new([root.join("in")]), &trash);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(read(&src), "a");
    assert!(!trash.join("a.pdf").exists());
}

#[test]
fn tampered_from_outside_roots_fails() {
    let (_t, root) = tmp();
    let (_o, outside) = tmp();
    let to = write(&root.join("x.pdf"), "x");
    let dest = outside.join("new/deep/x.pdf");
    let report = u(&[op(FileOpKind::Move, &s(&dest), &to)], &root);

    assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE));
    assert_eq!(read(&to), "x");
    assert!(!outside.join("new").exists());
}

#[test]
fn tampered_dotdot_fails() {
    let (_t, root) = tmp();
    let to = write(&root.join("in/x.pdf"), "x");
    let escaped = root.parent().unwrap().join("ieum-escaped-dir");
    // `..`가 허용 폴더 안으로 되돌아오는 경우도 거부
    for from in [
        format!("{}/in/../../ieum-escaped-dir/x.pdf", s(&root)),
        format!("{}/in/../y.pdf", s(&root)),
    ] {
        let report = u(&[op(FileOpKind::Move, &from, &to)], &root);
        assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE), "{from}");
    }
    // to에 `..`
    let to_dd = root.join("in/../in/x.pdf");
    let report = u(
        &[op(FileOpKind::Move, &s(&root.join("y.pdf")), &to_dd)],
        &root,
    );
    assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE));
    assert_eq!(read(&to), "x");
    assert!(!escaped.exists());
}

#[test]
fn tampered_relative_fails() {
    let (_t, root) = tmp();
    let to = write(&root.join("x.pdf"), "x");
    let report = u(&[op(FileOpKind::Move, "rel/x.pdf", &to)], &root);
    assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE));

    let mut rel_to = op(FileOpKind::Move, &s(&root.join("y.pdf")), &to);
    rel_to.to = PathBuf::from("x.pdf");
    let report = u(&[rel_to], &root);
    assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE));
    assert_eq!(read(&to), "x");
}

#[tokio::test]
async fn missing_file_id_not_undone() {
    let (_t, root) = tmp();
    let (j, src, at) = moved(&root, "a.pdf").await;
    let mut ops = j.ops();
    ops[0].file_id = None;

    let report = u(&ops, &root);
    assert_eq!(report.skipped[0].1, failed(NO_FILE_ID));
    assert!(!src.exists());
    assert_eq!(read(&at), "a.pdf");
}

#[test]
fn trash_to_must_be_in_trash_dir() {
    let (_t, root) = tmp();
    let trash = root.join("Trash");
    let in_trash = write(&trash.join("a.pdf"), "a");
    let in_other = write(&root.join("other/b.pdf"), "b");
    fs::create_dir_all(root.join("in")).unwrap();
    let roots = AllowedRoots::new([root.join("in")]);
    let from = |n: &str| s(&root.join("in").join(n));

    // 휴지통 밖에서 "휴지통 작업"이라 주장 → 거부
    let report = undo(
        &[op(FileOpKind::Trash, &from("b.pdf"), &in_other)],
        &roots,
        &trash,
    );
    assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE));
    // 휴지통 안 파일인데 이동 작업이라 주장 → 거부
    let report = undo(
        &[op(FileOpKind::Move, &from("a.pdf"), &in_trash)],
        &roots,
        &trash,
    );
    assert_eq!(report.skipped[0].1, failed(OUT_OF_SCOPE));
    assert_eq!(read(&in_trash), "a");
    assert_eq!(read(&in_other), "b");

    // 진짜 휴지통 작업은 된다
    let report = undo(
        &[op(FileOpKind::Trash, &from("a.pdf"), &in_trash)],
        &roots,
        &trash,
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(read(&root.join("in/a.pdf")), "a");
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
    // 옮겨도 같은 파일 → 세 기록 모두 같은 (dev, ino)
    for op in &ops {
        assert_eq!(op.file_id, Some(id_of(&trashed)), "{op:?}");
    }
}

#[test]
fn serializes_camel_case() {
    let op = FileOp {
        kind: FileOpKind::Trash,
        from: "/a".into(),
        to: "/b".into(),
        file_id: Some(FileId { dev: 1, ino: 2 }),
    };
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        json!({"kind": "trash", "from": "/a", "to": "/b", "fileId": {"dev": 1, "ino": 2}})
    );
    // 예전 키 file_id, 키 없음 모두 읽힌다
    let old: FileOp = serde_json::from_value(
        json!({"kind": "trash", "from": "/a", "to": "/b", "file_id": {"dev": 1, "ino": 2}}),
    )
    .unwrap();
    assert_eq!(old, op);
    let none: FileOp =
        serde_json::from_value(json!({"kind": "move", "from": "/a", "to": "/b"})).unwrap();
    assert_eq!(none.file_id, None);

    let report = UndoReport {
        restored: vec![],
        skipped: vec![(op, failed("m"))],
    };
    assert_eq!(
        serde_json::to_value(&report).unwrap()["skipped"][0][1],
        json!({"reason": "failed", "message": "m"})
    );
    assert_eq!(
        serde_json::to_value(UndoSkip::OriginalOccupied).unwrap(),
        json!({"reason": "originalOccupied"})
    );
}
