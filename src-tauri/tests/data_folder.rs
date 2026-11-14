//! 설정 고급 [데이터 폴더 열기] — 앱 데이터 폴더만, `/usr/bin/open`으로.

use std::ffi::OsStr;
use std::path::Path;

use ieum_desktop_lib::commands::data_folder_command;

#[test]
fn open_data_folder_opens_only_app_data_dir() {
    let dir = Path::new("/Users/me/Library/Application Support/com.ieum.desktop");
    let cmd = data_folder_command(dir);
    assert_eq!(cmd.get_program(), OsStr::new("/usr/bin/open"));
    let args: Vec<&OsStr> = cmd.get_args().collect();
    assert_eq!(args, [dir.as_os_str()]);
}
