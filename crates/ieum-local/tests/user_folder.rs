//! 사용자 폴더 정책. 가짜 홈은 임시 폴더.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use ieum_local::validate_user_folder;

#[test]
fn user_folder_policy() {
    let t = tempfile::tempdir().unwrap();
    let outside = t.path().join("outside");
    let home = t.path().join("home");
    let app = home.join("AppData");
    for d in [
        "Downloads/sub",
        "Documents",
        "Desktop",
        "Library/LaunchAgents",
        ".ssh",
        "Downloads/.hidden",
        "AppData/workflows",
    ] {
        fs::create_dir_all(home.join(d)).unwrap();
    }
    fs::create_dir_all(&outside).unwrap();
    symlink(home.join("Library/LaunchAgents"), home.join("Agents")).unwrap();
    symlink(&outside, home.join("Out")).unwrap();
    symlink(home.join("Documents"), home.join("Docs")).unwrap();

    let check = |p: &Path| validate_user_folder(p, &home, &app);

    for ok in [
        home.join("Downloads"),
        home.join("Downloads/sub"),
        home.join("Documents"),
        home.join("Desktop"),
        home.join("Docs"),                // 홈 아래 일반 폴더로 가는 링크
        home.join("Documents/아직 없음"), // 없는 폴더도 규칙만 맞으면
    ] {
        assert_eq!(check(&ok), Ok(()), "{}", ok.display());
    }

    for (bad, why) in [
        (Path::new("Downloads").to_path_buf(), "절대 경로"),
        (Path::new("/").to_path_buf(), "루트"),
        (outside.clone(), "홈 폴더 아래"),
        (home.clone(), "홈 폴더 자체"),
        (home.join("Library/LaunchAgents"), "~/Library"),
        (home.join("library/x"), "~/Library"),
        (home.join(".ssh"), "숨김"),
        (home.join("Downloads/.hidden"), "숨김"),
        (home.join("Downloads/../Library"), "숨김"),
        (home.join("AppData/workflows"), "앱 데이터"),
        (home.join("Agents"), "~/Library"),   // 링크 → ~/Library
        (home.join("Out/x"), "홈 폴더 아래"), // 링크 → 홈 밖
    ] {
        let err = check(&bad).expect_err(&bad.display().to_string());
        assert!(err.contains(why), "{}: {err}", bad.display());
    }
}
