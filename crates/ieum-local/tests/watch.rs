//! 폴더 감시 트리거. 모든 파일은 임시 폴더 안에서만 만든다.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use ieum_local::{
    AllowedRoots, FileId, FileOp, FileOpKind, FolderWatcher, NewFile, RecentWrites, WatchTiming,
    undo,
};
use tempfile::TempDir;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

const TICK: Duration = Duration::from_millis(100);
/// 크기 안정(2틱) + FSEvents 지연을 넉넉히 덮는 대기 시간.
const WAIT: Duration = Duration::from_millis(1500);

fn timing() -> WatchTiming {
    WatchTiming {
        stable_interval: TICK,
        repeat_window: Duration::from_secs(30),
        recent_window: Duration::from_secs(10),
    }
}

fn start(dir: &Path, recent: RecentWrites) -> (FolderWatcher, UnboundedReceiver<NewFile>) {
    start_t(dir, recent, timing())
}

fn start_t(
    dir: &Path,
    recent: RecentWrites,
    t: WatchTiming,
) -> (FolderWatcher, UnboundedReceiver<NewFile>) {
    let (tx, rx) = unbounded_channel();
    let w = FolderWatcher::start_with(dir.to_path_buf(), recent, tx, t).unwrap();
    sleep(TICK); // 감시 스트림이 자리 잡을 시간
    (w, rx)
}

/// 최대 `wait`까지 받은 이벤트를 모두 모은다.
fn collect(rx: &mut UnboundedReceiver<NewFile>, wait: Duration) -> Vec<NewFile> {
    let end = Instant::now() + wait;
    let mut got = Vec::new();
    while Instant::now() < end {
        while let Ok(f) = rx.try_recv() {
            got.push(f);
        }
        sleep(Duration::from_millis(20));
    }
    got
}

/// 첫 이벤트를 최대 `wait`까지 기다린다.
fn first(rx: &mut UnboundedReceiver<NewFile>, wait: Duration) -> Option<NewFile> {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if let Ok(f) = rx.try_recv() {
            return Some(f);
        }
        sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn fires_for_new_file() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    fs::write(dir.path().join("report.PDF"), b"hello").unwrap();

    let got = collect(&mut rx, WAIT);
    assert_eq!(got.len(), 1, "{got:?}");
    let f = &got[0];
    assert_eq!(f.name, "report.PDF");
    assert_eq!(f.extension, "PDF");
    assert_eq!(f.size, 5);
    assert_eq!(f.path.file_name().unwrap(), "report.PDF");
    assert!(f.path.exists());

    let json = serde_json::to_value(f).unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["extension", "name", "path", "size"]);
}

#[test]
fn ignores_hidden_and_temp() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    for name in [
        ".hidden",
        ".DS_Store",
        "Icon\r",
        "a.crdownload",
        "b.DOWNLOAD",
        "c.Part",
    ] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    let got = collect(&mut rx, WAIT);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn fires_once_after_rename_from_temp() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    let tmp = dir.path().join("a.pdf.crdownload");
    let mut f = fs::File::create(&tmp).unwrap();
    for _ in 0..5 {
        f.write_all(&[0u8; 1000]).unwrap();
        f.flush().unwrap();
        sleep(TICK / 2);
    }
    drop(f);
    fs::rename(&tmp, dir.path().join("a.pdf")).unwrap();

    let got = collect(&mut rx, WAIT);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].name, "a.pdf");
    assert_eq!(got[0].size, 5000);
}

#[test]
fn waits_until_size_stable() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    let path = dir.path().join("big.bin");
    let mut f = fs::File::create(&path).unwrap();
    for _ in 0..10 {
        f.write_all(&[0u8; 100]).unwrap();
        f.flush().unwrap();
        sleep(TICK / 2);
        assert!(rx.try_recv().is_err(), "커지는 중인 파일을 보냈다");
    }
    drop(f);

    let got = collect(&mut rx, WAIT);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].size, 1000);
}

#[test]
fn ignores_paths_written_by_ieum() {
    let dir = TempDir::new().unwrap();
    let recent = RecentWrites::default();
    let (_w, mut rx) = start(dir.path(), recent.clone());

    // 파일 노드처럼: 먼저 옮기고(rename) 조금 뒤 실제 경로(/private/var)로 mark 한다.
    // 감시는 비정규 경로(/var)로 시작 → 정규 경로 확인과 "보내기 직전 확인"을 함께 고정.
    let src = TempDir::new().unwrap();
    let from = src.path().join("moved.pdf");
    fs::write(&from, b"data").unwrap();
    let to = dir.path().join("moved.pdf");
    fs::rename(&from, &to).unwrap();
    sleep(TICK / 2);
    recent.mark(&to.canonicalize().unwrap());

    // 같은 폴더에 사용자가 만든 파일은 보낸다.
    fs::write(dir.path().join("user.pdf"), b"u").unwrap();

    let got = collect(&mut rx, WAIT);
    let names: Vec<_> = got.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["user.pdf"]);
}

