//! 온보딩 권한: 폴더 접근 판정, 시스템 설정 딥링크(정해진 것만).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess {
    Granted,
    Denied,
    Missing,
}

/// 폴더를 한 번 읽어 본 결과. 없는 폴더만 Missing, 그 밖 오류는 전부 Denied.
pub fn classify(r: std::io::Result<()>) -> FolderAccess {
    match r {
        Ok(()) => FolderAccess::Granted,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FolderAccess::Missing,
        Err(_) => FolderAccess::Denied,
    }
}

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
