/// 화면이 부를 수 있는 앱 명령. capabilities에서 `allow-<명령>`으로 하나씩 허용한다.
const COMMANDS: &[&str] = &[
    "list_workflows",
    "save_workflow",
    "delete_workflow",
    "validate_workflow",
    "run_now",
    "list_runs",
    "get_run",
    "usage_summary",
    "usage_report",
    "undo_run",
    "detect_providers",
    "list_credentials",
    "add_api_key",
    "rename_credential",
    "set_default_credential",
    "delete_credential",
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
    "begin_cli_account",
    "confirm_cli_account",
    "cancel_cli_account",
    "set_system_hidden",
    "subscription_usage",
    "antigravity_plan_usage",
    "open_system_settings",
    "open_provider_console",
    "send_test_notification",
    "open_data_folder",
    "check_update_now",
    // ── 캔버스 채팅: 토대 ──
    "get_chat",
    "pending_finish",
    "confirm_finish",
    // ── 캔버스 채팅 B(통로·동기화) — 여기 아래에 추가 ──
    "set_editing",
    "finish_notice",
    "undo_failed_notice",
    // ── 캔버스 채팅 C(터미널 term_*) — 여기 아래에 추가 ──
    // ── 캔버스 채팅 D(채팅 패널 chat_*) — 여기 아래에 추가 ──
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri 빌드 설정");
}
