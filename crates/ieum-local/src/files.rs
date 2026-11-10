//! `local.file_move`·`local.file_rename`·`local.file_trash`.
//! 영구 삭제 API를 쓰지 않는다. 옮기기는 같은 볼륨 안 rename뿐(복사 없음), 덮어쓰기 없음.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use ieum_engine::{NodeCtx, NodeError, NodeRunner};
use serde_json::{Map, Value, json};

use crate::journal::{FileId, FileOp, FileOpKind, Journal, RecentWrites};
use crate::paths::{AllowedRoots, numbered, resolve_in_base, sanitize_name, truncate_keep_ext};

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

/// 원본은 존재하는 일반 파일(절대 경로)이고, 실제 경로가 허용 폴더 안이어야 한다.
/// 폴더·심볼릭 링크는 안 된다. 실제 경로(canonicalize)를 돌려준다.
fn source_file(cfg: &Map<String, Value>, roots: &AllowedRoots) -> Result<PathBuf, NodeError> {
    let p = PathBuf::from(str_cfg(cfg, "path")?);
    if !p.is_absolute() {
        return Err(fail(format!(
            "파일 경로는 절대 경로여야 해요: {}",
            p.display()
        )));
    }
    match fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_file() => {}
        Ok(_) => {
            return Err(fail(format!(
                "일반 파일이 아니에요(폴더·심볼릭 링크): {}",
                p.display()
            )));
        }
        Err(e) => return Err(fail(format!("파일을 찾을 수 없어요: {}: {e}", p.display()))),
    }
    let real = p
        .canonicalize()
        .map_err(|e| fail(format!("파일을 찾을 수 없어요: {}: {e}", p.display())))?;
    if !roots.contains(&real) {
        return Err(fail(format!(
            "이 워크플로우가 다룰 수 있는 폴더 밖의 파일이에요: {}",
            real.display()
        )));
    }
    Ok(real)
}

/// 이미 있는 절대 경로 폴더의 실제 경로.
fn real_dir(p: &Path, what: &str) -> Result<PathBuf, NodeError> {
    let err = || {
        fail(format!(
            "{what}는 이미 있는 절대 경로 폴더여야 해요: {}",
            p.display()
        ))
    };
    if !p.is_absolute() {
        return Err(err());
    }
    let real = p.canonicalize().map_err(|_| err())?;
    if !real.is_dir() {
        return Err(err());
    }
    Ok(real)
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
/// 경로 어디에든 심볼릭 링크가 있으면 거부한다(검사 뒤 링크로 바뀌는 경합 차단).
#[cfg(target_os = "macos")]
pub(crate) fn rename_excl(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::{CString, c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }
    // SDK sys/stdio.h
    const RENAME_EXCL: c_uint = 0x0000_0004;
    const RENAME_NOFOLLOW_ANY: c_uint = 0x0000_0010;
    const ENOTSUP: i32 = 45;

    let f = CString::new(from.as_os_str().as_bytes())?;
    let t = CString::new(to.as_os_str().as_bytes())?;
    let call = |flags: c_uint| {
        // SAFETY: 두 포인터 모두 호출 동안 살아 있는 NUL 종료 C 문자열이다.
        if unsafe { renamex_np(f.as_ptr(), t.as_ptr(), flags) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    };
    let unsupported = |e: &io::Error| e.raw_os_error() == Some(ENOTSUP);
    match call(RENAME_EXCL | RENAME_NOFOLLOW_ANY) {
        // NOFOLLOW_ANY를 못 받는 볼륨: 원자적 "없을 때만"은 지키고 링크 차단만 뺀다.
        Err(e) if unsupported(&e) => match call(RENAME_EXCL) {
            Err(e) if unsupported(&e) => Err(no_excl()),
            r => r,
        },
        r => r,
    }
}

/// 1단계는 macOS만 지원 — 원자적 "없을 때만" rename이 없으면 옮기지 않는다.
#[cfg(not(target_os = "macos"))]
pub(crate) fn rename_excl(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(no_excl())
}

/// 덮어쓰기 없는 rename을 못 하는 볼륨·플랫폼. 확인 후 `fs::rename`은 경합에서 덮어쓰므로 쓰지 않는다.
fn no_excl() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "RENAME_EXCL 미지원")
}

