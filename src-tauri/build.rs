/// 화면이 부를 수 있는 앱 명령. capabilities에서 `allow-<명령>`으로 하나씩 허용한다.
const COMMANDS: &[&str] = &[
    "list_workflows",
    "save_workflow",
    "delete_workflow",
    "validate_workflow",
    "run_now",
    "list_runs",
    "get_run",
    "undo_run",
    "detect_providers",
    "set_api_key",
    "delete_api_key",
    "get_settings",
    "set_settings",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri 빌드 설정");
}
