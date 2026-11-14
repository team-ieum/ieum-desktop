//! API 키 저장 — macOS 키체인(`keyring`). 서비스 `com.ieum.desktop`, 계정 `vault` 항목 하나에
//! JSON `{ "<이름>": "<값>" }`로 모은다. 이름 = `<anthropic|openai|gemini>.<계정 키>[.<키 id>]`·`ieum-session`
//! (범위 없는 이름은 v0.1.0 옛 키 — 옮기기 전용). 이름마다 따로였던 옛 항목은 첫 읽기 때 `vault`로 옮긴다.
//! 키는 `ApiKey`로만 들고 다닌다 — `Debug`가 가린다.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};

const SERVICE: &str = "com.ieum.desktop";
const ACCOUNTS: [&str; 3] = ["anthropic", "openai", "gemini"];
/// 이음 계정 세션(접근·갱신 토큰 JSON). API 키 명령으로는 다루지 않는다.
pub const SESSION_ACCOUNT: &str = "ieum-session";

/// API 키. `Debug`·에러 어디에도 원문이 나가지 않게 감싼다.
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        ApiKey(key.into())
    }

    /// 헤더에 넣을 때만 쓴다.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// 글 안의 키를 `***`로 바꾼다.
    pub fn mask(&self, text: &str) -> String {
        if self.0.is_empty() {
            return text.to_string();
        }
        text.replace(&self.0, "***")
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

/// 키 저장소. 운영은 `Keychain`, 테스트는 가짜를 주입한다.
pub trait KeyStore: Send + Sync {
    /// 계정(`anthropic` 등)의 키. 없으면 `Ok(None)`, 키체인 접근 오류는 `Err`.
    fn get(&self, account: &str) -> Result<Option<String>, KeyError>;
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError>;
    fn delete(&self, account: &str) -> Result<(), KeyError>;
}

/// macOS 키체인. 모든 인스턴스가 프로세스 하나의 `Vault`(항목 하나)를 함께 쓴다.
pub struct Keychain;

static LEGACY_SCOPES: OnceLock<Vec<String>> = OnceLock::new();
static VAULT: LazyLock<Vault<Keyring>> =
    LazyLock::new(|| Vault::new(Keyring, LEGACY_SCOPES.get().cloned().unwrap_or_default()));

/// 옛 개별 항목을 옮길 계정 키(`accounts/` 폴더 이름) 목록. 첫 키체인 접근 전에 한 번 부른다.
pub fn set_legacy_scopes(key16s: Vec<String>) {
    let _ = LEGACY_SCOPES.set(key16s);
}

impl KeyStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        VAULT.get(account)
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        VAULT.set(account, key)
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        VAULT.delete(account)
    }
}

/// 키체인 항목 하나(서비스 `com.ieum.desktop`)를 읽고 쓰는 원시 저장소. 테스트는 가짜를 넣는다.
pub trait RawKeychain: Send + Sync {
    /// 없으면 `Ok(None)`.
    fn read(&self, account: &str) -> Result<Option<String>, KeyError>;
    fn write(&self, account: &str, value: &str) -> Result<(), KeyError>;
    /// 없던 항목이면 성공.
    fn remove(&self, account: &str) -> Result<(), KeyError>;
}

/// 모든 키를 담는 키체인 계정. 값 = JSON `{ "<이름>": "<값>" }`.
pub const VAULT_ACCOUNT: &str = "vault";

type Map = BTreeMap<String, String>;

/// 키를 항목 하나에 모아 둔다 — 서명이 바뀐 빌드에서도 허용 창이 실행당 한 번만 뜨게.
/// 첫 접근 때 한 번 읽어 캐시하고, 쓰기는 캐시 갱신 + 항목 전체 다시 쓰기(한 잠금 아래).
pub struct Vault<R> {
    raw: R,
    legacy_scopes: Vec<String>,
    /// `None` = 아직 못 읽음(읽기 실패는 굳히지 않고 다음 호출에서 다시 읽는다).
    cache: Mutex<Option<Map>>,
}

