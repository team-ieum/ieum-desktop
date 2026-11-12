//! 폴더 권한 판정·시스템 설정 딥링크.

use std::io::{Error, ErrorKind};

use ieum_desktop_lib::permissions::{FolderAccess, classify, system_settings_url};

#[test]
fn classify_maps_io_errors() {
    assert_eq!(classify(Ok(())), FolderAccess::Granted);
    assert_eq!(
        classify(Err(Error::from(ErrorKind::NotFound))),
        FolderAccess::Missing
    );
    assert_eq!(
        classify(Err(Error::from(ErrorKind::PermissionDenied))),
        FolderAccess::Denied
    );
    assert_eq!(
        classify(Err(Error::from(ErrorKind::Other))),
        FolderAccess::Denied
    );
    assert_eq!(
        serde_json::to_value(FolderAccess::Granted).unwrap(),
        "granted"
    );
}

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
