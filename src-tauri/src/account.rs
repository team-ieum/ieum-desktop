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
    let not_found =
        |r: &std::io::Result<fs::Metadata>| matches!(r, Err(e) if e.kind() == ErrorKind::NotFound);
    // 표시는 hard_link로 배타적으로 만든다 — 동시 호출이어도 주인은 하나.
    let mut tries = 0;
    let owner = loop {
        match fs::read_to_string(&marker) {
            Ok(owner) => break owner,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tries += 1;
                if tries > 3 {
                    return Err(format!(
                        "{}: 주인 표시를 만들지 못했어요.",
                        marker.display()
                    ));
                }
                fs::create_dir_all(&accounts).map_err(|e| err(&accounts, e))?;
                let tmp = accounts.join(format!(".{}.tmp", crate::store::new_id()));
                let linked = fs::File::create(&tmp)
                    .and_then(|mut f| f.write_all(key.as_bytes()).and_then(|()| f.sync_all()))
                    .and_then(|()| fs::hard_link(&tmp, &marker));
                let _ = fs::remove_file(&tmp);
                match linked {
                    Ok(()) => break key.to_string(),
                    // 깨진 링크 등 일반 파일이 아닌 표시는 다시 읽어도 NotFound라 끝낸다.
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        match fs::symlink_metadata(&marker) {
                            Ok(m) if m.is_file() => continue,
                            Err(e) if e.kind() == ErrorKind::NotFound => continue,
                            _ => {
                                return Err(format!(
                                    "{}: 주인 표시가 일반 파일이 아니에요.",
                                    marker.display()
                                ));
                            }
                        }
                    }
                    Err(e) => return Err(err(&marker, e)),
                }
            }
            Err(e) => return Err(err(&marker, e)),
        }
    };
    let owner = owner.trim();
    if !is_valid_key(owner) {
        return Err(format!(
            "{}: 내용이 잘못됐어요: {owner:?}",
            marker.display()
        ));
    }
    if owner != key {
        return Ok(false);
    }
    let root = account_root(app_data, key);
    for name in ["workflows", "runs"] {
        let old = app_data.join(name);
        // 링크·일반 파일은 따라가지 않고 건너뛴다 — 바깥 사용자 파일을 옮기지 않게.
        match fs::symlink_metadata(&old) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => continue,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(err(&old, e)),
        }
        // 같은 키의 동시 호출이 먼저 옮기거나 지운 것(NotFound)은 이미 옮겨진 것으로 본다.
        let entries = match fs::read_dir(&old) {
            Ok(e) => e,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(err(&old, e)),
        };
        let new = root.join(name);
        fs::create_dir_all(&new).map_err(|e| err(&new, e))?;
        let mut left = false;
        for entry in entries {
            let entry = entry.map_err(|e| err(&old, e))?;
            let dest = new.join(entry.file_name());
            // 목적지에 이미 있으면 덮어쓰지 않고 옛 자리에 남긴다.
            if !not_found(&fs::symlink_metadata(&dest)) {
                left |= !not_found(&fs::symlink_metadata(entry.path()));
                continue;
            }
            // macOS는 같은 원본을 동시에 rename하면 NotFound 대신 EINVAL을 내기도 해서, 원본이 사라졌는지로 판단한다.
            if let Err(e) = fs::rename(entry.path(), &dest)
                && !not_found(&fs::symlink_metadata(entry.path()))
            {
                return Err(err(&entry.path(), e));
            }
        }
        if !left {
            match fs::remove_dir(&old) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(err(&old, e)),
            }
        }
    }
    Ok(true)
}
