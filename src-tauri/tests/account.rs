//! 계정 키·계정 폴더·옛 데이터 옮기기. 모든 파일은 임시 폴더 안에서만 만든다.

use std::fs;

use ieum_desktop_lib::account::{account_key, account_root, claim_legacy};
use ieum_desktop_lib::store::Store;

const A: &str = "aaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbb";
const WF: &str = "11111111-1111-4111-8111-111111111111.json";
const RUN: &str = "22222222-2222-4222-8222-222222222222.json";

#[test]
fn account_key_is_stable_and_case_insensitive() {
    let a = account_key("https://green.kiacms.co.kr", "Me@X.com");
    assert_eq!(a, account_key("https://green.kiacms.co.kr", "me@x.com"));
    assert_eq!(a.len(), 16);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(a, account_key("http://localhost:8080", "me@x.com"));
}

#[test]
fn legacy_claimed_once() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    fs::create_dir_all(dir.join("workflows")).unwrap();
    fs::create_dir_all(dir.join("runs")).unwrap();
    fs::write(dir.join("workflows").join(WF), "{}").unwrap();
    fs::write(dir.join("runs").join(RUN), "{}").unwrap();

    assert!(claim_legacy(dir, A).unwrap());
    assert!(account_root(dir, A).join("workflows").join(WF).exists());
    assert!(account_root(dir, A).join("runs").join(RUN).exists());
    assert!(!dir.join("workflows").exists());
    assert!(!dir.join("runs").exists());

    assert!(!claim_legacy(dir, B).unwrap());
    assert!(!account_root(dir, B).join("workflows").exists());
    assert!(claim_legacy(dir, A).unwrap()); // 같은 계정 재호출은 무해
}

#[test]
fn legacy_claim_resumes_after_partial_move() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    fs::create_dir_all(dir.join("accounts")).unwrap();
    fs::write(dir.join("accounts").join(".legacy-owner"), A).unwrap();
    fs::create_dir_all(dir.join("workflows")).unwrap();
    fs::write(dir.join("workflows").join(WF), "{}").unwrap();

    assert!(!claim_legacy(dir, B).unwrap());
    assert!(dir.join("workflows").join(WF).exists());

    assert!(claim_legacy(dir, A).unwrap());
    assert!(account_root(dir, A).join("workflows").join(WF).exists());
    assert!(!dir.join("workflows").exists());
    assert!(!claim_legacy(dir, B).unwrap());
}

#[test]
fn legacy_claim_without_old_folders_still_marks_owner() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    assert!(claim_legacy(dir, A).unwrap());
    assert!(!claim_legacy(dir, B).unwrap());
}

#[test]
fn invalid_key_is_rejected() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    for bad in ["../../etc", "AAAAAAAAAAAAAAAA", "aaaa", ""] {
        assert!(claim_legacy(dir, bad).is_err(), "{bad:?}");
    }
    assert!(!dir.join("accounts").join(".legacy-owner").exists());
}

#[test]
fn old_settings_file_reads_with_new_fields_default() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    fs::write(
        dir.join("settings.json"),
        r#"{"language":"ko","autostart":true,"firstRunDone":true,"serverUrl":"https://green.kiacms.co.kr"}"#,
    )
    .unwrap();
    let s = Store::new(dir.into()).get_settings().unwrap();
    assert_eq!(s.current_account, None);
    assert_eq!(s.notifications_seen_at, "");
    assert!(s.autostart);
}
