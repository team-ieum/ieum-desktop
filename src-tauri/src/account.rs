//! 계정별 "내 PC" 데이터: `accounts/<계정 키>/{workflows,runs}/`.
//! 계정 키는 이메일을 경로에 쓰지 않으려고 해시 앞 16자만 쓴다.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountId {
    pub email: String,
    pub server_url: String,
}

/// `sha256(server_url + "\n" + 소문자 email)` hex 앞 16자.
pub fn account_key(server_url: &str, email: &str) -> String {
    let digest = Sha256::digest(format!("{server_url}\n{}", email.to_lowercase()));
    hex::encode(digest)[..16].to_string()
}

/// 16자 소문자 hex만 — 폴더 이름이 되므로 `../` 같은 값을 막는다.
pub fn is_valid_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 검사하지 않는다 — 바깥에서 받은 키는 `is_valid_key`로 먼저 거른다.
pub fn account_root(app_data: &Path, key: &str) -> PathBuf {
    app_data.join("accounts").join(key)
}

/// v0.1.0의 `workflows/`·`runs/`를 처음 요청한 계정 하나에만 옮긴다. `true` = 이 계정이 주인.
/// 주인 표시를 먼저 남기고 옮기므로, 옮기다 끊겨도 다음 호출에서 같은 계정만 이어 옮긴다.
pub fn claim_legacy(app_data: &Path, key: &str) -> Result<bool, String> {
    if !is_valid_key(key) {
        return Err(format!("잘못된 계정 키예요: {key:?}"));
    }
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let accounts = app_data.join("accounts");
    let marker = accounts.join(".legacy-owner");
    match fs::read_to_string(&marker) {
        Ok(owner) if owner.trim() == key => {}
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(&accounts).map_err(|e| err(&accounts, e))?;
            let tmp = accounts.join(format!(".{}.tmp", crate::store::new_id()));
            fs::File::create(&tmp)
                .and_then(|mut f| f.write_all(key.as_bytes()).and_then(|()| f.sync_all()))
                .and_then(|()| fs::rename(&tmp, &marker))
                .map_err(|e| err(&marker, e))?;
        }
        Err(e) => return Err(err(&marker, e)),
    }
    let root = account_root(app_data, key);
    for name in ["workflows", "runs"] {
        let old = app_data.join(name);
        let entries = match fs::read_dir(&old) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(err(&old, e)),
        };
        let new = root.join(name);
        fs::create_dir_all(&new).map_err(|e| err(&new, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| err(&old, e))?;
            fs::rename(entry.path(), new.join(entry.file_name()))
                .map_err(|e| err(&entry.path(), e))?;
        }
        fs::remove_dir(&old).map_err(|e| err(&old, e))?;
    }
    Ok(true)
}
