//! 번들에 넣는 `ieum`은 번들 전용 설정에만 있고, `workflow` 전용 bin만 빌드한다.

#[test]
fn only_the_bundle_config_carries_the_ieum_binary() {
    let bundle: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.bundle.conf.json")).unwrap();
    assert_eq!(
        bundle["bundle"]["externalBin"],
        serde_json::json!(["binaries/ieum"])
    );
    let base: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    // 있으면 사본 없이 cargo test가 깨진다
    assert!(base["bundle"].get("externalBin").is_none());
    let script = include_str!("../../scripts/build-ieum-bin.sh");
    // run·conformance는 번들 밖
    assert!(script.contains("--bin ieum ") && !script.contains("ieum-dev"));
}

/// 개발 빌드(`npm run tauri dev`)도 앱 실행 파일 옆에 `ieum`을 둔다 — 없으면 터미널의 첫 명령이 `command not found`.
#[test]
fn dev_command_builds_the_ieum_binary_first() {
    let base: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let dev = base["build"]["beforeDevCommand"].as_str().unwrap();
    assert!(
        dev.starts_with("cargo build -p ieum-cli --bin ieum && "),
        "{dev}"
    );
}

/// bin 폴더에 `ieum` 파일이 없으면 세션을 띄우지 않고 무엇이 없는지 알린다.
#[test]
fn session_refused_without_the_ieum_binary() {
    use ieum_desktop_lib::channel::require_ieum;
    let t = tempfile::tempdir().unwrap();
    let err = require_ieum(t.path()).unwrap_err();
    assert!(
        err.contains("ieum") && err.contains(&t.path().display().to_string()),
        "{err}"
    );
    // 같은 이름의 폴더는 실행 파일이 아니다
    std::fs::create_dir(t.path().join("ieum")).unwrap();
    assert!(require_ieum(t.path()).is_err());
    std::fs::remove_dir(t.path().join("ieum")).unwrap();
    std::fs::write(t.path().join("ieum"), "").unwrap();
    assert!(require_ieum(t.path()).is_ok());
}
