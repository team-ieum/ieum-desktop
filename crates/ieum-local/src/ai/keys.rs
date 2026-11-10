//! API 키 저장 — macOS 키체인(`keyring`). 서비스 `com.ieum.desktop`, 계정 = `anthropic`|`openai`|`gemini`.
//! 키는 `ApiKey`로만 들고 다닌다 — `Debug`가 가린다.

use std::fmt;

const SERVICE: &str = "com.ieum.desktop";
const ACCOUNTS: [&str; 3] = ["anthropic", "openai", "gemini"];

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

/// 키를 꺼내는 곳. 운영은 `Keychain`, 테스트는 가짜를 주입한다.
pub trait KeyStore: Send + Sync {
    /// 계정(`anthropic` 등)의 키. 없으면 None.
    fn get(&self, account: &str) -> Option<String>;
}

/// macOS 키체인.
pub struct Keychain;

impl KeyStore for Keychain {
    fn get(&self, account: &str) -> Option<String> {
        get(account)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct KeyError(String);

fn entry(provider: &str) -> Result<keyring::Entry, KeyError> {
    if !ACCOUNTS.contains(&provider) {
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

pub fn get(provider: &str) -> Option<String> {
    let k = entry(provider).ok()?.get_password().ok()?;
    Some(k.trim().to_string()).filter(|k| !k.is_empty())
}

/// 키 삭제. 없던 키면 성공으로 본다.
pub fn delete(provider: &str) -> Result<(), KeyError> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(KeyError(format!("키체인에서 지우지 못했어요: {e}"))),
    }
}