impl<R> fmt::Debug for Vault<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Vault(***)")
    }
}

impl<R: RawKeychain> Vault<R> {
    pub fn new(raw: R, legacy_scopes: Vec<String>) -> Self {
        Vault {
            raw,
            legacy_scopes,
            cache: Mutex::new(None),
        }
    }

    fn loaded(&self) -> Result<MutexGuard<'_, Option<Map>>, KeyError> {
        let mut g = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(self.load()?);
        }
        Ok(g)
    }

    fn load(&self) -> Result<Map, KeyError> {
        if let Some(raw) = self.raw.read(VAULT_ACCOUNT)? {
            // serde 오류 문자열에는 원문이 섞일 수 있어 고정 문구만.
            return serde_json::from_str(&raw)
                .map_err(|_| KeyError("키체인의 키 묶음을 읽을 수 없어요(형식 오류).".into()));
        }
        // 옛 개별 항목 → vault. vault를 쓴 뒤에만 옛 항목을 지운다 — 그 전에 끊기면 다음에 처음부터 다시.
        let mut names: Vec<String> = ACCOUNTS
            .iter()
            .chain([&SESSION_ACCOUNT])
            .map(|s| s.to_string())
            .collect();
        for k in &self.legacy_scopes {
            names.extend(ACCOUNTS.iter().map(|a| format!("{a}.{k}")));
        }
        names.retain(|n| valid_name(n));
        let mut map = Map::new();
        for name in names {
            if let Some(v) = self.raw.read(&name)? {
                let v = v.trim();
                if !v.is_empty() {
                    map.insert(name, v.to_string());
                }
            }
        }
        self.raw.write(VAULT_ACCOUNT, &encode(&map)?)?;
        for name in map.keys() {
            if let Err(e) = self.raw.remove(name) {
                eprintln!("[ieum] 옛 키체인 항목을 지우지 못했어요: {name}: {e}");
            }
        }
        Ok(map)
    }

    /// 캐시 사본을 고쳐 항목에 쓰고, 성공했을 때만 캐시를 바꾼다.
    fn update(&self, f: impl FnOnce(&mut Map)) -> Result<(), KeyError> {
        let mut g = self.loaded()?;
        let cur = g.as_ref().expect("loaded");
        let mut next = cur.clone();
        f(&mut next);
        if &next == cur {
            return Ok(());
        }
        self.raw.write(VAULT_ACCOUNT, &encode(&next)?)?;
        *g = Some(next);
        Ok(())
    }
}

fn encode(map: &Map) -> Result<String, KeyError> {
    serde_json::to_string(map).map_err(|_| KeyError("키 묶음을 만들지 못했어요.".into()))
}

fn check_name(name: &str) -> Result<(), KeyError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(KeyError(format!("알 수 없는 공급원이에요: {name}")))
    }
}

impl<R: RawKeychain> KeyStore for Vault<R> {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        check_name(account)?;
        Ok(self
            .loaded()?
            .as_ref()
            .expect("loaded")
            .get(account)
            .cloned())
    }
    /// 앞뒤 공백은 뺀다.
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        check_name(account)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(KeyError("키가 비었어요.".into()));
        }
        self.update(|m| {
            m.insert(account.to_string(), key.to_string());
        })
    }
    /// 없던 키면 성공.
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        check_name(account)?;
        self.update(|m| {
            m.remove(account);
        })
    }
}

/// 운영 원시 저장소(`keyring`).
pub struct Keyring;

fn entry(account: &str) -> Result<keyring::Entry, KeyError> {
    keyring::Entry::new(SERVICE, account).map_err(|e| KeyError(format!("키체인 오류: {e}")))
}