#[test]
fn ignores_subfolders_and_symlinks() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("t.txt");
    fs::write(&target, b"t").unwrap();

    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("inner.txt"), b"x").unwrap();
    symlink(&target, dir.path().join("link.txt")).unwrap();

    let got = collect(&mut rx, WAIT);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn existing_files_not_sent_at_start() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.pdf"), b"old").unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());

    let got = collect(&mut rx, WAIT);
    assert!(got.is_empty(), "{got:?}");

    // 시작 뒤 새 파일은 보낸다(감시는 살아 있음).
    fs::write(dir.path().join("new.pdf"), b"new").unwrap();
    assert_eq!(first(&mut rx, WAIT).unwrap().name, "new.pdf");
}

#[test]
fn file_vanishing_before_stable_not_sent() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    let path = dir.path().join("gone.pdf");
    fs::write(&path, b"x").unwrap();
    fs::remove_file(&path).unwrap();

    let got = collect(&mut rx, WAIT);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn same_path_not_sent_again_within_window() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    let path = dir.path().join("again.pdf");
    fs::write(&path, b"1").unwrap();
    assert!(first(&mut rx, WAIT).is_some());

    fs::remove_file(&path).unwrap();
    fs::write(&path, b"22").unwrap();
    let got = collect(&mut rx, WAIT);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn same_path_sent_again_after_window() {
    let dir = TempDir::new().unwrap();
    let t = WatchTiming {
        repeat_window: Duration::from_millis(300),
        ..timing()
    };
    let (_w, mut rx) = start_t(dir.path(), RecentWrites::default(), t);
    let path = dir.path().join("again.pdf");
    fs::write(&path, b"1").unwrap();
    assert!(first(&mut rx, WAIT).is_some());

    sleep(Duration::from_millis(500));
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"22").unwrap();
    let got = collect(&mut rx, WAIT);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].size, 2);
}

#[test]
fn firefox_placeholder_waits_for_part() {
    let dir = TempDir::new().unwrap();
    let (_w, mut rx) = start(dir.path(), RecentWrites::default());
    let path = dir.path().join("foo.pdf");
    fs::write(&path, b"").unwrap();
    let part = dir.path().join("foo.pdf.part");
    let mut f = fs::File::create(&part).unwrap();
    for _ in 0..5 {
        f.write_all(&[0u8; 1000]).unwrap();
        f.flush().unwrap();
        sleep(TICK * 3); // 자리 파일(0B)은 그동안 크기 안정
        assert!(rx.try_recv().is_err(), "자리 파일을 보냈다");
    }
    drop(f);
    fs::rename(&part, &path).unwrap();

    let got = collect(&mut rx, WAIT);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].name, "foo.pdf");
    assert_eq!(got[0].size, 5000);
}

#[test]
fn undone_file_not_retriggered() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let real = dir.path().canonicalize().unwrap();
    let out_real = outside.path().canonicalize().unwrap();
    // 감시 폴더에서 밖으로 옮겨진 기록(파일 노드가 남긴 것과 같은 모양)
    let to = out_real.join("a.pdf");
    fs::write(&to, b"data").unwrap();
    let op = FileOp {
        kind: FileOpKind::Move,
        from: real.join("a.pdf"),
        to: to.clone(),
        file_id: Some(FileId {
            dev: fs::metadata(&to).unwrap().dev(),
            ino: fs::metadata(&to).unwrap().ino(),
        }),
    };

    let recent = RecentWrites::default();
    let (_w, mut rx) = start(dir.path(), recent.clone());
    let report = undo(
        &[op],
        &AllowedRoots::new([real.clone(), out_real]),
        &real.join("Trash"),
        &recent,
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(real.join("a.pdf").exists());

    let got = collect(&mut rx, WAIT);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn start_fails_for_missing_folder() {
    let dir = TempDir::new().unwrap();
    let (tx, _rx) = unbounded_channel();
    assert!(FolderWatcher::start(dir.path().join("nope"), RecentWrites::default(), tx).is_err());
}
