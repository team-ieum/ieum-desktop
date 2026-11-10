use std::fs;
use std::path::{Component, Path, PathBuf};

use ieum_engine::Workflow;

const MAX_NAME_BYTES: usize = 255;

/// 파일 노드가 원본을 집어도 되는 폴더들(트리거 폴더 + 각 `file_move`의 기준 폴더).
/// 실제 경로(canonicalize)로 보관하고, 상대 경로·없는 폴더는 버린다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowedRoots(Vec<PathBuf>);

/// 워크플로우의 허용 폴더 — 트리거 `folder` + 모든 `local.file_move`의 `baseFolder`. CLI와 앱이 같이 쓴다.
pub fn roots_for_workflow(wf: &Workflow) -> AllowedRoots {
    AllowedRoots::new(wf.nodes.iter().filter_map(|n| {
        let key = match n.event.as_str() {
            "local.folder_new_file" => "folder",
            "local.file_move" => "baseFolder",
            _ => return None,
        };
        let dir = n.config.get(key)?.as_str().filter(|s| !s.is_empty())?;
        Some(PathBuf::from(dir))
    }))
}

impl AllowedRoots {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self(
            roots
                .into_iter()
                // 상대 경로는 cwd 기준으로 풀려(Finder 실행 = `/`) 디스크 전체가 될 수 있다
                .filter(|r| r.is_absolute())
                .filter_map(|r| r.canonicalize().ok())
                .collect(),
        )
    }

    /// 실제 경로 목록 — 실행 기록에 남겨 되돌릴 때 그대로 쓴다.
    pub fn paths(&self) -> &[PathBuf] {
        &self.0
    }

    /// `path`는 실제 경로여야 한다.
    pub(crate) fn contains(&self, path: &Path) -> bool {
        self.0.iter().any(|r| path.starts_with(r))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("절대 경로는 쓸 수 없어요: {0}")]
    Absolute(String),
    #[error("기준 폴더 밖을 가리켜요: {0}")]
    Escape(String),
    #[error("경로에 심볼릭 링크가 있어요: {}", .0.display())]
    Symlink(PathBuf),
}

/// `rel`을 `base` 아래 상대 경로로만 해석한다. 칸마다 `sanitize_name`을 거친다.
/// 절대 경로·`..`·이미 있는 심볼릭 링크를 지나는 경로는 에러.
pub fn resolve_in_base(base: &Path, rel: &str) -> Result<PathBuf, PathError> {
    let mut out = base.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => return Err(PathError::Escape(rel.into())),
            Component::RootDir | Component::Prefix(_) => {
                return Err(PathError::Absolute(rel.into()));
            }
            Component::Normal(s) => {
                out.push(sanitize_name(&s.to_string_lossy()));
                if fs::symlink_metadata(&out).is_ok_and(|m| m.file_type().is_symlink()) {
                    return Err(PathError::Symlink(out));
                }
            }
        }
    }
    Ok(out)
}

/// 파일·폴더 이름 한 칸으로 쓸 수 있게 고친다.
pub fn sanitize_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == ':' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = replaced.trim_matches(|c: char| c.is_whitespace() || c == '.');
    let out = truncate_keep_ext(trimmed);
    if out.is_empty() {
        "untitled".into()
    } else {
        out
    }
}

/// `name (n).ext` — 255바이트를 넘지 않게 앞부분을 자른다. `n == 0`이면 그대로.
pub(crate) fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.into();
    }
    let suffix = format!(" ({n})");
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let stem = &name[..name.len() - ext.len()];
    let room = MAX_NAME_BYTES.saturating_sub(ext.len() + suffix.len());
    format!("{}{suffix}{ext}", floor_char(stem, room))
}

pub(crate) fn truncate_keep_ext(name: &str) -> String {
    if name.len() <= MAX_NAME_BYTES {
        return name.into();
    }
    let ext = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i < MAX_NAME_BYTES => &name[i..],
        _ => "",
    };
    let stem = &name[..name.len() - ext.len()];
    format!("{}{ext}", floor_char(stem, MAX_NAME_BYTES - ext.len()))
}

fn floor_char(s: &str, max: usize) -> &str {
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
