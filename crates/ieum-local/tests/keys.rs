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
