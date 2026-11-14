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
