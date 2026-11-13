//! 설정 화면용 명령 — 키 목록(값 없이)과 "웹에서 수정하기" 주소.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ieum_desktop_lib::commands::{provider_console_url, web_editor_url, web_workflow_url};
use ieum_desktop_lib::credentials::AccountCredentials;
use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::KeyError;

const K16: &str = "aaaaaaaaaaaaaaaa";

/// `openai.*`만 키체인 접근 오류.
#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl KeyStore for FakeKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        if account.starts_with("openai") {
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
    let t = tempfile::tempdir().unwrap();
    let keys = Arc::new(FakeKeys::default());
    keys.set(&format!("anthropic.{K16}"), "sk-secret-123")
        .unwrap();
    keys.set("ieum-session", "{\"accessToken\":\"tok-secret\"}")
        .unwrap();

    let v = AccountCredentials::new(t.path().to_path_buf(), K16, keys).list();
    assert_eq!(v.api_keys["anthropic"].len(), 1);
    assert!(v.api_keys["openai"].is_empty());
    assert!(v.api_keys["gemini"].is_empty());
    assert_eq!(v.keychain_error.as_deref(), Some("잠김"));

    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("secret"), "{json}");
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

#[test]
fn web_workflow_url_rejects_non_uuid() {
    const ID: &str = "abcdef01-1111-4111-8111-111111111111";
    assert_eq!(
        web_workflow_url("https://green.kiacms.co.kr", ID).unwrap(),
        format!("https://green.kiacms.co.kr/workflow/{ID}")
    );
    for bad in [
        "../x",
        "",
        "abcdef01111141118111111111111111",
        &ID.to_uppercase(),
    ] {
        assert!(
            web_workflow_url("https://green.kiacms.co.kr", bad).is_err(),
            "{bad}"
        );
    }
    assert!(web_workflow_url("https://evil.example", ID).is_err());
}

#[test]
fn provider_console_url_only_known() {
    assert_eq!(
        provider_console_url("anthropic").unwrap(),
        "https://console.anthropic.com/settings/keys"
    );
    assert_eq!(
        provider_console_url("openai").unwrap(),
        "https://platform.openai.com/api-keys"
    );
    assert_eq!(
        provider_console_url("gemini").unwrap(),
        "https://aistudio.google.com/apikey"
    );
    for bad in ["", "Anthropic", "https://evil.example", "claude", "gemini "] {
        assert!(provider_console_url(bad).is_err(), "{bad}");
    }
}
