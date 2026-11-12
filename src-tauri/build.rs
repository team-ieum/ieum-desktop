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
    "keychain_status",
    "open_web_editor",
    "get_settings",
    "set_settings",
    "web_login",
    "web_logout",
    "web_account",
    "current_account",
    "web_list_workflows",
    "web_get_workflow",
    "web_list_executions",
    "web_execution_logs",
    "web_watch_execution",
    "web_unwatch_execution",
    "web_send_code",
    "web_verify_code",
    "web_register",
    "web_dashboard_summary",
    "web_dashboard_executions",
    "web_dashboard_errors",
    "open_help",
    "open_web_workflow",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri 빌드 설정");
}
