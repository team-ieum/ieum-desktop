//! API 키 여러 개·구독 계정 목록. 키 값은 키체인 vault(`<공급자>.<계정 키>.<키 id>`)에만,
//! 이름표·기본 여부·끝 4자는 계정 폴더 `credentials.json`에 둔다(값은 절대 넣지 않는다).
//! 옛 키(`<공급자>.<계정 키>`)는 id `default`·이름표 "기본"으로 읽기만 한다(옮겨 쓰지 않음).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ieum_local::ai::{CredentialResolver, KeyStore};
use serde::{Deserialize, Serialize};

pub use ieum_local::ai::CREDENTIAL_MISSING as MISSING;

pub const API_PROVIDERS: [&str; 3] = ["anthropic", "openai", "gemini"];
/// 옛 키의 id.
const LEGACY_ID: &str = "default";
/// CLI 기본 설정 폴더(이 Mac의 로그인) 계정 id.
const SYSTEM_ID: &str = "system";
const FILE: &str = "credentials.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyMeta {
    pub id: String,
    pub label: String,
    pub is_default: bool,
    pub created_at: String,
    /// 키 끝 4자(8자 미만 키는 빈 값).
    pub last4: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliAccountMeta {
    pub id: String,
    pub label: String,
    pub is_default: bool,
    pub created_at: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Credentials {
    pub api_keys: BTreeMap<String, Vec<ApiKeyMeta>>,
    pub claude_accounts: Vec<CliAccountMeta>,
    pub codex_accounts: Vec<CliAccountMeta>,
}

/// `list_credentials` 응답. 키체인을 못 읽으면 옛 키는 빠지고 `keychain_error`에 까닭.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsView {
    pub api_keys: BTreeMap<String, Vec<ApiKeyMeta>>,
    pub claude_accounts: Vec<CliAccountMeta>,
    pub codex_accounts: Vec<CliAccountMeta>,
    pub keychain_error: Option<String>,
}

/// 기본·이름표·만든 시각을 함께 다루려고.
trait Meta {
    fn id(&self) -> &str;
    fn created(&self) -> &str;
    fn label_mut(&mut self) -> &mut String;
    fn default_mut(&mut self) -> &mut bool;
}

macro_rules! impl_meta {
    ($t:ty) => {
        impl Meta for $t {
            fn id(&self) -> &str {
                &self.id
            }
            fn created(&self) -> &str {
                &self.created_at
            }
            fn label_mut(&mut self) -> &mut String {
                &mut self.label
            }
            fn default_mut(&mut self) -> &mut bool {
                &mut self.is_default
            }
        }
    };
}
impl_meta!(ApiKeyMeta);
impl_meta!(CliAccountMeta);

fn find<'a, T: Meta>(list: &'a mut [T], id: &str) -> Result<&'a mut T, String> {
    list.iter_mut()
        .find(|m| m.id() == id)
        .ok_or_else(|| MISSING.to_string())
}

fn set_default<T: Meta>(list: &mut [T], id: &str) -> Result<(), String> {
    find(list, id)?;
    for m in list {
        let is = m.id() == id;
        *m.default_mut() = is;
    }
    Ok(())
}

/// 지우고, 기본이었으면 남은 것 중 가장 먼저 만든 것을 기본으로.
fn remove<T: Meta>(list: &mut Vec<T>, id: &str) -> Result<(), String> {
    let i = list
        .iter()
        .position(|m| m.id() == id)
        .ok_or_else(|| MISSING.to_string())?;
    let mut gone = list.remove(i);
    if *gone.default_mut()
        && let Some(oldest) = list.iter_mut().min_by(|a, b| a.created().cmp(b.created()))
    {
        *oldest.default_mut() = true;
    }
    Ok(())
}

fn clean_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("이름표를 넣어 주세요.".into());
    }
    Ok(label.chars().take(40).collect())
}

fn last4(key: &str) -> String {
    let n = key.chars().count();
    if n < 8 {
        return String::new();
    }
    key.chars().skip(n - 4).collect()
}

