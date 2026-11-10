use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_NAME_BYTES: usize = 255;

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

fn truncate_keep_ext(name: &str) -> String {
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
