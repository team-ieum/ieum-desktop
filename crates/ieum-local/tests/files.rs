//! 파일 노드와 경로 안전. 모든 파일은 임시 폴더 안에서만 만든다(휴지통도 임시 폴더로 주입).

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ieum_engine::{Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::{
    FileMoveRunner, FileOpKind, FileRenameRunner, FileTrashRunner, Journal, RecentWrites,
    resolve_in_base, sanitize_name,
};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

async fn run(runner: &dyn NodeRunner, config: Value) -> Result<Map<String, Value>, NodeError> {
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
}

fn write(p: &Path, body: &str) -> PathBuf {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
    p.to_path_buf()
}

fn s(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

fn mover() -> (FileMoveRunner, Journal, RecentWrites) {
    let (j, r) = (Journal::default(), RecentWrites::default());
    (FileMoveRunner::new(j.clone(), r.clone()), j, r)
}

#[test]
fn rejects_escape_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, base.join("link")).unwrap();

    assert!(resolve_in_base(&base, "../x").is_err());
    assert!(resolve_in_base(&base, "a/../../x").is_err());
    assert!(resolve_in_base(&base, "/etc").is_err());
    assert!(resolve_in_base(&base, "link").is_err());
    assert!(resolve_in_base(&base, "link/sub").is_err());

    assert_eq!(resolve_in_base(&base, "").unwrap(), base);
    assert_eq!(
        resolve_in_base(&base, "a/./b").unwrap(),
        base.join("a").join("b")
    );
    // 금지 문자는 칸마다 치환
    assert_eq!(resolve_in_base(&base, "a:b").unwrap(), base.join("a_b"));
}

#[tokio::test]
async fn move_rejects_escape_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    fs::create_dir_all(&base).unwrap();
    let src = write(&tmp.path().join("in/a.pdf"), "a");
    let (m, j, _) = mover();

    for dest in ["../x", "/etc", "a/../../x"] {
        let err = run(
            &m,
            json!({"path": s(&src), "baseFolder": s(&base), "destination": dest}),
        )
        .await
        .unwrap_err();
        assert!(!err.message.is_empty());
    }
    assert!(src.exists());
    assert!(!tmp.path().join("x").exists());
    assert!(j.ops().is_empty());

    // 기준 폴더가 상대 경로거나 없으면 실패
    for b in ["base", "/nope/ieum-test-missing"] {
        assert!(
            run(&m, json!({"path": s(&src), "baseFolder": b}))
                .await
                .is_err()
        );
    }
}

#[test]
fn sanitizes_names() {
    assert_eq!(sanitize_name("a:b/c"), "a_b_c");
    assert_eq!(sanitize_name("a\0b\nc"), "a_b_c");
    assert_eq!(sanitize_name("  ..hidden. "), "hidden");
    assert_eq!(sanitize_name(""), "untitled");
    assert_eq!(sanitize_name(" . "), "untitled");

    let long = format!("{}.pdf", "가".repeat(100)); // 300 + 4 바이트
    let out = sanitize_name(&long);
    assert!(out.len() <= 255, "{}", out.len());
    assert!(out.ends_with(".pdf"));
    assert!(out.starts_with('가'));

    let no_ext = "b".repeat(300);
    assert_eq!(sanitize_name(&no_ext).len(), 255);
}

#[tokio::test]
async fn move_never_overwrites() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    let old = write(&base.join("report.pdf"), "old");
    let (m, _, _) = mover();

    let a = write(&tmp.path().join("in/report.pdf"), "new1");
    let out = run(
        &m,
        json!({"path": s(&a), "baseFolder": s(&base), "destination": ""}),
    )
    .await
    .unwrap();
    assert_eq!(out["path"], json!(s(&base.join("report (1).pdf"))));
    assert_eq!(out["name"], json!("report (1).pdf"));

    let b = write(&tmp.path().join("in/report.pdf"), "new2");
    let out = run(&m, json!({"path": s(&b), "baseFolder": s(&base)}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("report (2).pdf"));

    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
    assert_eq!(
        fs::read_to_string(base.join("report (1).pdf")).unwrap(),
        "new1"
    );
    assert_eq!(
        fs::read_to_string(base.join("report (2).pdf")).unwrap(),
        "new2"
    );
    assert!(!a.exists());
}

#[tokio::test]
async fn move_creates_destination_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    fs::create_dir_all(&base).unwrap();
    let src = write(&tmp.path().join("in/bill.pdf"), "x");
    let (m, _, _) = mover();

    let out = run(
        &m,
        json!({"path": s(&src), "baseFolder": s(&base), "destination": "청구서/2026"}),
    )
    .await
    .unwrap();
    let to = base.join("청구서/2026/bill.pdf");
    assert_eq!(out["path"], json!(s(&to)));
    assert_eq!(fs::read_to_string(&to).unwrap(), "x");
    assert!(!src.exists());
}