fn api_provider(p: &str) -> Result<&'static str, String> {
    API_PROVIDERS
        .into_iter()
        .find(|a| *a == p)
        .ok_or_else(|| format!("알 수 없는 공급원이에요: {p}"))
}

/// 파일 읽기·쓰기를 한 프로세스 안에서 차례로.
static LOCK: Mutex<()> = Mutex::new(());

/// 계정 하나의 키·계정 목록. 키 저장소는 범위 없는 vault(`anthropic.<계정 키>...` 전체 이름으로 접근).
pub struct AccountCredentials {
    root: PathBuf,
    key16: String,
    keys: Arc<dyn KeyStore>,
}

impl AccountCredentials {
    /// `root` = 계정 폴더(`accounts/<계정 키>`).
    pub fn new(root: PathBuf, key16: &str, keys: Arc<dyn KeyStore>) -> Self {
        AccountCredentials {
            root,
            key16: key16.to_string(),
            keys,
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(FILE)
    }

    /// 키 저장소 이름. 옛 키 id면 범위 이름 그대로.
    fn vault_name(&self, provider: &str, id: &str) -> String {
        if id == LEGACY_ID {
            format!("{provider}.{}", self.key16)
        } else {
            format!("{provider}.{}.{id}", self.key16)
        }
    }

    fn read_file(&self) -> Result<Credentials, String> {
        read_json(&self.path())
    }

    /// 파일 + 옛 키 + `system` 계정. 옛 키를 못 읽으면 둘째 값에 까닭(값은 담지 않는다).
    fn merged(&self) -> Result<(Credentials, Option<String>), String> {
        let mut c = self.read_file()?;
        let mut keychain_error = None;
        for p in API_PROVIDERS {
            let list = c.api_keys.entry(p.to_string()).or_default();
            if list.iter().any(|m| m.id == LEGACY_ID) {
                continue;
            }
            match self.keys.get(&self.vault_name(p, LEGACY_ID)) {
                Ok(Some(v)) => {
                    let is_default = !list.iter().any(|m| m.is_default);
                    list.insert(
                        0,
                        ApiKeyMeta {
                            id: LEGACY_ID.into(),
                            label: "기본".into(),
                            is_default,
                            created_at: String::new(),
                            last4: last4(v.trim()),
                        },
                    );
                }
                Ok(None) => {}
                Err(e) => keychain_error = Some(e.to_string()),
            }
        }
        for list in [&mut c.claude_accounts, &mut c.codex_accounts] {
            if !list.iter().any(|m| m.id == SYSTEM_ID) {
                let is_default = !list.iter().any(|m| m.is_default);
                list.insert(
                    0,
                    CliAccountMeta {
                        id: SYSTEM_ID.into(),
                        label: "이 Mac의 로그인".into(),
                        is_default,
                        created_at: String::new(),
                        email: None,
                    },
                );
            }
        }
        Ok((c, keychain_error))
    }

    /// 고칠 때: 키체인을 못 읽었으면 옛 키를 빠뜨린 채 저장하지 않게 멈춘다.
    fn edit<R>(&self, f: impl FnOnce(&mut Credentials) -> Result<R, String>) -> Result<R, String> {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut c, err) = self.merged()?;
        if let Some(e) = err {
            return Err(format!("키체인을 읽지 못했어요: {e}"));
        }
        let r = f(&mut c)?;
        crate::store::write_atomic(&self.path(), &c)?;
        Ok(r)
    }

    pub fn list(&self) -> CredentialsView {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (c, keychain_error) = match self.merged() {
            Ok(x) => x,
            Err(e) => (Credentials::default(), Some(e)),
        };
        CredentialsView {
            api_keys: c.api_keys,
            claude_accounts: c.claude_accounts,
            codex_accounts: c.codex_accounts,
            keychain_error,
        }
    }

