//! API 키 저장 — macOS 키체인(`keyring`). 서비스 `com.ieum.desktop`, 계정 = `<anthropic|openai|gemini>.<계정 키>`
//! (범위 없는 이름은 v0.1.0 옛 키 — 옮기기 전용).
//! 키는 `ApiKey`로만 들고 다닌다 — `Debug`가 가린다.

use std::fmt;
use std::sync::Arc;

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

/// macOS 키체인.
pub struct Keychain;

impl KeyStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        get(account)
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        set(account, key)
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        delete(account)
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

    fn name(&self, account: &str) -> String {
        if account == SESSION_ACCOUNT {
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
        self.inner.set(&self.name(account), key)
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        self.inner.delete(&self.name(account))
    }
}

/// v0.1.0의 범위 없는 API 키를 이 계정으로 옮긴다(이미 있으면 덮지 않음). 옛 항목은 지운다.
/// 옛 데이터 주인 계정에서만 부른다. 다시 불러도 안전하다.
pub fn migrate_legacy_keys(keys: &dyn KeyStore, key16: &str) -> Result<(), KeyError> {
    let scoped = format!(".{key16}");
    for name in ACCOUNTS {
        let Some(old) = keys.get(name)? else {
            continue;
        };
        let new = format!("{name}{scoped}");
        if keys.get(&new)?.is_none() {
            keys.set(&new, &old)?;
        }
        keys.delete(name)?;
    }
    Ok(())
}

/// 허용 이름: `ACCOUNTS`, `SESSION_ACCOUNT`, `<ACCOUNTS 하나>.<16자 소문자 hex>`.
fn valid_name(name: &str) -> bool {
    if ACCOUNTS.contains(&name) || name == SESSION_ACCOUNT {
        return true;
    }
    name.split_once('.').is_some_and(|(acc, key)| {
        ACCOUNTS.contains(&acc)
            && key.len() == 16
            && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn entry(provider: &str) -> Result<keyring::Entry, KeyError> {
    if !valid_name(provider) {
        return Err(KeyError(format!("알 수 없는 공급원이에요: {provider}")));
    }
    keyring::Entry::new(SERVICE, provider).map_err(|e| KeyError(format!("키체인 오류: {e}")))
}

/// 키 저장(앞뒤 공백은 뺀다).
pub fn set(provider: &str, key: &str) -> Result<(), KeyError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(KeyError("키가 비었어요.".into()));
    }
    entry(provider)?
        .set_password(key)
        .map_err(|e| KeyError(format!("키체인에 저장하지 못했어요: {e}")))
}

/// 없으면 `Ok(None)`. 접근 거부·사용자 취소 같은 키체인 오류는 삼키지 않고 `Err`.
pub fn get(provider: &str) -> Result<Option<String>, KeyError> {
    match entry(provider)?.get_password() {
        Ok(k) => Ok(Some(k.trim().to_string()).filter(|k| !k.is_empty())),
        Err(keyring::Error::NoEntry) => Ok(None),
        // BadEncoding은 키 바이트를 담으므로 `{e}`를 쓰지 않는다.
        Err(keyring::Error::BadEncoding(_)) => Err(KeyError(
            "키체인의 키를 읽을 수 없어요(인코딩 오류).".into(),
        )),
        Err(e) => Err(KeyError(format!("키체인에서 키를 읽지 못했어요: {e}"))),
    }
}

/// 키 삭제. 없던 키면 성공으로 본다.
pub fn delete(provider: &str) -> Result<(), KeyError> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(KeyError(format!("키체인에서 지우지 못했어요: {e}"))),
    }
}
