//! ieum 스킬 설치 — 원본 하나 + CLI별 링크, 사용자가 고친 사본은 그대로.

use ieum_desktop_lib::skill::{CODEX_RULE, SKILL_MD, SkillReport, install_skill};

fn home() -> tempfile::TempDir {
    let h = tempfile::tempdir().unwrap();
    for d in [".claude", ".gemini/config", ".codex"] {
        std::fs::create_dir_all(h.path().join(d)).unwrap();
    }
    h
}

#[test]
fn installs_skill_links_and_codex_rule() {
    let h = home();
    let acct = h.path().join("accts/claude/a1b2c3d4");
    std::fs::create_dir_all(&acct).unwrap();
    let r = install_skill(
        h.path(),
        &[h.path().join(".claude"), acct.clone()],
        &[h.path().join(".codex")],
    )
    .unwrap();
    assert_eq!(
        r,
        SkillReport {
            installed: true,
            kept: false
        }
    );
    let src = h.path().join(".agents/skills/ieum");
    assert_eq!(
        std::fs::read_to_string(src.join("SKILL.md")).unwrap(),
        SKILL_MD
    );
    assert!(
        SKILL_MD.starts_with("---\nname: ieum\ndescription: ")
            && SKILL_MD.contains("ieum workflow guide")
    );
    for link in [
        h.path().join(".claude/skills/ieum"),
        acct.join("skills/ieum"),
        h.path().join(".gemini/config/skills/ieum"),
    ] {
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            src,
            "{}",
            link.display()
        );
    }
    assert_eq!(
        std::fs::read_to_string(h.path().join(".codex/rules/ieum.rules")).unwrap(),
        CODEX_RULE
    );
    // 두 번째는 아무것도 바꾸지 않는다
    assert_eq!(
        install_skill(
            h.path(),
            &[h.path().join(".claude")],
            &[h.path().join(".codex")]
        )
        .unwrap(),
        SkillReport::default()
    );
}

#[test]
fn user_edited_copies_are_kept() {
    let h = home();
    let src = h.path().join(".agents/skills/ieum");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("SKILL.md"), "내가 고친 글").unwrap();
    std::fs::create_dir_all(h.path().join(".claude/skills/ieum")).unwrap(); // 링크 자리에 사용자의 폴더
    std::fs::create_dir_all(h.path().join(".codex/rules")).unwrap();
    std::fs::write(h.path().join(".codex/rules/ieum.rules"), "# mine\n").unwrap();
    let r = install_skill(
        h.path(),
        &[h.path().join(".claude")],
        &[h.path().join(".codex")],
    )
    .unwrap();
    assert_eq!(
        r,
        SkillReport {
            installed: false,
            kept: true
        }
    );
    assert_eq!(
        std::fs::read_to_string(src.join("SKILL.md")).unwrap(),
        "내가 고친 글"
    );
    assert!(!h.path().join(".claude/skills/ieum").is_symlink());
    assert_eq!(
        std::fs::read_to_string(h.path().join(".codex/rules/ieum.rules")).unwrap(),
        "# mine\n"
    );
}

#[test]
fn missing_cli_folders_are_not_created() {
    let h = tempfile::tempdir().unwrap(); // .claude·.gemini·.codex 없음
    install_skill(
        h.path(),
        &[h.path().join(".claude")],
        &[h.path().join(".codex")],
    )
    .unwrap();
    assert!(h.path().join(".agents/skills/ieum/SKILL.md").is_file());
    for p in [".claude", ".gemini", ".codex"] {
        assert!(!h.path().join(p).exists(), "{p}");
    }
}
