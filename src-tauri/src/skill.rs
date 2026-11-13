//! `ieum` 스킬 설치 — 원본은 `~/.agents/skills/ieum`, CLI별 자리에는 링크만.
//! 사용자가 고친 사본·이미 있는 것은 덮지 않고, 없는 CLI의 설정 폴더를 만들어 내지 않는다.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

// ponytail: SKILL_MD는 guide를 가리키는 고정 글이라 버전 비교가 없다. 글을 바꾸게 되면 옛 글 목록과 비교해 갱신.
pub const SKILL_MD: &str = "---
name: ieum
description: IEUM 데스크톱 앱의 터미널에서 워크플로우를 만들거나 고칠 때 쓴다. `ieum workflow` 명령으로 노드를 추가·설정·테스트한다.
---

# IEUM 워크플로우

IEUM 앱 안 터미널에서만 동작한다. 먼저 아래 명령을 실행해 지금 앱 버전의 안내를 읽고 그대로 따른다.

    ieum workflow guide
";

/// Codex가 `ieum workflow …`만 묻지 않고 실행하게 하는 규칙(`<codex 폴더>/rules/ieum.rules`).
pub const CODEX_RULE: &str = "prefix_rule(pattern=[\"ieum\", \"workflow\"], decision=\"allow\")\n";

#[derive(Debug, PartialEq, Default)]
pub struct SkillReport {
    /// SKILL.md를 이번에 새로 썼다.
    pub installed: bool,
    /// 사용자가 고친 사본이라 그대로 뒀다.
    pub kept: bool,
}

/// 그 자리에 아무것도 없다(깨진 링크도 "있다"로 본다).
fn vacant(p: &Path) -> bool {
    matches!(p.symlink_metadata(), Err(e) if e.kind() == ErrorKind::NotFound)
}

fn io(e: std::io::Error) -> String {
    format!("스킬을 설치하지 못했어요: {e}")
}

/// `cli_dir`가 있고 `cli_dir/skills/ieum` 자리가 비었을 때만 원본을 가리키는 링크를 건다.
fn link(cli_dir: &Path, src: &Path) -> Result<(), String> {
    let at = cli_dir.join("skills/ieum");
    if !cli_dir.is_dir() || !vacant(&at) {
        return Ok(());
    }
    std::fs::create_dir_all(cli_dir.join("skills")).map_err(io)?;
    std::os::unix::fs::symlink(src, &at).map_err(io)
}

/// `claude_dirs`·`codex_dirs` = 그 CLI의 설정 폴더들(이 Mac의 것 + 앱 기본 계정 폴더). 없는 폴더는 건너뛴다.
pub fn install_skill(
    home: &Path,
    claude_dirs: &[PathBuf],
    codex_dirs: &[PathBuf],
) -> Result<SkillReport, String> {
    let src = home.join(".agents/skills/ieum");
    let md = src.join("SKILL.md");
    let mut report = SkillReport::default();
    if vacant(&md) {
        std::fs::create_dir_all(&src).map_err(io)?;
        std::fs::write(&md, SKILL_MD).map_err(io)?;
        report.installed = true;
    } else if std::fs::read_to_string(&md).ok().as_deref() != Some(SKILL_MD) {
        report.kept = true;
    }
    for dir in claude_dirs {
        link(dir, &src)?;
    }
    link(&home.join(".gemini/config"), &src)?; // Antigravity
    for dir in codex_dirs {
        let rule = dir.join("rules/ieum.rules");
        if dir.is_dir() && vacant(&rule) {
            std::fs::create_dir_all(dir.join("rules")).map_err(io)?;
            std::fs::write(&rule, CODEX_RULE).map_err(io)?;
        }
    }
    Ok(report)
}
