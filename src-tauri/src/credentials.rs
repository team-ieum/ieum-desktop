//! API 키 여러 개·구독 계정 목록. 키 값은 키체인 vault(`<공급자>.<계정 키>.<키 id>`)에만,
//! 이름표·기본 여부·끝 4자는 계정 폴더 `credentials.json`에 둔다(값은 절대 넣지 않는다).
//! 옛 키(`<공급자>.<계정 키>`)는 id `default`·이름표 "기본"으로 읽기만 한다(옮겨 쓰지 않음).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ieum_local::ai::{CredentialResolver, KeyStore};
use serde::{Deserialize, Serialize};

use crate::plan_usage::PlanUsage;
use crate::sub_usage::SubscriptionUsage;

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
    /// 계정 추가 [확인] 전(로그인 대기) — 목록·노드 선택에 보이지 않는다.
    pub pending_claude_accounts: Vec<CliAccountMeta>,
    pub pending_codex_accounts: Vec<CliAccountMeta>,
    /// IEUM이 쓰지 않게 숨긴 `system` 계정(`claude`|`codex`). CLI 로그아웃은 하지 않는다.
    pub hidden_system: Vec<String>,
}

/// `begin_cli_account` 응답: 터미널에 붙여 넣을 로그인 명령.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliAccountBegin {
    pub id: String,
    pub command: String,
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

fn cli_kind(k: &str) -> Result<&'static str, String> {
    ["claude", "codex"]
        .into_iter()
        .find(|a| *a == k)
        .ok_or_else(|| format!("알 수 없는 구독이에요: {k}"))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

