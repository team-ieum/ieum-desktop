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
    assert!(
        a.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_ne!(a, account_key("http://localhost:8080", "me@x.com"));
    assert_eq!(
        account_key("https://green.kiacms.co.kr", "me@x.com"),
        "23d01d95b2708534"
    );
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
    for bad in [
        "../../etc",
        "AAAAAAAAAAAAAAAA",
        "aaaa",
        "",
        "aaaaaaaaaaaaaaaaa",
    ] {
        assert!(claim_legacy(dir, bad).is_err(), "{bad:?}");
    }
    assert!(!dir.join("accounts").join(".legacy-owner").exists());
}

#[test]
fn invalid_marker_content_is_error() {
    for content in ["", "zzzz", "../../etc"] {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        fs::create_dir_all(dir.join("accounts")).unwrap();
        fs::write(dir.join("accounts").join(".legacy-owner"), content).unwrap();
        assert!(claim_legacy(dir, A).is_err(), "{content:?}");
    }
}

#[test]
fn marker_written_before_move() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    let old = dir.join("workflows");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join(WF), "{}").unwrap();
    fs::write(old.join("same.json"), "OLD").unwrap();
    // 목적지에 같은 이름의 비어 있지 않은 폴더 — 이 파일은 옮기지 못한다.
    let blocker = account_root(dir, A).join("workflows").join("same.json");
    fs::create_dir_all(&blocker).unwrap();
    fs::write(blocker.join("x"), "x").unwrap();

    let _ = claim_legacy(dir, A);
    let owner = fs::read_to_string(dir.join("accounts").join(".legacy-owner")).unwrap();
    assert_eq!(owner.trim(), A);
    assert!(!claim_legacy(dir, B).unwrap());
    assert_eq!(fs::read_to_string(old.join("same.json")).unwrap(), "OLD");
    assert!(!account_root(dir, B).join("workflows").exists());
}

#[test]
fn concurrent_claims_have_one_owner() {
    use std::sync::{Arc, Barrier};
    for _ in 0..50 {
        let t = tempfile::tempdir().unwrap();
        let dir = Arc::new(t.path().to_path_buf());
        fs::create_dir_all(dir.join("workflows")).unwrap();
        fs::write(dir.join("workflows").join(WF), "{}").unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = [A, B]
            .into_iter()
            .map(|k| {
                let (dir, barrier) = (dir.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    (k, claim_legacy(&dir, k).unwrap())
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let winners: Vec<_> = results.iter().filter(|(_, won)| *won).collect();
        assert_eq!(winners.len(), 1, "{results:?}");
        let loser = results.iter().find(|(_, won)| !won).unwrap().0;
        assert!(!account_root(&dir, loser).join("workflows").exists());
        assert!(
            account_root(&dir, winners[0].0)
                .join("workflows")
                .join(WF)
                .exists()
        );
    }
}

#[test]
fn concurrent_same_key_claims_both_succeed() {
    use std::sync::{Arc, Barrier};
    for _ in 0..50 {
        let t = tempfile::tempdir().unwrap();
        let dir = Arc::new(t.path().to_path_buf());
        fs::create_dir_all(dir.join("workflows")).unwrap();
        for i in 0..50 {
            fs::write(dir.join("workflows").join(format!("{i}.json")), "{}").unwrap();
        }
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (dir, barrier) = (dir.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    claim_legacy(&dir, A)
                })
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), Ok(true));
        }
        let moved = fs::read_dir(account_root(&dir, A).join("workflows"))
            .unwrap()
            .count();
        assert_eq!(moved, 50);
    }
}

#[cfg(unix)]
#[test]
fn dangling_marker_link_is_error_not_loop() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().to_path_buf();
    fs::create_dir_all(dir.join("accounts")).unwrap();
    std::os::unix::fs::symlink("/nonexistent", dir.join("accounts").join(".legacy-owner")).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(claim_legacy(&dir, A)).unwrap());
    let r = rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("2초 안에 끝나야 한다");
    assert!(r.is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_old_folder_is_left_alone() {
    let t = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dir = t.path();
    fs::write(outside.path().join(WF), "user").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.join("workflows")).unwrap();
    fs::create_dir_all(dir.join("runs")).unwrap();
    fs::write(dir.join("runs").join(RUN), "{}").unwrap();

    assert!(claim_legacy(dir, A).unwrap());
    assert_eq!(fs::read_to_string(outside.path().join(WF)).unwrap(), "user");
    assert!(!account_root(dir, A).join("workflows").join(WF).exists());
    assert!(account_root(dir, A).join("runs").join(RUN).exists());
    assert!(claim_legacy(dir, A).unwrap());
}

#[test]
fn existing_destination_is_not_overwritten() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    let dest = account_root(dir, A).join("workflows");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("same.json"), "NEW").unwrap();
    fs::create_dir_all(dir.join("workflows")).unwrap();
    fs::write(dir.join("workflows").join("same.json"), "OLD").unwrap();

    assert!(claim_legacy(dir, A).unwrap());
    assert_eq!(fs::read_to_string(dest.join("same.json")).unwrap(), "NEW");
    assert_eq!(
        fs::read_to_string(dir.join("workflows").join("same.json")).unwrap(),
        "OLD"
    );
    assert!(claim_legacy(dir, A).unwrap());
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
