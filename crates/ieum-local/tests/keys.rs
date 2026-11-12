//! 계정 범위 키(`ScopedKeys`)·키체인 이름 검사·옛 키 옮기기. 키체인은 가짜(이름 검사만 진짜 함수).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::{
    self, KeyError, RawKeychain, ScopedKeys, VAULT_ACCOUNT, Vault, migrate_legacy_keys,
};

#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl KeyStore for FakeKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().insert(account.into(), key.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

/// 옮기기가 계정 키를 확인하는 순간(`get`) 사용자가 같은 공급원 키를 저장하는 상황을 끼워 넣는다.
#[derive(Default)]
struct RacyKeys {
    map: Mutex<HashMap<String, String>>,
    on_scoped_get: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl KeyStore for RacyKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        // 값을 먼저 읽고(= 옮기기가 "비었다"고 본 뒤) 사용자의 저장을 끼워 넣는다.
        let v = self.map.lock().unwrap().get(account).cloned();
        if account.contains('.')
            && let Some(hook) = self.on_scoped_get.lock().unwrap().take()
        {
            hook();
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        Ok(v)
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        self.map.lock().unwrap().insert(account.into(), key.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        self.map.lock().unwrap().remove(account);
        Ok(())
    }
}

#[test]
fn migrate_does_not_overwrite_key_saved_meanwhile() {
    let inner = Arc::new(RacyKeys::default());
    inner.set("openai", "sk-old").unwrap();
    let user = ScopedKeys::new(inner.clone(), A);
    let (tx, rx) = std::sync::mpsc::channel();
    *inner.on_scoped_get.lock().unwrap() = Some(Box::new(move || {
        tx.send(std::thread::spawn(move || {
            user.set("openai", "sk-user").unwrap()
        }))
        .unwrap();
    }));
    migrate_legacy_keys(inner.as_ref(), A).unwrap();
    rx.recv().unwrap().join().unwrap();
    assert_eq!(
        inner.get("openai.aaaaaaaaaaaaaaaa").unwrap().as_deref(),
        Some("sk-user")
    );
}

const A: &str = "aaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbb";

#[test]
fn scoped_keys_do_not_cross_accounts() {
    let inner = Arc::new(FakeKeys::default());
    let a = ScopedKeys::new(inner.clone(), A);
    let b = ScopedKeys::new(inner.clone(), B);
    a.set("anthropic", "sk-a").unwrap();
    assert_eq!(b.get("anthropic").unwrap(), None);
    assert_eq!(
        inner.get("anthropic.aaaaaaaaaaaaaaaa").unwrap().as_deref(),
        Some("sk-a")
    );
    // 세션은 범위 없이 그대로.
    a.set(keys::SESSION_ACCOUNT, "s").unwrap();
    assert_eq!(b.get(keys::SESSION_ACCOUNT).unwrap().as_deref(), Some("s"));
}

#[test]
fn entry_rejects_unknown_scoped_names() {
    let (raw, _) = FakeRaw::new();
    let vault = Vault::new(raw, vec![]);
    for name in [
        "anthropic.zz",
        "foo.aaaaaaaaaaaaaaaa",
        "anthropic.aaaaaaaaaaaaaaaaX",
        "anthropic.AAAAAAAAAAAAAAAA",
        "ieum-session.aaaaaaaaaaaaaaaa",
        "anthropic.",
        "anthropic.aaaaaaaaaaaaaaaa.0123abcd9",
        "anthropic.aaaaaaaaaaaaaaaa.0123ABCD",
        "anthropic.aaaaaaaaaaaaaaaa.",
        "anthropic.aaaaaaaaaaaaaaaa.default",
        "ieum-session.aaaaaaaaaaaaaaaa.0123abcd",
    ] {
        assert!(vault.get(name).is_err(), "{name}");
        assert!(vault.set(name, "k").is_err(), "{name}");
        assert!(vault.delete(name).is_err(), "{name}");
    }
    // 키 여러 개: `<공급자>.<계정 키>.<키 id 8자 hex>`.
    vault
        .set("anthropic.aaaaaaaaaaaaaaaa.0123abcd", "sk-1")
        .unwrap();
    assert_eq!(
        vault
            .get("anthropic.aaaaaaaaaaaaaaaa.0123abcd")
            .unwrap()
            .as_deref(),
        Some("sk-1")
    );
}

#[test]
fn scoped_keys_pass_own_full_names() {
    let inner = Arc::new(FakeKeys::default());
    let a = ScopedKeys::new(inner.clone(), A);
    a.set("anthropic.aaaaaaaaaaaaaaaa.0123abcd", "sk-a1")
        .unwrap();
    assert_eq!(
        inner
            .get("anthropic.aaaaaaaaaaaaaaaa.0123abcd")
            .unwrap()
            .as_deref(),
        Some("sk-a1")
    );
    // 다른 계정 이름은 그대로 통과시키지 않는다.
    a.get("anthropic.bbbbbbbbbbbbbbbb.0123abcd").unwrap();
    assert!(
        !inner
            .0
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.starts_with("anthropic.bbbb"))
    );
    assert_eq!(a.get("anthropic.bbbbbbbbbbbbbbbb.0123abcd").unwrap(), None);
}