fn is_hex8(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// (확정 목록, 대기 목록)
fn accounts<'a>(
    c: &'a mut Credentials,
    kind: &str,
) -> (&'a mut Vec<CliAccountMeta>, &'a mut Vec<CliAccountMeta>) {
    if kind == "claude" {
        (&mut c.claude_accounts, &mut c.pending_claude_accounts)
    } else {
        (&mut c.codex_accounts, &mut c.pending_codex_accounts)
    }
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
#[derive(Clone)]
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
        for (kind, list) in [
            ("claude", &mut c.claude_accounts),
            ("codex", &mut c.codex_accounts),
        ] {
            if c.hidden_system.iter().any(|h| h == kind) {
                list.retain(|m| m.id != SYSTEM_ID);
            } else if !list.iter().any(|m| m.id == SYSTEM_ID) {
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
        let written = std::cell::Cell::new(None);
        let r = self.edit(|c| {
            let list = c.api_keys.entry(provider.to_string()).or_default();
            let id = loop {
                let id = new_id();
                if !list.iter().any(|m| m.id == id) {
                    break id;
                }
            };
            let name = self.vault_name(provider, &id);
            self.keys.set(&name, key).map_err(|e| e.to_string())?;
            written.set(Some(name));
            let m = ApiKeyMeta {
                id,
                label,
                is_default: list.is_empty(),
                created_at: crate::store::now(),
                last4: last4(key),
            };
            list.push(m.clone());
            Ok(m)
        });
        // 메타를 못 썼으면 목록에 없는 값이 vault에 남지 않게 되돌린다.
        if r.is_err()
            && let Some(name) = written.take()
        {
            let _ = self.keys.delete(&name);
        }
        r
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

    /// 계정 설정 폴더 `accounts/<계정 키>/<claude|codex>/<id>`. id는 8자 hex만(경로 밖으로 못 나가게).
    fn account_dir(&self, kind: &str, id: &str) -> Result<PathBuf, String> {
        if !is_hex8(id) {
            return Err(MISSING.into());
        }
        Ok(self.root.join(kind).join(id))
    }

    /// 폴더를 만들고 대기 목록에 넣는다(아직 목록엔 안 보임). 명령의 경로는 큰따옴표로 감싼다.
    pub fn begin_cli_account(&self, kind: &str, label: &str) -> Result<CliAccountBegin, String> {
        let kind = cli_kind(kind)?;
        let label = clean_label(label)?;
        self.edit(|c| {
            let (list, pending) = accounts(c, kind);
            let id = loop {
                let id = new_id();
                if !list.iter().chain(pending.iter()).any(|m| m.id == id) {
                    break id;
                }
            };
            let dir = self.account_dir(kind, &id)?;
            let shown = dir.display().to_string();
            // 큰따옴표 안에서도 뜻이 바뀌는 글자가 있으면 붙여 넣을 명령을 만들지 않는다.
            // ASCII가 아니면 claude가 경로를 NFC로 바꿔 해시해 앱이 넘기는 폴더와 어긋날 수 있다.
            if !shown.is_ascii() || shown.contains(['"', '$', '`', '\\', '\n']) {
                return Err("계정 폴더 경로에 쓸 수 없는 글자가 있어요.".into());
            }
            std::fs::create_dir_all(&dir).map_err(|e| format!("계정 폴더를 못 만들었어요: {e}"))?;
            pending.push(CliAccountMeta {
                id: id.clone(),
                label,
                is_default: false,
                created_at: crate::store::now(),
                email: None,
            });
            let command = match kind {
                "claude" => format!("CLAUDE_CONFIG_DIR=\"{shown}\" claude"),
                _ => format!("CODEX_HOME=\"{shown}\" codex login"),
            };
            Ok(CliAccountBegin { id, command })
        })
    }

    /// 대기 중인 계정의 설정 폴더(파일만 읽는다).
    pub fn pending_dir(&self, kind: &str, id: &str) -> Result<PathBuf, String> {
        let kind = cli_kind(kind)?;
        let mut c = self.read_file()?;
        if !accounts(&mut c, kind).1.iter().any(|m| m.id == id) {
            return Err(MISSING.into());
        }
        self.account_dir(kind, id)
    }

    /// 확정된 계정의 설정 폴더. `system` = None(CLI 기본 설정). 파일만 읽는다.
    pub fn cli_dir(&self, kind: &str, id: &str) -> Result<Option<PathBuf>, String> {
        let kind = cli_kind(kind)?;
        let mut c = self.read_file()?;
        if id == SYSTEM_ID {
            // 숨긴 로그인은 없는 계정처럼(다른 계정으로 대신하지 않는다).
            if c.hidden_system.iter().any(|h| h == kind) {
                return Err(MISSING.into());
            }
            return Ok(None);
        }
        let dir = self.account_dir(kind, id)?;
        if !accounts(&mut c, kind).0.iter().any(|m| m.id == id) || !dir.is_dir() {
            return Err(MISSING.into());
        }
        Ok(Some(dir))
    }

    /// 대기 → 목록(이메일과 함께).
    fn finish_cli_account(&self, kind: &str, id: &str, email: String) -> Result<(), String> {
        self.edit(|c| {
            let (list, pending) = accounts(c, kind);
            // [확인]이 겹치면 먼저 끝난 쪽이 이미 옮겼다.
            if list.iter().any(|m| m.id == id) {
                return Ok(());
            }
            let i = pending
                .iter()
                .position(|m| m.id == id)
                .ok_or_else(|| MISSING.to_string())?;
            let mut m = pending.remove(i);
            m.email = Some(email);
            m.is_default = !list.iter().any(|m| m.is_default);
            list.push(m);
            Ok(())
        })
    }

    /// 대기 중인 계정을 버리고 그 폴더를 지운다.
    pub fn cancel_cli_account(&self, kind: &str, id: &str) -> Result<(), String> {
        let kind = cli_kind(kind)?;
        let dir = self.account_dir(kind, id)?;
        self.edit(|c| {
            let pending = accounts(c, kind).1;
            let i = pending
                .iter()
                .position(|m| m.id == id)
                .ok_or_else(|| MISSING.to_string())?;
            pending.remove(i);
            Ok(())
        })?;
        remove_dir(&dir)
    }

    /// `system`(이 Mac의 CLI 로그인)을 IEUM에서 숨기거나 되살린다. 숨기면 목록·노드 선택에서 빠지고,
    /// 기본이었으면 남은 것 중 가장 먼저 만든 것이 기본. CLI 로그아웃은 하지 않는다.
    pub fn set_system_hidden(&self, kind: &str, hidden: bool) -> Result<(), String> {
        let kind = cli_kind(kind)?;
        self.edit(|c| {
            let was = c.hidden_system.iter().any(|h| h == kind);
            if hidden && !was {
                remove(accounts(c, kind).0, SYSTEM_ID)?;
                c.hidden_system.push(kind.into());
            } else if !hidden {
                c.hidden_system.retain(|h| h != kind);
            }
            Ok(())
        })
    }

    /// API 키는 vault 값도, 구독 계정은 설정 폴더도 지운다. `system` 계정은 지울 수 없다.
    pub fn delete(&self, kind: &str, id: &str) -> Result<(), String> {
        if matches!(kind, "claude" | "codex") {
            if id == SYSTEM_ID {
                return Err("이 Mac의 로그인은 지울 수 없어요.".into());
            }
            let dir = self.account_dir(kind, id)?;
            self.edit(|c| remove(accounts(c, kind).0, id))?;
            return remove_dir(&dir);
        }
        let p = api_provider(kind)?;
        self.edit(|c| {
            let list = c.api_keys.entry(p.into()).or_default();
            find(list, id)?;
            self.keys
                .delete(&self.vault_name(p, id))
                .map_err(|e| e.to_string())?;
            remove(list, id)
        })
    }
}

impl CredentialResolver for AccountCredentials {
    /// 고른 id가 목록에 없거나 vault에 값이 없으면 `MISSING`. 비우면 기본(없으면 옛 이름 — 공급원이 "키가 없어요").
    fn api_key_name(&self, provider: &str, credential: Option<&str>) -> Result<String, String> {
        let provider = api_provider(provider)?;
        let (c, keychain_error) = {
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
        // 옛 키는 키체인을 읽어야 목록에 보인다 — 못 읽었으면 없는 키가 아니라 키체인 오류.
        if id == LEGACY_ID
            && let Some(e) = keychain_error
        {
            return Err(format!("키체인에서 키를 읽지 못했어요: {e}"));
        }
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

    /// 고른 계정(비우면 기본 계정)의 설정 폴더. `system`이면 None. 없는 id는 `MISSING`.
    fn claude_config_dir(&self, credential: Option<&str>) -> Result<Option<PathBuf>, String> {
        let id = match credential {
            Some(id) => id.to_string(),
            None => self
                .read_file()?
                .claude_accounts
                .into_iter()
                .find(|m| m.is_default)
                .map_or(SYSTEM_ID.into(), |m| m.id),
        };
        self.cli_dir("claude", &id)
    }

    /// `claude_config_dir`와 같은 규칙 — codex 계정의 `CODEX_HOME`.
    fn codex_home(&self, credential: Option<&str>) -> Result<Option<PathBuf>, String> {
        let id = match credential {
            Some(id) => id.to_string(),
            None => self
                .read_file()?
                .codex_accounts
                .into_iter()
                .find(|m| m.is_default)
                .map_or(SYSTEM_ID.into(), |m| m.id),
        };
        self.cli_dir("codex", &id)
    }
}

/// 계정 추가 [확인]: 그 폴더 기준으로 사용량·이메일을 새로 읽고(허용 창이 뜰 수 있음),
/// 이메일이 보이면 목록에 넣는다. 아니면 대기 그대로(`not_logged_in` 등을 그대로 돌려준다).
pub async fn confirm_cli_account(
    c: Arc<AccountCredentials>,
    usage: &SubscriptionUsage,
    kind: &str,
    id: &str,
) -> Result<PlanUsage, String> {
    let (k, i) = (kind.to_string(), id.to_string());
    let c2 = c.clone();
    let dir = blocking(move || c2.pending_dir(&k, &i)).await?;
    let u = usage.refresh(kind, Some(dir)).await?;
    if let Some(email) = u.account_email.clone() {
        let (k, i) = (kind.to_string(), id.to_string());
        // 목록 저장은 옛 키를 키체인에서 읽는다 — 메인 스레드 밖에서.
        blocking(move || c.finish_cli_account(&k, &i, email)).await?;
    }
    Ok(u)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// 없으면 그대로 둔다.
fn remove_dir(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("계정 폴더를 지우지 못했어요: {e}"))
        }
        _ => Ok(()),
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
