//! 로그인·로그아웃이 활성 계정 슬롯을 움직인다. 가짜 BE·가짜 키·임시 폴더만.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_desktop_lib::account::AccountId;
use ieum_desktop_lib::active::{Active, BaseDeps};
use ieum_desktop_lib::commands::{account_of, login_and_activate, logout_and_deactivate};
use ieum_desktop_lib::store::{Store, StoredWorkflow};
use ieum_desktop_lib::supervisor::WatchState;
use ieum_desktop_lib::web::BeClient;
use ieum_local::WatchTiming;
use ieum_local::ai::keys::KeyError;
use ieum_local::ai::{AiRunner, KeyStore};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

fn new_active(root: &Path, settings: &Store, keys: Arc<FakeKeys>) -> Active {
    Active::new(BaseDeps {
        app_data: root.join("data"),
        settings: settings.clone(),
        home: root.to_path_buf(),
        trash_dir: root.join("trash"),
        keys,
        emit: Arc::new(|_| {}),
        timing: WatchTiming {
            stable_interval: Duration::from_millis(100),
            repeat_window: Duration::from_secs(30),
            recent_window: Duration::from_secs(10),
        },
        recheck: Duration::from_secs(30),
        ai_factory: Arc::new(|_| Arc::new(AiRunner::empty())),
    })
}

/// 로그인엔 토큰, 그 밖(로그아웃)엔 200. 요청 하나 = 연결 하나.
async fn serve() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let data = if head.starts_with("POST /api/v1/auth/login ") {
                    json!({"accessToken": "acc-1", "refreshToken": "ref-1", "expiresIn": 3600})
                } else {
                    json!(null)
                };
                let body = json!({"success": true, "data": data, "message": "OK", "code": "OK"})
                    .to_string();
                let res = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(res.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    base
}

#[tokio::test(flavor = "multi_thread")]
async fn login_activates_and_logout_deactivates() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let inbox = root.join("in");
    fs::create_dir_all(&inbox).unwrap();
    fs::create_dir_all(root.join("trash")).unwrap();
    let settings = Store::new(root.join("data"));
    let keys = Arc::new(FakeKeys::default());
    let active = new_active(&root, &settings, keys.clone());
    let base = serve().await;
    let client = BeClient::new(&base, keys.clone()).unwrap();

    assert_eq!(account_of(&active).await, None);
    let acc = login_and_activate(&active, &client, "a@x.com", "pw-secret-9")
        .await
        .unwrap();
    let id = AccountId {
        email: "a@x.com".into(),
        server_url: base.clone(),
    };
    assert_eq!(acc.email, id.email);
    assert_eq!(account_of(&active).await, Some(id.clone()));
    assert_eq!(settings.get_settings().unwrap().current_account, Some(id));

    let sup = active.sup().await.unwrap();
    sup.save_workflow(StoredWorkflow {
        id: String::new(),
        name: "감시".into(),
        enabled: true,
        workflow: serde_json::from_value(json!({
            "nodes": [{"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": inbox}}],
            "edges": []
        }))
        .unwrap(),
        updated_at: String::new(),
    })
    .unwrap();
    assert_eq!(sup.list_workflows().unwrap()[0].watch, WatchState::Watching);

    logout_and_deactivate(&active, &client).await.unwrap();
    assert_eq!(account_of(&active).await, None);
    assert_eq!(sup.list_workflows().unwrap()[0].watch, WatchState::Off);
    assert!(client.account().is_none());
}

/// 슬롯이 비었어도 저장된 계정이 있으면 그 계정(네트워크 안 씀).
#[tokio::test]
async fn account_of_falls_back_to_saved() {
    let t = tempfile::tempdir().unwrap();
    let settings = Store::new(t.path().join("data"));
    let id = AccountId {
        email: "a@x.com".into(),
        server_url: "http://127.0.0.1:1".into(),
    };
    let mut s = settings.get_settings().unwrap();
    s.current_account = Some(id.clone());
    settings.set_settings(&s).unwrap();
    let active = new_active(t.path(), &settings, Arc::new(FakeKeys::default()));
    assert_eq!(account_of(&active).await, Some(id));
}