impl RawKeychain for Keyring {
    /// 접근 거부·사용자 취소 같은 키체인 오류는 삼키지 않고 `Err`.
    fn read(&self, account: &str) -> Result<Option<String>, KeyError> {
        match entry(account)?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            // BadEncoding은 키 바이트를 담으므로 `{e}`를 쓰지 않는다.
            Err(keyring::Error::BadEncoding(_)) => Err(KeyError(
                "키체인의 키를 읽을 수 없어요(인코딩 오류).".into(),
            )),
            Err(e) => Err(KeyError(format!("키체인에서 키를 읽지 못했어요: {e}"))),
        }
    }
    fn write(&self, account: &str, value: &str) -> Result<(), KeyError> {
        entry(account)?
            .set_password(value)
            .map_err(|e| KeyError(format!("키체인에 저장하지 못했어요: {e}")))
    }
    fn remove(&self, account: &str) -> Result<(), KeyError> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyError(format!("키체인에서 지우지 못했어요: {e}"))),
        }
    }
}

/// 키체인 오류. 메시지에 키 값은 넣지 않는다.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct KeyError(pub String);

/// 계정 하나의 키만 보는 저장소. `anthropic` → `anthropic.<계정 키>`. 세션은 한 번에 하나라 범위 없이 통과.
pub struct ScopedKeys {
    inner: Arc<dyn KeyStore>,
    key16: String,
}

impl ScopedKeys {
    pub fn new(inner: Arc<dyn KeyStore>, key16: &str) -> Self {
        ScopedKeys {
            inner,
            key16: key16.to_string(),
        }
    }

    /// 이 계정의 전체 이름(`anthropic.<계정 키>[.<키 id>]`)은 그대로 — 다른 계정 이름은 뒤에 계정 키가 붙어 거부된다.
    fn name(&self, account: &str) -> String {
        let own = account.split('.').nth(1) == Some(self.key16.as_str());
        if account == SESSION_ACCOUNT || own {
            account.to_string()
        } else {
            format!("{account}.{}", self.key16)
        }
    }
}

impl KeyStore for ScopedKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        self.inner.get(&self.name(account))
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        let _g = write_lock();
        self.inner.set(&self.name(account), key)
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        let _g = write_lock();
        self.inner.delete(&self.name(account))
    }
}

/// 옮기기의 "비었나 확인 → 쓰기"와 사용자의 키 저장·삭제가 끼어들지 않게 하는 프로세스 잠금.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

fn write_lock() -> MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// v0.1.0의 범위 없는 API 키를 이 계정 이름으로 복사한다(이미 있으면 덮지 않음). 옛 항목은 그대로.
pub fn copy_legacy_keys(keys: &dyn KeyStore, key16: &str) -> Result<(), KeyError> {
    let _g = write_lock();
    for name in ACCOUNTS {
        let Some(old) = keys.get(name)? else {
            continue;
        };
        let new = format!("{name}.{key16}");
        if keys.get(&new)?.is_none() {
            keys.set(&new, &old)?;
        }
    }
    Ok(())
}

/// 범위 없는 옛 API 키 항목을 지운다.
pub fn delete_legacy_keys(keys: &dyn KeyStore) -> Result<(), KeyError> {
    ACCOUNTS.iter().try_for_each(|name| keys.delete(name))
}

/// 복사 + 옛 항목 삭제. 옛 데이터 주인 계정에서만 부른다.
pub fn migrate_legacy_keys(keys: &dyn KeyStore, key16: &str) -> Result<(), KeyError> {
    copy_legacy_keys(keys, key16)?;
    delete_legacy_keys(keys)
}

/// 허용 이름: `ACCOUNTS`, `SESSION_ACCOUNT`, `<ACCOUNTS 하나>.<16자 소문자 hex>`,
/// `<ACCOUNTS 하나>.<16자 소문자 hex>.<8자 소문자 hex>`(키 여러 개).
fn valid_name(name: &str) -> bool {
    if ACCOUNTS.contains(&name) || name == SESSION_ACCOUNT {
        return true;
    }
    let hex = |s: &str, n: usize| {
        s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    let mut parts = name.split('.');
    let (Some(acc), Some(key)) = (parts.next(), parts.next()) else {
        return false;
    };
    ACCOUNTS.contains(&acc)
        && hex(key, 16)
        && match (parts.next(), parts.next()) {
            (None, _) => true,
            (Some(id), None) => hex(id, 8),
            _ => false,
        }
}
