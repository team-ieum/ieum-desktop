//! `local.file_move`·`local.file_rename`·`local.file_trash`.
//! 영구 삭제 API를 쓰지 않는다. 옮기기는 같은 볼륨 안 rename뿐(복사 없음), 덮어쓰기 없음.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ieum_engine::{NodeCtx, NodeError, NodeRunner};
use serde_json::{Map, Value, json};

use crate::journal::{FileOp, FileOpKind, Journal, RecentWrites};
use crate::paths::{numbered, resolve_in_base, sanitize_name};

/// `name (n).ext`를 시도하는 최대 번호.
const MAX_SUFFIX: u32 = 9999;

fn fail(message: impl Into<String>) -> NodeError {
    NodeError {
        message: message.into(),
    }
}

fn str_cfg<'a>(cfg: &'a Map<String, Value>, key: &str) -> Result<&'a str, NodeError> {
    match cfg.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s),
        _ => Err(fail(format!("'{key}' 칸이 비었거나 글자가 아니에요."))),
    }
}

/// 원본은 존재하는 일반 파일(절대 경로)이어야 한다. 폴더·심볼릭 링크는 안 된다.
fn source_file(cfg: &Map<String, Value>) -> Result<PathBuf, NodeError> {
    let p = PathBuf::from(str_cfg(cfg, "path")?);
    if !p.is_absolute() {
        return Err(fail(format!(
            "파일 경로는 절대 경로여야 해요: {}",
            p.display()
        )));
    }
    match fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_file() => Ok(p),
        Ok(_) => Err(fail(format!(
            "일반 파일이 아니에요(폴더·심볼릭 링크): {}",
            p.display()
        ))),
        Err(e) => Err(fail(format!("파일을 찾을 수 없어요: {}: {e}", p.display()))),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn path_str(p: &Path) -> Value {
    Value::String(p.to_string_lossy().into_owned())
}

/// 대상이 없을 때만 원자적으로 rename. 있으면 `AlreadyExists`.
#[cfg(target_os = "macos")]
fn rename_excl(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::{CString, c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }
    const RENAME_EXCL: c_uint = 0x0000_0004;

    let f = CString::new(from.as_os_str().as_bytes())?;
    let t = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: 두 포인터 모두 호출 동안 살아 있는 NUL 종료 C 문자열이다.
    if unsafe { renamex_np(f.as_ptr(), t.as_ptr(), RENAME_EXCL) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

// ponytail: 확인과 rename 사이 경합 창이 남음 — macOS 외 지원 시 renameat2(RENAME_NOREPLACE)로
#[cfg(not(target_os = "macos"))]
fn rename_excl(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(from, to)
}

/// `dir/name`으로 옮긴다. 같은 이름이 있으면 `name (1).ext`, `name (2).ext` …
fn move_no_clobber(from: &Path, dir: &Path, name: &str) -> Result<PathBuf, NodeError> {
    for n in 0..=MAX_SUFFIX {
        let to = dir.join(numbered(name, n));
        match rename_excl(from, &to) {
            Ok(()) => return Ok(to),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
                return Err(fail(format!(
                    "다른 볼륨으로는 옮길 수 없어요(같은 디스크 안에서만 돼요): {}",
                    dir.display()
                )));
            }
            Err(e) => {
                return Err(fail(format!(
                    "옮기지 못했어요: {} → {}: {e}",
                    from.display(),
                    to.display()
                )));
            }
        }
    }
    Err(fail(format!(
        "같은 이름 파일이 너무 많아요: {}",
        dir.join(name).display()
    )))
}

fn record(journal: &Journal, recent: &RecentWrites, kind: FileOpKind, from: PathBuf, to: &Path) {
    recent.mark(to);
    journal.push(FileOp {
        kind,
        from,
        to: to.to_path_buf(),
    });
}

/// `local.file_move` — 기준 폴더 아래로만 옮긴다.
pub struct FileMoveRunner {
    journal: Journal,
    recent: RecentWrites,
}

impl FileMoveRunner {
    pub fn new(journal: Journal, recent: RecentWrites) -> Self {
        Self { journal, recent }
    }
}

#[async_trait]
impl NodeRunner for FileMoveRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let cfg = &ctx.config;
        let from = source_file(cfg)?;
        let base = PathBuf::from(str_cfg(cfg, "baseFolder")?);
        if !base.is_absolute() || !base.is_dir() {
            return Err(fail(format!(
                "기준 폴더는 이미 있는 절대 경로 폴더여야 해요: {}",
                base.display()
            )));
        }
        let rel = match cfg.get("destination") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(_) => return Err(fail("'destination' 칸은 글자여야 해요.")),
        };
        let dir = resolve_in_base(&base, &rel).map_err(|e| fail(e.to_string()))?;
        fs::create_dir_all(&dir).map_err(|e| {
            fail(format!(
                "목적지 폴더를 만들지 못했어요: {}: {e}",
                dir.display()
            ))
        })?;

        let to = move_no_clobber(&from, &dir, &file_name(&from))?;
        record(&self.journal, &self.recent, FileOpKind::Move, from, &to);
        Ok(json!({"path": path_str(&to), "name": file_name(&to)})
            .as_object()
            .cloned()
            .unwrap_or_default())
    }
}