/// `dir/name`으로 옮긴다. 같은 이름이 있으면 `name (1).ext`, `name (2).ext` …
/// 이미 그 자리면 아무것도 하지 않고 `from`을 돌려준다.
fn move_no_clobber(from: &Path, dir: &Path, name: &str) -> Result<PathBuf, NodeError> {
    if dir.join(name) == from {
        return Ok(from.to_path_buf());
    }
    for n in 0..=MAX_SUFFIX {
        let to = dir.join(numbered(name, n));
        match rename_excl(from, &to) {
            Ok(()) => return Ok(to),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) if e.kind() == io::ErrorKind::Unsupported => {
                return Err(fail("이 디스크에서는 덮어쓰기 없이 옮길 수 없어요."));
            }
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

/// 실제로 옮겨졌을 때만 기록한다.
fn record(journal: &Journal, recent: &RecentWrites, kind: FileOpKind, from: PathBuf, to: &Path) {
    if from == to {
        return;
    }
    recent.mark(to);
    journal.push(FileOp {
        kind,
        from,
        to: to.to_path_buf(),
        file_id: FileId::of(to).ok(),
    });
}

fn output(to: &Path, with_name: bool) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("path".into(), path_str(to));
    if with_name {
        out.insert("name".into(), json!(file_name(to)));
    }
    out
}

/// `local.file_move` — 기준 폴더 아래로만 옮긴다.
pub struct FileMoveRunner {
    journal: Journal,
    recent: RecentWrites,
    roots: AllowedRoots,
}

impl FileMoveRunner {
    pub fn new(journal: Journal, recent: RecentWrites, roots: AllowedRoots) -> Self {
        Self {
            journal,
            recent,
            roots,
        }
    }
}

#[async_trait]
impl NodeRunner for FileMoveRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let cfg = &ctx.config;
        let from = source_file(cfg, &self.roots)?;
        let base = real_dir(Path::new(str_cfg(cfg, "baseFolder")?), "기준 폴더")?;
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
        Ok(output(&to, true))
    }
}

/// 원래 확장자를 항상 유지한다. `newName`이 이미 `.<원래 확장자>`로 끝나면(대소문자 무시) 그대로.
fn renamed(from: &Path, new_name: &str) -> String {
    let name = sanitize_name(new_name);
    match from.extension() {
        Some(ext) => {
            let dot_ext = format!(".{}", ext.to_string_lossy());
            if name.to_lowercase().ends_with(&dot_ext.to_lowercase()) {
                name
            } else {
                truncate_keep_ext(&format!("{name}{dot_ext}"))
            }
        }
        None => name,
    }
}

/// `local.file_rename` — 같은 폴더 안에서 이름만 바꾼다.
pub struct FileRenameRunner {
    journal: Journal,
    recent: RecentWrites,
    roots: AllowedRoots,
}

impl FileRenameRunner {
    pub fn new(journal: Journal, recent: RecentWrites, roots: AllowedRoots) -> Self {
        Self {
            journal,
            recent,
            roots,
        }
    }
}

#[async_trait]
impl NodeRunner for FileRenameRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let cfg = &ctx.config;
        let from = source_file(cfg, &self.roots)?;
        // 원본이 허용 폴더 안이므로 그 폴더도 허용 폴더 안이다.
        let dir = from.parent().unwrap_or(Path::new("/")).to_path_buf();
        let name = renamed(&from, str_cfg(cfg, "newName")?);
        let to = move_no_clobber(&from, &dir, &name)?;
        record(&self.journal, &self.recent, FileOpKind::Rename, from, &to);
        Ok(output(&to, true))
    }
}

/// `local.file_trash` — 휴지통 폴더로 옮긴다(기본 `~/.Trash`).
// ponytail: Finder "되돌려 놓기"는 안 됨, 되돌리기는 앱 기록으로만 — 필요해지면 NSFileManager trashItem 연동
pub struct FileTrashRunner {
    journal: Journal,
    recent: RecentWrites,
    roots: AllowedRoots,
    trash_dir: PathBuf,
}

impl FileTrashRunner {
    pub fn new(journal: Journal, recent: RecentWrites, roots: AllowedRoots) -> Self {
        let trash_dir = std::env::home_dir().unwrap_or_default().join(".Trash");
        Self::with_trash_dir(journal, recent, roots, trash_dir)
    }

    pub fn with_trash_dir(
        journal: Journal,
        recent: RecentWrites,
        roots: AllowedRoots,
        trash_dir: PathBuf,
    ) -> Self {
        Self {
            journal,
            recent,
            roots,
            trash_dir,
        }
    }
}

#[async_trait]
impl NodeRunner for FileTrashRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let from = source_file(&ctx.config, &self.roots)?;
        let trash = real_dir(&self.trash_dir, "휴지통")?;
        let to = move_no_clobber(&from, &trash, &file_name(&from))?;
        record(&self.journal, &self.recent, FileOpKind::Trash, from, &to);
        Ok(output(&to, false))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// 검사 뒤 목적지 폴더가 링크로 바뀌어도(RENAME_SWAP 경합) rename이 링크를 따라가지 않는다.
    #[test]
    fn rename_refuses_symlink_in_path() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        let (base, out) = (root.join("base"), root.join("out"));
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&out).unwrap();
        symlink(&out, base.join("l")).unwrap();
        let src = root.join("a.pdf");
        fs::write(&src, "a").unwrap();

        assert!(rename_excl(&src, &base.join("l/a.pdf")).is_err());
        assert!(src.exists() && !out.join("a.pdf").exists());

        rename_excl(&src, &base.join("a.pdf")).unwrap();
        assert!(base.join("a.pdf").exists());
    }
}
