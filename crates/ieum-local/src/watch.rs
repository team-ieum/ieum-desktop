//! `local.folder_new_file` 트리거: 감시 폴더 바로 아래에 새로 생긴 파일을 알린다.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::{Duration, Instant};

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::journal::RecentWrites;

/// 트리거 출력과 같은 키(`path`·`name`·`extension`·`size`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewFile {
    pub path: PathBuf,
    pub name: String,
    pub extension: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct WatchTiming {
    /// 크기 확인 간격. 두 번 연속 같으면 보낸다.
    pub stable_interval: Duration,
    /// 같은 경로를 다시 보내지 않는 시간.
    pub repeat_window: Duration,
    /// `RecentWrites`에 이 시간 안에 있으면 앱이 쓴 파일로 보고 무시.
    pub recent_window: Duration,
}

impl Default for WatchTiming {
    fn default() -> Self {
        Self {
            stable_interval: Duration::from_secs(1),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("감시할 폴더를 열 수 없어요: {0}")]
    Folder(#[from] std::io::Error),
    #[error("폴더 감시를 시작할 수 없어요: {0}")]
    Notify(#[from] notify::Error),
}

/// drop하면 감시가 멈춘다(알림 콜백의 송신자가 사라져 작업 스레드도 끝남).
pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
}

impl FolderWatcher {
    pub fn start(
        folder: PathBuf,
        recent: RecentWrites,
        tx: mpsc::UnboundedSender<NewFile>,
    ) -> Result<Self, WatchError> {
        Self::start_with(folder, recent, tx, WatchTiming::default())
    }

    pub fn start_with(
        folder: PathBuf,
        recent: RecentWrites,
        tx: mpsc::UnboundedSender<NewFile>,
        timing: WatchTiming,
    ) -> Result<Self, WatchError> {
        // 이벤트 경로는 실제 경로(/var → /private/var)로 온다.
        let real = folder.canonicalize()?;
        if !real.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).into());
        }

        let (names_tx, names_rx) = std_mpsc::channel::<OsString>();
        let (real_cb, folder_cb) = (real.clone(), folder.clone());
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                let ev = match res {
                    Ok(ev) => ev,
                    Err(e) => return eprintln!("[ieum-local] 폴더 감시 오류: {e}"),
                };
                // 시작 뒤 생긴 파일만: 생성·이름 변경 이벤트만 본다(시작 시 스캔 없음).
                if !matches!(
                    ev.kind,
                    EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
                ) {
                    return;
                }
                for p in ev.paths {
                    let direct = p.parent().is_some_and(|d| d == real_cb || d == folder_cb);
                    if let (true, Some(name)) = (direct, p.file_name()) {
                        let _ = names_tx.send(name.to_os_string());
                    }
                }
            })?;
        watcher.watch(&real, RecursiveMode::NonRecursive)?;

        thread::spawn(move || run(folder, recent, tx, timing, names_rx));
        Ok(Self { _watcher: watcher })
    }
}

fn ignored(name: &str) -> bool {
    if name.starts_with('.') || name == "Icon\r" {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            ["crdownload", "download", "part"]
                .iter()
                .any(|t| e.eq_ignore_ascii_case(t))
        })
}

fn run(
    folder: PathBuf,
    recent: RecentWrites,
    tx: mpsc::UnboundedSender<NewFile>,
    timing: WatchTiming,
    names: std_mpsc::Receiver<OsString>,
) {
    // 이름 → 직전 확인 때 크기(아직 확인 전이면 None)
    let mut pending: HashMap<OsString, Option<u64>> = HashMap::new();
    let mut sent: HashMap<OsString, Instant> = HashMap::new();
    let mut next_check = Instant::now() + timing.stable_interval;

    loop {
        let wait = next_check.saturating_duration_since(Instant::now());
        match names.recv_timeout(wait) {
            Ok(name) => {
                if name.to_str().is_some_and(|n| !ignored(n)) {
                    pending.entry(name).or_insert(None);
                }
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
        }
        // 이벤트가 쉬지 않고 와도 확인은 제때 한다.
        if Instant::now() < next_check {
            continue;
        }
        if tx.is_closed() {
            return;
        }
        next_check = Instant::now() + timing.stable_interval;
        sent.retain(|_, t| t.elapsed() < timing.repeat_window);

        pending.retain(|name, last| {
            let path = folder.join(name);
            // 사라졌거나 일반 파일이 아니면(폴더·심볼릭 링크) 버린다.
            let Ok(meta) = path.symlink_metadata() else {
                return false;
            };
            if !meta.is_file() {
                return false;
            }
            if *last != Some(meta.len()) {
                *last = Some(meta.len());
                return true;
            }
            // 크기 안정. 파일 노드는 rename 뒤에 mark하므로 보내기 직전에 확인한다.
            let ours = recent.is_recent(&path, timing.recent_window)
                || path
                    .canonicalize()
                    .is_ok_and(|real| recent.is_recent(&real, timing.recent_window));
            if !ours && !sent.contains_key(name) {
                sent.insert(name.clone(), Instant::now());
                let name_str = name.to_string_lossy().into_owned();
                let extension = Path::new(name)
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let _ = tx.send(NewFile {
                    path: path.clone(),
                    name: name_str,
                    extension,
                    size: meta.len(),
                });
            }
            false
        });
    }
}