/// `local.file_rename` — 같은 폴더 안에서 이름만 바꾼다.
pub struct FileRenameRunner {
    journal: Journal,
    recent: RecentWrites,
}

impl FileRenameRunner {
    pub fn new(journal: Journal, recent: RecentWrites) -> Self {
        Self { journal, recent }
    }
}

#[async_trait]
impl NodeRunner for FileRenameRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let cfg = &ctx.config;
        let from = source_file(cfg)?;
        let dir = from.parent().unwrap_or(Path::new("/")).to_path_buf();
        let mut name = sanitize_name(str_cfg(cfg, "newName")?);
        if Path::new(&name).extension().is_none()
            && let Some(ext) = from.extension()
        {
            name = format!("{name}.{}", ext.to_string_lossy());
        }
        let to = if name == file_name(&from) {
            from.clone()
        } else {
            let to = move_no_clobber(&from, &dir, &name)?;
            record(&self.journal, &self.recent, FileOpKind::Rename, from, &to);
            to
        };
        Ok(json!({"path": path_str(&to), "name": file_name(&to)})
            .as_object()
            .cloned()
            .unwrap_or_default())
    }
}

/// `local.file_trash` — 휴지통 폴더로 옮긴다(기본 `~/.Trash`).
// ponytail: Finder "되돌려 놓기"는 안 됨, 되돌리기는 앱 기록으로만 — 필요해지면 NSFileManager trashItem 연동
pub struct FileTrashRunner {
    journal: Journal,
    recent: RecentWrites,
    trash_dir: PathBuf,
}

impl FileTrashRunner {
    pub fn new(journal: Journal, recent: RecentWrites) -> Self {
        let trash_dir = std::env::home_dir().unwrap_or_default().join(".Trash");
        Self::with_trash_dir(journal, recent, trash_dir)
    }

    pub fn with_trash_dir(journal: Journal, recent: RecentWrites, trash_dir: PathBuf) -> Self {
        Self {
            journal,
            recent,
            trash_dir,
        }
    }
}

#[async_trait]
impl NodeRunner for FileTrashRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let from = source_file(&ctx.config)?;
        if !self.trash_dir.is_absolute() || !self.trash_dir.is_dir() {
            return Err(fail(format!(
                "휴지통 폴더를 찾을 수 없어요: {}",
                self.trash_dir.display()
            )));
        }
        let to = move_no_clobber(&from, &self.trash_dir, &file_name(&from))?;
        record(&self.journal, &self.recent, FileOpKind::Trash, from, &to);
        Ok(json!({"path": path_str(&to)})
            .as_object()
            .cloned()
            .unwrap_or_default())
    }
}