/// 원시 키체인 가짜: 항목 맵 + 읽기 횟수 + 다음 N번 읽기/vault 쓰기 실패.
#[derive(Default)]
struct RawState {
    items: Mutex<HashMap<String, String>>,
    reads: AtomicUsize,
    fail_reads: AtomicUsize,
    fail_vault_writes: AtomicUsize,
}

struct FakeRaw(Arc<RawState>);

impl FakeRaw {
    fn new() -> (Self, Arc<RawState>) {
        let s = Arc::new(RawState::default());
        (FakeRaw(s.clone()), s)
    }
}

fn take(n: &AtomicUsize) -> bool {
    n.try_update(SeqCst, SeqCst, |v| v.checked_sub(1)).is_ok()
}

impl RawKeychain for FakeRaw {
    fn read(&self, account: &str) -> Result<Option<String>, KeyError> {
        self.0.reads.fetch_add(1, SeqCst);
        if take(&self.0.fail_reads) {
            return Err(KeyError("사용자가 취소했어요".into()));
        }
        Ok(self.0.items.lock().unwrap().get(account).cloned())
    }
    fn write(&self, account: &str, value: &str) -> Result<(), KeyError> {
        if account == VAULT_ACCOUNT && take(&self.0.fail_vault_writes) {
            return Err(KeyError("쓰기 실패".into()));
        }
        self.0
            .items
            .lock()
            .unwrap()
            .insert(account.into(), value.into());
        Ok(())
    }
    fn remove(&self, account: &str) -> Result<(), KeyError> {
        self.0.items.lock().unwrap().remove(account);
        Ok(())
    }
}

fn vault_json(s: &RawState) -> HashMap<String, String> {
    serde_json::from_str(&s.items.lock().unwrap()[VAULT_ACCOUNT]).unwrap()
}

