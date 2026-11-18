//! 키별 월 한도 저장(`key-limits.json`). 모든 파일은 임시 폴더 안에서만 만든다.

use std::fs;

use ieum_desktop_lib::store::{KeyLimitsFile, Notified, Store};

const K: &str = "anthropic:3fa9c1d2";

fn setup() -> (tempfile::TempDir, Store) {
    let t = tempfile::tempdir().unwrap();
    let s = Store::new(t.path().to_path_buf());
    (t, s)
}

#[test]
fn limits_roundtrip() {
    let (t, s) = setup();
    s.set_key_limit(K, Some(10.0)).unwrap();
    assert_eq!(s.key_limits().limits[K], 10.0);
    let disk = fs::read_to_string(t.path().join("key-limits.json")).unwrap();
    assert!(disk.contains("\"limits\""));
}

#[test]
fn rejects_bad_amounts() {
    let (t, s) = setup();
    for v in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(s.set_key_limit(K, Some(v)).is_err(), "{v}");
    }
    assert!(!t.path().join("key-limits.json").exists());
}

#[test]
fn none_removes_limit() {
    let (_t, s) = setup();
    s.set_key_limit(K, Some(10.0)).unwrap();
    s.set_key_limit(K, None).unwrap();
    assert!(!s.key_limits().limits.contains_key(K));
}

#[test]
fn set_limit_clears_notified() {
    let (_t, s) = setup();
    let n = Notified {
        month: "2026-11".into(),
        level: 100,
    };
    s.set_notified(K, n.clone()).unwrap();
    assert_eq!(s.key_limits().notified[K], n);
    s.set_key_limit(K, Some(20.0)).unwrap();
    assert!(!s.key_limits().notified.contains_key(K));
}

#[test]
fn broken_file_starts_empty() {
    let (t, s) = setup();
    fs::write(t.path().join("key-limits.json"), "\"깨진\"").unwrap();
    assert_eq!(s.key_limits(), KeyLimitsFile::default());
    s.set_key_limit(K, Some(5.0)).unwrap();
    assert_eq!(s.key_limits().limits[K], 5.0);
}

#[test]
fn remove_key_limit_clears_both() {
    let (t, s) = setup();
    s.set_key_limit(K, Some(10.0)).unwrap();
    s.set_notified(
        K,
        Notified {
            month: "2026-11".into(),
            level: 80,
        },
    )
    .unwrap();
    s.remove_key_limit(K).unwrap();
    assert_eq!(s.key_limits(), KeyLimitsFile::default());
    // 지울 게 없으면 파일을 쓰지 않는다.
    let (t2, s2) = setup();
    s2.remove_key_limit(K).unwrap();
    assert!(!t2.path().join("key-limits.json").exists());
    drop(t);
}
