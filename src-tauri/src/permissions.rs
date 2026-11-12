//! 권한: 시스템 설정 딥링크(정해진 것만).

/// 화면이 고른 창 이름 → 시스템 설정 주소. 모르는 이름은 None(임의 URL 금지).
pub fn system_settings_url(pane: &str, bundle_id: &str) -> Option<String> {
    Some(match pane {
        "files" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders".into()
        }
        "notifications" => format!(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={bundle_id}"
        ),
        "login_items" => "x-apple.systempreferences:com.apple.LoginItems-Settings.extension".into(),
        _ => return None,
    })
}
