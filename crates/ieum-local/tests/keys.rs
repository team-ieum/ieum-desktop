//! 계정 범위 키(`ScopedKeys`)·키체인 이름 검사·옛 키 옮기기. 키체인은 가짜(이름 검사만 진짜 함수).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::{self, KeyError, ScopedKeys, migrate_legacy_keys};

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
    for name in [
        "anthropic.zz",
        "foo.aaaaaaaaaaaaaaaa",
        "anthropic.aaaaaaaaaaaaaaaaX",
        "anthropic.AAAAAAAAAAAAAAAA",
        "ieum-session.aaaaaaaaaaaaaaaa",
        "anthropic.",
    ] {
        assert!(keys::get(name).is_err(), "{name}");
        assert!(keys::delete(name).is_err(), "{name}");
    }
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
