//! 설정 화면용 명령 — 키체인 저장 여부(값 없이)와 "웹에서 수정하기" 주소.

use std::collections::HashMap;
use std::sync::Mutex;

use ieum_desktop_lib::commands::{KeyState, key_states, web_editor_url};
use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::KeyError;

/// `openai`만 키체인 접근 오류.
#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl KeyStore for FakeKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        if account == "openai" {
            return Err(KeyError("잠김".into()));
        }
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

#[test]
fn keychain_status_reports_presence_without_values() {
    let keys = FakeKeys::default();
    keys.set("anthropic", "sk-secret-123").unwrap();
    keys.set("ieum-session", "{\"accessToken\":\"tok-secret\"}")
        .unwrap();

    let s = key_states(&keys);
    assert_eq!(s["anthropic"], KeyState::Stored);
    assert_eq!(s["openai"], KeyState::Error);
    assert_eq!(s["gemini"], KeyState::Missing);
    assert_eq!(s["ieum-session"], KeyState::Stored);

    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(
        json,
        r#"{"anthropic":"stored","gemini":"missing","ieum-session":"stored","openai":"error"}"#
    );
    assert!(!json.contains("secret"));
}

#[test]
fn web_editor_url_only_for_allowed_servers() {
    assert_eq!(
        web_editor_url("https://green.kiacms.co.kr/").unwrap(),
        "https://green.kiacms.co.kr/main"
    );
    assert_eq!(
        web_editor_url("http://localhost:8080").unwrap(),
        "http://localhost:8080/main"
    );
    assert!(web_editor_url("https://evil.example").is_err());
}