    /// 첫 키면 기본. 값은 vault에, 메타엔 끝 4자만.
    pub fn add_api_key(
        &self,
        provider: &str,
        label: &str,
        key: &str,
    ) -> Result<ApiKeyMeta, String> {
        let provider = api_provider(provider)?;
        let label = clean_label(label)?;
        let key = key.trim();
        if key.is_empty() {
            return Err("키가 비었어요.".into());
        }
        self.edit(|c| {
            let list = c.api_keys.entry(provider.to_string()).or_default();
            let id = loop {
                let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
                if !list.iter().any(|m| m.id == id) {
                    break id;
                }
            };
            self.keys
                .set(&self.vault_name(provider, &id), key)
                .map_err(|e| e.to_string())?;
            let m = ApiKeyMeta {
                id,
                label,
                is_default: list.is_empty(),
                created_at: crate::store::now(),
                last4: last4(key),
            };
            list.push(m.clone());
            Ok(m)
        })
    }

    /// `kind` = `anthropic|openai|gemini|claude|codex`.
    pub fn rename(&self, kind: &str, id: &str, label: &str) -> Result<(), String> {
        let label = clean_label(label)?;
        self.edit(|c| {
            match kind {
                "claude" => *find(&mut c.claude_accounts, id)?.label_mut() = label,
                "codex" => *find(&mut c.codex_accounts, id)?.label_mut() = label,
                p => {
                    *find(c.api_keys.entry(api_provider(p)?.into()).or_default(), id)?.label_mut() =
                        label
                }
            }
            Ok(())
        })
    }

    pub fn set_default(&self, kind: &str, id: &str) -> Result<(), String> {
        self.edit(|c| match kind {
            "claude" => set_default(&mut c.claude_accounts, id),
            "codex" => set_default(&mut c.codex_accounts, id),
            p => set_default(c.api_keys.entry(api_provider(p)?.into()).or_default(), id),
        })
    }

    /// API 키는 vault 값도 지운다. `system` 계정은 지울 수 없다.
    pub fn delete(&self, kind: &str, id: &str) -> Result<(), String> {
        if id == SYSTEM_ID && matches!(kind, "claude" | "codex") {
            return Err("이 Mac의 로그인은 지울 수 없어요.".into());
        }
        self.edit(|c| match kind {
            "claude" => remove(&mut c.claude_accounts, id),
            "codex" => remove(&mut c.codex_accounts, id),
            p => {
                let p = api_provider(p)?;
                let list = c.api_keys.entry(p.into()).or_default();
                find(list, id)?;
                self.keys
                    .delete(&self.vault_name(p, id))
                    .map_err(|e| e.to_string())?;
                remove(list, id)
            }
        })
    }
}

impl CredentialResolver for AccountCredentials {
    /// 고른 id가 목록에 없거나 vault에 값이 없으면 `MISSING`. 비우면 기본(없으면 옛 이름 — 공급원이 "키가 없어요").
    fn api_key_name(&self, provider: &str, credential: Option<&str>) -> Result<String, String> {
        let provider = api_provider(provider)?;
        let (c, _) = {
            let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            self.merged()?
        };
        let list = c.api_keys.get(provider).map(Vec::as_slice).unwrap_or(&[]);
        let Some(id) = credential else {
            let id = list
                .iter()
                .find(|m| m.is_default)
                .map_or(LEGACY_ID, |m| &m.id);
            return Ok(self.vault_name(provider, id));
        };
        if !list.iter().any(|m| m.id == id) {
            return Err(MISSING.into());
        }
        let name = self.vault_name(provider, id);
        match self.keys.get(&name) {
            Ok(Some(_)) => Ok(name),
            Ok(None) => Err(MISSING.into()),
            Err(e) => Err(format!("키체인에서 키를 읽지 못했어요: {e}")),
        }
    }

    /// Task 1: `system`(CLI 기본 설정)만.
    fn claude_config_dir(&self, credential: Option<&str>) -> Result<Option<PathBuf>, String> {
        match credential {
            None | Some(SYSTEM_ID) => Ok(None),
            Some(_) => Err(MISSING.into()),
        }
    }
}

/// 없는 파일은 빈 목록.
fn read_json(path: &Path) -> Result<Credentials, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Credentials::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