#[tokio::test]
async fn rename_keeps_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let (j, r) = (Journal::default(), RecentWrites::default());
    let rn = FileRenameRunner::new(j, r);

    let a = write(&tmp.path().join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "청구서"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("청구서.pdf"));
    assert!(tmp.path().join("청구서.pdf").exists());

    let b = write(&tmp.path().join("b.pdf"), "b");
    let out = run(&rn, json!({"path": s(&b), "newName": "c.txt"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("c.txt"));

    // 같은 이름이 있으면 번호
    let d = write(&tmp.path().join("d.pdf"), "d");
    let out = run(&rn, json!({"path": s(&d), "newName": "청구서"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("청구서 (1).pdf"));
    assert_eq!(
        fs::read_to_string(tmp.path().join("청구서.pdf")).unwrap(),
        "a"
    );
}

#[tokio::test]
async fn rename_rejects_separators() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("dir");
    let rn = FileRenameRunner::new(Journal::default(), RecentWrites::default());

    let a = write(&dir.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "../evil"}))
        .await
        .unwrap();
    let to = PathBuf::from(out["path"].as_str().unwrap());
    assert_eq!(to.parent().unwrap(), dir);
    assert_eq!(out["name"], json!("_evil.pdf"));
    assert!(!tmp.path().join("evil.pdf").exists());

    let b = write(&dir.join("b.pdf"), "b");
    let out = run(&rn, json!({"path": s(&b), "newName": "x/y"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("x_y.pdf"));
    assert!(dir.join("x_y.pdf").exists());
}

#[tokio::test]
async fn trash_moves_into_trash_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let trash = tmp.path().join("Trash");
    fs::create_dir_all(&trash).unwrap();
    write(&trash.join("a.pdf"), "earlier");
    let t =
        FileTrashRunner::with_trash_dir(Journal::default(), RecentWrites::default(), trash.clone());

    let a = write(&tmp.path().join("in/a.pdf"), "now");
    let out = run(&t, json!({"path": s(&a)})).await.unwrap();
    assert_eq!(out["path"], json!(s(&trash.join("a (1).pdf"))));
    assert!(!a.exists());
    assert_eq!(fs::read_to_string(trash.join("a.pdf")).unwrap(), "earlier");
    assert_eq!(fs::read_to_string(trash.join("a (1).pdf")).unwrap(), "now");
}

#[tokio::test]
async fn journal_records_each_op() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    let trash = tmp.path().join("Trash");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&trash).unwrap();
    let (j, r) = (Journal::default(), RecentWrites::default());
    let m = FileMoveRunner::new(j.clone(), r.clone());
    let rn = FileRenameRunner::new(j.clone(), r.clone());
    let t = FileTrashRunner::with_trash_dir(j.clone(), r.clone(), trash.clone());

    let src = write(&tmp.path().join("in/a.pdf"), "a");
    let moved = base.join("docs/a.pdf");
    let renamed = base.join("docs/b.pdf");
    let trashed = trash.join("b.pdf");
    run(
        &m,
        json!({"path": s(&src), "baseFolder": s(&base), "destination": "docs"}),
    )
    .await
    .unwrap();
    run(&rn, json!({"path": s(&moved), "newName": "b"}))
        .await
        .unwrap();
    run(&t, json!({"path": s(&renamed)})).await.unwrap();
    // 실패한 작업은 기록하지 않는다
    assert!(run(&t, json!({"path": s(&renamed)})).await.is_err());

    let ops = j.ops();
    assert_eq!(ops.len(), 3);
    assert_eq!(
        ops.iter()
            .map(|o| (o.kind, o.from.clone(), o.to.clone()))
            .collect::<Vec<_>>(),
        vec![
            (FileOpKind::Move, src, moved.clone()),
            (FileOpKind::Rename, moved, renamed.clone()),
            (FileOpKind::Trash, renamed, trashed.clone()),
        ]
    );
    let v = serde_json::to_value(&ops[0]).unwrap();
    assert_eq!(v["kind"], json!("move"));

    let within = Duration::from_secs(60);
    assert!(r.is_recent(&base.join("docs/b.pdf"), within));
    assert!(r.is_recent(&trashed, within));
    assert!(!r.is_recent(&tmp.path().join("in/a.pdf"), within));
}

#[tokio::test]
async fn source_must_be_regular_file() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    let dir = tmp.path().join("folder");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let real = write(&tmp.path().join("real.pdf"), "r");
    let link = tmp.path().join("link.pdf");
    symlink(&real, &link).unwrap();
    let missing = tmp.path().join("missing.pdf");
    let (m, j, _) = mover();
    let rn = FileRenameRunner::new(j.clone(), RecentWrites::default());
    let t = FileTrashRunner::with_trash_dir(j.clone(), RecentWrites::default(), base.clone());

    for p in [&dir, &link, &missing] {
        assert!(
            run(&m, json!({"path": s(p), "baseFolder": s(&base)}))
                .await
                .is_err()
        );
        assert!(
            run(&rn, json!({"path": s(p), "newName": "z"}))
                .await
                .is_err()
        );
        assert!(run(&t, json!({"path": s(p)})).await.is_err());
    }
    assert!(
        run(&m, json!({"path": "real.pdf", "baseFolder": s(&base)}))
            .await
            .is_err()
    );
    assert!(dir.is_dir() && real.exists() && link.symlink_metadata().is_ok());
    assert!(j.ops().is_empty());
}

/// 테스트 환경에서 다른 볼륨을 만들 수 없어 기본은 건너뛴다.
/// 확인하려면 다른 볼륨의 쓰기 가능한 빈 폴더를 주고 돌린다:
/// `IEUM_OTHER_VOLUME=/Volumes/X/tmp cargo test -p ieum-local -- --ignored`
#[tokio::test]
#[ignore = "다른 볼륨 필요: IEUM_OTHER_VOLUME"]
async fn cross_volume_fails_without_copy() {
    let other = PathBuf::from(std::env::var("IEUM_OTHER_VOLUME").expect("IEUM_OTHER_VOLUME"));
    let tmp = tempfile::tempdir().unwrap();
    let src = write(&tmp.path().join("a.pdf"), "a");
    let (m, j, _) = mover();

    let err = run(&m, json!({"path": s(&src), "baseFolder": s(&other)}))
        .await
        .unwrap_err();
    assert!(err.message.contains("다른 볼륨"), "{}", err.message);
    assert!(src.exists());
    assert!(!other.join("a.pdf").exists());
    assert!(j.ops().is_empty());
}
