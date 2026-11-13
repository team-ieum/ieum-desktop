//! 파일 노드와 경로 안전. 모든 파일은 임시 폴더 안에서만 만든다(휴지통도 임시 폴더로 주입).

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ieum_engine::{Node, NodeCtx, NodeError, NodeRunner, Workflow};
use ieum_local::{
    AllowedRoots, FileMoveRunner, FileOpKind, FileRenameRunner, FileTrashRunner, Journal,
    RecentWrites, resolve_in_base, roots_for_workflow, sanitize_name,
};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
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

fn roots(r: &Path) -> AllowedRoots {
    AllowedRoots::new([r.to_path_buf()])
}

fn mover(r: &Path) -> (FileMoveRunner, Journal, RecentWrites) {
    let (j, w) = (Journal::default(), RecentWrites::default());
    (FileMoveRunner::new(j.clone(), w.clone(), roots(r)), j, w)
}

fn renamer(r: &Path) -> (FileRenameRunner, Journal) {
    let j = Journal::default();
    (
        FileRenameRunner::new(j.clone(), RecentWrites::default(), roots(r)),
        j,
    )
}

fn name_of(out: &Map<String, Value>) -> &str {
    out["name"].as_str().unwrap()
}

#[test]
fn rejects_escape_paths() {
    let (_t, root) = tmp();
    let base = root.join("base");
    let outside = root.join("outside");
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
    let (_t, root) = tmp();
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    let src = write(&root.join("in/a.pdf"), "a");
    let (m, j, _) = mover(&root);

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
    assert!(!root.join("x").exists());
    assert!(j.ops().is_empty());

    assert!(
        run(
            &m,
            json!({"path": s(&src), "baseFolder": "/nope/ieum-test-missing"})
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn move_rejects_relative_base_folder() {
    let (_t, root) = tmp();
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    let src = write(&root.join("in/a.pdf"), "a");
    let (m, j, _) = mover(&root);
    // 실행 위치와 상관없이 임시 폴더의 base를 가리키는 상대 경로(`..`가 루트에서 멈춘다).
    // 검사가 깨져도 파일은 임시 폴더 안으로만 간다.
    let rel = format!(
        "{}{}",
        "../".repeat(64),
        base.strip_prefix("/").unwrap().display()
    );
    assert!(Path::new(&rel).is_relative() && Path::new(&rel).is_dir());

    assert!(
        run(&m, json!({"path": s(&src), "baseFolder": rel}))
            .await
            .is_err()
    );
    assert!(src.exists());
    assert!(!base.join("a.pdf").exists());
    assert!(j.ops().is_empty());
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
    let (_t, root) = tmp();
    let base = root.join("base");
    let old = write(&base.join("report.pdf"), "old");
    let (m, _, _) = mover(&root);

    let a = write(&root.join("in/report.pdf"), "new1");
    let out = run(
        &m,
        json!({"path": s(&a), "baseFolder": s(&base), "destination": ""}),
    )
    .await
    .unwrap();
    assert_eq!(out["path"], json!(s(&base.join("report (1).pdf"))));
    assert_eq!(out["name"], json!("report (1).pdf"));

    let b = write(&root.join("in/report.pdf"), "new2");
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
async fn move_numbers_hidden_and_extensionless_names() {
    let (_t, root) = tmp();
    let base = root.join("base");
    write(&base.join(".bashrc"), "old");
    write(&base.join("README"), "old");
    let (m, _, _) = mover(&root);

    let a = write(&root.join("in/.bashrc"), "new");
    let out = run(&m, json!({"path": s(&a), "baseFolder": s(&base)}))
        .await
        .unwrap();
    assert_eq!(name_of(&out), ".bashrc (1)");

    let b = write(&root.join("in/README"), "new");
    let out = run(&m, json!({"path": s(&b), "baseFolder": s(&base)}))
        .await
        .unwrap();
    assert_eq!(name_of(&out), "README (1)");
}

#[tokio::test]
async fn move_long_name_collision_truncated() {
    let (_t, root) = tmp();
    let base = root.join("base");
    let long = format!("{}.pdf", "a".repeat(251)); // 정확히 255바이트
    write(&base.join(&long), "old");
    let (m, _, _) = mover(&root);

    let src = write(&root.join("in").join(&long), "new");
    let out = run(&m, json!({"path": s(&src), "baseFolder": s(&base)}))
        .await
        .unwrap();
    let name = name_of(&out);
    assert!(name.len() <= 255, "{}", name.len());
    assert!(name.ends_with(" (1).pdf"), "{name}");
    assert_eq!(fs::read_to_string(base.join(&long)).unwrap(), "old");
}

#[tokio::test]
async fn move_in_place_not_recorded() {
    let (_t, root) = tmp();
    let base = root.join("base");
    let same = write(&base.join("same.pdf"), "x");
    let (m, j, _) = mover(&root);

    let out = run(&m, json!({"path": s(&same), "baseFolder": s(&base)}))
        .await
        .unwrap();
    assert_eq!(out["path"], json!(s(&same)));
    assert_eq!(fs::read_to_string(&same).unwrap(), "x");
    assert!(j.ops().is_empty());
}

#[tokio::test]
async fn move_creates_destination_folder() {
    let (_t, root) = tmp();
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    let src = write(&root.join("in/bill.pdf"), "x");
    let (m, _, _) = mover(&root);

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
    let (_t, root) = tmp();
    let (rn, _) = renamer(&root);

    let a = write(&root.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "청구서"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("청구서.pdf"));
    assert!(root.join("청구서.pdf").exists());

    // 같은 이름이 있으면 번호
    let d = write(&root.join("d.pdf"), "d");
    let out = run(&rn, json!({"path": s(&d), "newName": "청구서"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("청구서 (1).pdf"));
    assert_eq!(fs::read_to_string(root.join("청구서.pdf")).unwrap(), "a");

    // 원본에 확장자가 없으면 결과에도 확장자가 생기지 않는다(`.` → `_`)
    let n = write(&root.join("noext"), "n");
    let out = run(&rn, json!({"path": s(&n), "newName": "v1.2"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("v1_2"));
}

/// 리뷰 B P2: 확장자 없는 원본에 AI가 실행형 확장자(`.command` 등)를 붙이거나 RTLO로 위장하지 못한다.
#[tokio::test]
async fn rename_extensionless_gets_no_extension_and_rejects_bidi() {
    let (_t, root) = tmp();
    let (rn, j) = renamer(&root);
    for (src, new, want) in [
        ("p1", "run.terminal", "run_terminal"),
        ("p2", "open.fileloc", "open_fileloc"),
        ("p3", "a.b.command", "a_b_command"),
    ] {
        let p = write(&root.join(src), "x");
        let out = run(&rn, json!({"path": s(&p), "newName": new}))
            .await
            .unwrap();
        assert_eq!(out["name"], json!(want));
    }
    let n = j.ops().len();
    for (src, new) in [
        ("q1", "invoice\u{202E}fdp.command"),
        ("q2.pdf", "a\u{2067}b"),
        ("q3.pdf", "a\u{200F}b"),
    ] {
        let p = write(&root.join(src), "x");
        let err = run(&rn, json!({"path": s(&p), "newName": new}))
            .await
            .unwrap_err();
        assert!(err.message.contains("방향"), "{}", err.message);
        assert!(p.exists());
    }
    assert_eq!(j.ops().len(), n);
}

#[tokio::test]
async fn rename_dotted_name_keeps_extension() {
    let (_t, root) = tmp();
    let (rn, _) = renamer(&root);
    let a = write(&root.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "2026.11.10 회의록"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("2026.11.10 회의록.pdf"));
}

#[tokio::test]
async fn rename_does_not_change_type() {
    let (_t, root) = tmp();
    let (rn, _) = renamer(&root);
    let a = write(&root.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "report.docx"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("report.docx.pdf"));
}

#[tokio::test]
async fn rename_same_ext_not_doubled() {
    let (_t, root) = tmp();
    let (rn, _) = renamer(&root);
    let a = write(&root.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "report.pdf"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("report.pdf"));
    let b = write(&root.join("b.pdf"), "b");
    let out = run(&rn, json!({"path": s(&b), "newName": "Memo.PDF"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("Memo.PDF"));
}

#[tokio::test]
async fn rename_long_name_truncated_with_extension() {
    let (_t, root) = tmp();
    let (rn, _) = renamer(&root);
    let a = write(&root.join("p1.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "b".repeat(255)}))
        .await
        .unwrap();
    let name = name_of(&out);
    assert!(name.len() <= 255, "{}", name.len());
    assert!(name.ends_with(".pdf"), "{name}");
    assert!(root.join(name).exists());
}

#[tokio::test]
async fn rename_same_name_not_recorded() {
    let (_t, root) = tmp();
    let (rn, j) = renamer(&root);
    let a = write(&root.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "a"}))
        .await
        .unwrap();
    assert_eq!(out["path"], json!(s(&a)));
    assert!(j.ops().is_empty());
}

#[tokio::test]
async fn rename_rejects_separators() {
    let (_t, root) = tmp();
    let dir = root.join("dir");
    let (rn, _) = renamer(&root);

    let a = write(&dir.join("a.pdf"), "a");
    let out = run(&rn, json!({"path": s(&a), "newName": "../evil"}))
        .await
        .unwrap();
    let to = PathBuf::from(out["path"].as_str().unwrap());
    assert_eq!(to.parent().unwrap(), dir);
    assert_eq!(out["name"], json!("_evil.pdf"));
    assert!(!root.join("evil.pdf").exists());

    let b = write(&dir.join("b.pdf"), "b");
    let out = run(&rn, json!({"path": s(&b), "newName": "x/y"}))
        .await
        .unwrap();
    assert_eq!(out["name"], json!("x_y.pdf"));
    assert!(dir.join("x_y.pdf").exists());
}

#[tokio::test]
async fn trash_moves_into_trash_dir() {
    let (_t, root) = tmp();
    let trash = root.join("Trash");
    fs::create_dir_all(&trash).unwrap();
    write(&trash.join("a.pdf"), "earlier");
    let a = write(&root.join("in/a.pdf"), "now");
    let t = FileTrashRunner::with_trash_dir(
        Journal::default(),
        RecentWrites::default(),
        roots(&root.join("in")),
        trash.clone(),
    );

    let out = run(&t, json!({"path": s(&a)})).await.unwrap();
    assert_eq!(out["path"], json!(s(&trash.join("a (1).pdf"))));
    assert!(!a.exists());
    assert_eq!(fs::read_to_string(trash.join("a.pdf")).unwrap(), "earlier");
    assert_eq!(fs::read_to_string(trash.join("a (1).pdf")).unwrap(), "now");
}

#[tokio::test]
async fn journal_records_each_op() {
    let (_t, root) = tmp();
    let base = root.join("base");
    let trash = root.join("Trash");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&trash).unwrap();
    let (j, r) = (Journal::default(), RecentWrites::default());
    let m = FileMoveRunner::new(j.clone(), r.clone(), roots(&root));
    let rn = FileRenameRunner::new(j.clone(), r.clone(), roots(&root));
    let t = FileTrashRunner::with_trash_dir(j.clone(), r.clone(), roots(&root), trash.clone());

    let src = write(&root.join("in/a.pdf"), "a");
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
    assert!(!r.is_recent(&root.join("in/a.pdf"), within));
}

#[tokio::test]
async fn source_must_be_regular_file() {
    let (_t, root) = tmp();
    let base = root.join("base");
    let dir = root.join("folder");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let real = write(&root.join("real.pdf"), "r");
    let link = root.join("link.pdf");
    symlink(&real, &link).unwrap();
    let missing = root.join("missing.pdf");
    // 리뷰 B P4: FIFO도 일반 파일이 아니다
    let fifo = root.join("pipe.pdf");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (m, j, _) = mover(&root);
    let rn = FileRenameRunner::new(j.clone(), RecentWrites::default(), roots(&root));
    let t = FileTrashRunner::with_trash_dir(
        j.clone(),
        RecentWrites::default(),
        roots(&root),
        base.clone(),
    );

    for p in [&dir, &link, &missing, &fifo] {
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

#[tokio::test]
async fn source_outside_allowed_roots_fails() {
    let (_t, root) = tmp();
    let allowed = root.join("allowed");
    let base = allowed.join("base");
    let trash = root.join("Trash");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&trash).unwrap();
    let secret = write(&root.join("private/secret.txt"), "s");
    // 허용 폴더와 이름 앞부분만 같은 형제 폴더도 밖이다
    let sibling = write(&root.join("allowed2/x.txt"), "x");

    let j = Journal::default();
    let w = RecentWrites::default();
    let m = FileMoveRunner::new(j.clone(), w.clone(), roots(&allowed));
    let rn = FileRenameRunner::new(j.clone(), w.clone(), roots(&allowed));
    let t = FileTrashRunner::with_trash_dir(j.clone(), w.clone(), roots(&allowed), trash.clone());

    for p in [&secret, &sibling] {
        assert!(
            run(&m, json!({"path": s(p), "baseFolder": s(&base)}))
                .await
                .is_err()
        );
        assert!(
            run(&rn, json!({"path": s(p), "newName": "x"}))
                .await
                .is_err()
        );
        assert!(run(&t, json!({"path": s(p)})).await.is_err());
    }
    // 허용 폴더가 비어 있으면 전부 거부
    let none =
        FileTrashRunner::with_trash_dir(j.clone(), w.clone(), AllowedRoots::new([]), trash.clone());
    let inside = write(&allowed.join("in.txt"), "i");
    assert!(run(&none, json!({"path": s(&inside)})).await.is_err());

    assert!(secret.exists() && sibling.exists() && inside.exists());
    assert!(fs::read_dir(&trash).unwrap().next().is_none());
    assert!(j.ops().is_empty());
}

#[tokio::test]
async fn source_symlink_into_allowed_root_pointing_outside_fails() {
    let (_t, root) = tmp();
    let allowed = root.join("allowed");
    let trash = root.join("Trash");
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&trash).unwrap();
    let secret = write(&root.join("private/secret.txt"), "s");
    // 허용 폴더 안의 폴더 링크 → 밖
    symlink(root.join("private"), allowed.join("door")).unwrap();
    let via = allowed.join("door/secret.txt");
    assert!(via.exists());

    let j = Journal::default();
    let w = RecentWrites::default();
    let m = FileMoveRunner::new(j.clone(), w.clone(), roots(&allowed));
    let rn = FileRenameRunner::new(j.clone(), w.clone(), roots(&allowed));
    let t = FileTrashRunner::with_trash_dir(j.clone(), w.clone(), roots(&allowed), trash.clone());

    assert!(
        run(&m, json!({"path": s(&via), "baseFolder": s(&allowed)}))
            .await
            .is_err()
    );
    assert!(
        run(&rn, json!({"path": s(&via), "newName": "x"}))
            .await
            .is_err()
    );
    assert!(run(&t, json!({"path": s(&via)})).await.is_err());
    assert_eq!(fs::read_to_string(&secret).unwrap(), "s");
    assert!(j.ops().is_empty());
}

/// 테스트 환경에서 다른 볼륨을 만들 수 없어 기본은 건너뛴다.
/// 확인하려면 다른 볼륨의 쓰기 가능한 빈 폴더를 주고 돌린다:
/// `IEUM_OTHER_VOLUME=/Volumes/X/tmp cargo test -p ieum-local -- --ignored`
#[tokio::test]
#[ignore = "다른 볼륨 필요: IEUM_OTHER_VOLUME"]
async fn cross_volume_fails_without_copy() {
    let other = PathBuf::from(std::env::var("IEUM_OTHER_VOLUME").expect("IEUM_OTHER_VOLUME"));
    let (_t, root) = tmp();
    let src = write(&root.join("a.pdf"), "a");
    let (m, j, _) = mover(&root);

    let err = run(&m, json!({"path": s(&src), "baseFolder": s(&other)}))
        .await
        .unwrap_err();
    assert!(err.message.contains("다른 볼륨"), "{}", err.message);
    assert!(src.exists());
    assert!(!other.join("a.pdf").exists());
    assert!(j.ops().is_empty());
}

/// exFAT 볼륨에서 같은 이름으로 동시에 옮겨도 덮어쓰지 않는다(리뷰 B2 Critical 재현).
/// `IEUM_EXFAT_MOUNT=/Volumes/X cargo test -p ieum-local exfat -- --ignored`
#[test]
#[ignore = "exFAT 볼륨 필요: IEUM_EXFAT_MOUNT"]
fn exfat_concurrent_moves_never_overwrite() {
    use std::sync::{Arc, Barrier};

    let vol = PathBuf::from(std::env::var("IEUM_EXFAT_MOUNT").expect("IEUM_EXFAT_MOUNT"));
    let t = tempfile::tempdir_in(&vol).unwrap();
    let root = t.path().canonicalize().unwrap();
    let base = root.join("base");
    fs::create_dir_all(&base).unwrap();
    let n = 32;
    for round in 0..20 {
        let dest = format!("r{round}");
        let srcs: Vec<PathBuf> = (0..n)
            .map(|i| {
                write(
                    &root.join(format!("in{round}_{i}/same.pdf")),
                    &format!("B{i}"),
                )
            })
            .collect();
        let bar = Arc::new(Barrier::new(n));
        let handles: Vec<_> = srcs
            .into_iter()
            .map(|src| {
                let (base, bar, root, dest) =
                    (base.clone(), bar.clone(), root.clone(), dest.clone());
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .build()
                        .unwrap();
                    let (m, _, _) = mover(&root);
                    bar.wait();
                    rt.block_on(run(
                        &m,
                        json!({"path": s(&src), "baseFolder": s(&base), "destination": dest}),
                    ))
                    .is_err()
                })
            })
            .collect();
        let errs = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|e| *e)
            .count();
        let mut bodies: Vec<String> = fs::read_dir(base.join(&dest))
            .map(|d| {
                d.map(|e| e.unwrap().path())
                    .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with("._"))
                    .map(|p| fs::read_to_string(p).unwrap())
                    .collect()
            })
            .unwrap_or_default();
        let files = bodies.len();
        bodies.sort();
        bodies.dedup();
        assert_eq!(bodies.len(), files, "round {round}: 같은 내용 중복");
        assert_eq!(
            files + errs,
            n,
            "round {round}: 덮어쓰기 손실 {}",
            n - files - errs
        );
    }
}

#[test]
fn roots_for_workflow_collects_folders() {
    let dir = TempDir::new().unwrap();
    let [inbox, sorted, other] = ["in", "out", "x"].map(|d| dir.path().join(d));
    for d in [&inbox, &sorted, &other] {
        fs::create_dir(d).unwrap();
    }
    let wf: Workflow = serde_json::from_value(json!({"nodes": [
        {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": inbox}},
        {"id": "m", "type": "ACTION", "event": "local.file_move",
         "config": {"path": "{{nodes.t.output.path}}", "baseFolder": sorted, "destination": "a"}},
        {"id": "r", "type": "ACTION", "event": "local.file_rename", "config": {"path": other, "newName": "b"}},
        {"id": "g", "type": "ACTION", "event": "local.file_move", "config": {"baseFolder": dir.path().join("없음")}}
    ]}))
    .unwrap();
    // 트리거 폴더 + file_move 기준 폴더만, 없는 폴더는 버린다
    assert_eq!(roots_for_workflow(&wf), AllowedRoots::new([inbox, sorted]));
}

#[test]
fn relative_folder_never_becomes_root() {
    // 상대 경로는 cwd 기준으로 풀린다 — Finder에서 띄운 앱은 cwd가 `/`라 허용 폴더가 디스크 전체가 된다.
    for folder in [
        json!("."),
        json!("Users/me"),
        json!(""),
        json!(3),
        json!("{{nodes.x.output.path}}"),
    ] {
        let wf: Workflow = serde_json::from_value(json!({"nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": folder}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"baseFolder": folder}}
        ]}))
        .unwrap();
        assert_eq!(roots_for_workflow(&wf), AllowedRoots::default(), "{folder}");
    }
    // 공유 지점에서 막는다 — 다른 호출자가 상대 경로를 넘겨도 버린다
    assert_eq!(
        AllowedRoots::new([PathBuf::from("."), PathBuf::from("..")]),
        AllowedRoots::default()
    );
}

#[tokio::test]
async fn file_errors_carry_safe_codes() {
    let (_t, root) = tmp();
    let (_o, outside) = tmp();
    let (watch, base) = (root.join("watch"), root.join("base"));
    write(&watch.join("a.txt"), "a");
    fs::create_dir_all(&base).unwrap();
    write(&outside.join("secret.txt"), "s");
    // (config, 기대 코드)
    let cases = [
        (json!({"path": "", "baseFolder": base}), "FIELD_EMPTY path"),
        (
            json!({"path": watch.join("없는파일.txt"), "baseFolder": base}),
            "FILE_MISSING",
        ),
        (
            json!({"path": outside.join("secret.txt"), "baseFolder": base}),
            "FOLDER_NOT_ALLOWED",
        ),
        (
            json!({"path": watch.join("a.txt"), "baseFolder": base.join("없는폴더")}),
            "FOLDER_MISSING",
        ),
        (
            json!({"path": watch.join("a.txt"), "baseFolder": base, "destination": "../밖"}),
            "FOLDER_NOT_ALLOWED",
        ),
        (
            json!({"path": watch.join("a.txt"), "baseFolder": base, "destination": true}),
            "FIELD_INVALID destination",
        ),
    ];
    for (config, want) in cases {
        let e = run(&mover(&root).0, config).await.unwrap_err();
        assert_eq!(e.code.as_deref(), Some(want));
        let code = e.code.unwrap();
        // 코드에 경로·이름 없음
        assert!(
            !code.contains('/') && !code.contains("없는") && !code.contains("secret"),
            "{code}"
        );
    }
}
