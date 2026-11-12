//! 시스템 설정 딥링크.

use ieum_desktop_lib::permissions::system_settings_url;

#[test]
fn system_settings_url_only_known_panes() {
    let id = "com.ieum.desktop";
    assert_eq!(
        system_settings_url("files", id).as_deref(),
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders")
    );
    assert_eq!(
        system_settings_url("notifications", id).as_deref(),
        Some(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=com.ieum.desktop"
        )
    );
    assert_eq!(
        system_settings_url("login_items", id).as_deref(),
        Some("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")
    );
    for bad in ["https://evil", "", "files;rm", "FILES"] {
        assert_eq!(system_settings_url(bad, id), None, "{bad}");
    }
}