#[test]
fn vault_reads_raw_store_once() {
    let (raw, s) = FakeRaw::new();
    s.items
        .lock()
        .unwrap()
        .insert(VAULT_ACCOUNT.into(), r#"{"ieum-session":"s"}"#.into());
    let vault = Vault::new(raw, vec![A.into()]);
    assert_eq!(vault.get("ieum-session").unwrap().as_deref(), Some("s"));
    vault.set("anthropic.aaaaaaaaaaaaaaaa", "sk-a").unwrap();
    assert_eq!(
        vault.get("anthropic.aaaaaaaaaaaaaaaa").unwrap().as_deref(),
        Some("sk-a")
    );
    vault.delete("ieum-session").unwrap();
    assert_eq!(vault.get("ieum-session").unwrap(), None);
    assert_eq!(s.reads.load(SeqCst), 1);
    assert_eq!(
        vault_json(&s),
        HashMap::from([("anthropic.aaaaaaaaaaaaaaaa".to_string(), "sk-a".to_string())])
    );
}

#[test]
fn vault_concurrent_sets_keep_both() {
    let (raw, s) = FakeRaw::new();
    let vault = Arc::new(Vault::new(raw, vec![]));
    let names = ["anthropic", "openai", "gemini", "ieum-session"];
    let handles: Vec<_> = names
        .into_iter()
        .map(|n| {
            let v = vault.clone();
            std::thread::spawn(move || v.set(n, &format!("k-{n}")).unwrap())
        })
        .collect();
    handles.into_iter().for_each(|h| h.join().unwrap());
    let saved = vault_json(&s);
    for n in names {
        assert_eq!(saved[n], format!("k-{n}"));
    }
}

#[test]
fn vault_read_error_not_cached() {
    let (raw, s) = FakeRaw::new();
    s.items
        .lock()
        .unwrap()
        .insert(VAULT_ACCOUNT.into(), r#"{"openai":"sk-o"}"#.into());
    s.fail_reads.store(1, SeqCst);
    let vault = Vault::new(raw, vec![]);
    assert!(vault.get("openai").is_err());
    assert_eq!(vault.get("openai").unwrap().as_deref(), Some("sk-o"));
}

#[test]
fn legacy_items_move_into_vault_then_deleted() {
    let (raw, s) = FakeRaw::new();
    {
        let mut m = s.items.lock().unwrap();
        m.insert("openai".into(), "sk-o".into());
        m.insert("ieum-session".into(), "sess".into());
        m.insert("anthropic.aaaaaaaaaaaaaaaa".into(), "sk-a".into());
        m.insert("gemini.bbbbbbbbbbbbbbbb".into(), "sk-b".into());
    }
    let vault = Vault::new(raw, vec![A.into(), B.into()]);
    assert_eq!(
        vault.get("anthropic.aaaaaaaaaaaaaaaa").unwrap().as_deref(),
        Some("sk-a")
    );
    assert_eq!(
        vault.get("gemini.bbbbbbbbbbbbbbbb").unwrap().as_deref(),
        Some("sk-b")
    );
    assert_eq!(vault.get("openai").unwrap().as_deref(), Some("sk-o"));
    assert_eq!(vault.get("ieum-session").unwrap().as_deref(), Some("sess"));
    let m = s.items.lock().unwrap();
    assert_eq!(m.keys().collect::<Vec<_>>(), vec![VAULT_ACCOUNT]);
}

#[test]
fn legacy_move_resumes_after_crash() {
    let (raw, s) = FakeRaw::new();
    s.items
        .lock()
        .unwrap()
        .insert("anthropic.aaaaaaaaaaaaaaaa".into(), "sk-a".into());
    s.fail_vault_writes.store(1, SeqCst);
    let vault = Vault::new(raw, vec![A.into()]);
    // vault 쓰기 전 끊김 → 옛 항목 그대로.
    assert!(vault.get("anthropic.aaaaaaaaaaaaaaaa").is_err());
    {
        let m = s.items.lock().unwrap();
        assert_eq!(m["anthropic.aaaaaaaaaaaaaaaa"], "sk-a");
        assert!(!m.contains_key(VAULT_ACCOUNT));
    }
    // 다음 시도(= 다음 실행)에서 이어 옮긴다.
    let vault = Vault::new(FakeRaw(s.clone()), vec![A.into()]);
    assert_eq!(
        vault.get("anthropic.aaaaaaaaaaaaaaaa").unwrap().as_deref(),
        Some("sk-a")
    );
    assert!(
        !s.items
            .lock()
            .unwrap()
            .contains_key("anthropic.aaaaaaaaaaaaaaaa")
    );
}

#[test]
fn vault_errors_never_contain_values() {
    let (raw, s) = FakeRaw::new();
    s.items.lock().unwrap().insert(
        VAULT_ACCOUNT.into(),
        r#"{"openai": "sk-secret-1", oops"#.into(),
    );
    let vault = Vault::new(raw, vec![]);
    let e = vault.get("openai").unwrap_err();
    assert!(!format!("{e} {e:?}").contains("sk-secret"), "{e}");
    let e = vault.set("openai", "sk-secret-2").unwrap_err();
    assert!(!format!("{e} {e:?}").contains("sk-secret"), "{e}");
    assert!(!format!("{vault:?}").contains("sk-secret"));
}

#[test]
fn legacy_keys_move_to_first_account_only() {
    let inner = Arc::new(FakeKeys::default());
    inner.set("openai", "sk-o").unwrap();
    inner.set("anthropic", "sk-old").unwrap();
    inner.set("anthropic.aaaaaaaaaaaaaaaa", "sk-new").unwrap();
    migrate_legacy_keys(inner.as_ref(), A).unwrap();
    let a = ScopedKeys::new(inner.clone(), A);
    assert_eq!(a.get("openai").unwrap().as_deref(), Some("sk-o"));
    // 이미 있던 계정 키는 덮지 않는다.
    assert_eq!(a.get("anthropic").unwrap().as_deref(), Some("sk-new"));
    assert_eq!(inner.get("openai").unwrap(), None);
    assert_eq!(inner.get("anthropic").unwrap(), None);

    migrate_legacy_keys(inner.as_ref(), B).unwrap();
    let b = ScopedKeys::new(inner.clone(), B);
    assert_eq!(b.get("openai").unwrap(), None);
}
