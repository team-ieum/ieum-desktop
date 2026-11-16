//! GitHub 연결(gh CLI). gh 출력 읽기·연결 상태 판정.

use serde::{Deserialize, Serialize};

/// 받는 gh 최소 버전(`auth status --json` 도입).
pub const MIN_GH: (u32, u32, u32) = (2, 81, 0);

/// `gh version 2.103.3 (2026-10-26)` → (2, 103, 3).
pub fn parse_gh_version(line: &str) -> Option<(u32, u32, u32)> {
    let ver = line.trim().strip_prefix("gh version ")?;
    let mut parts = ver.split_whitespace().next()?.split('.');
    let mut num = || parts.next()?.parse().ok();
    Some((num()?, num()?, num()?))
}

/// 줄 안 첫 `XXXX-XXXX`(대문자·숫자, 앞뒤가 영숫자가 아님) device 코드.
pub fn parse_device_code(line: &str) -> Option<String> {
    let b = line.as_bytes();
    let code = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit();
    (0..(b.len() + 1).saturating_sub(9))
        .find(|&i| {
            let w = &b[i..i + 9];
            w[..4].iter().all(|&c| code(c))
                && w[4] == b'-'
                && w[5..].iter().all(|&c| code(c))
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
                && b.get(i + 9).is_none_or(|c| !c.is_ascii_alphanumeric())
        })
        .map(|i| line[i..i + 9].to_string())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GhAccount {
    pub login: String,
    pub active: bool,
    pub state: String,
    #[serde(rename = "tokenSource")]
    pub token_source: String,
}

/// `gh auth status --json hosts`의 github.com 계정들. 호스트가 없으면 빈 Vec.
pub fn parse_auth_status(json: &str) -> Result<Vec<GhAccount>, String> {
    #[derive(Deserialize)]
    struct Status {
        hosts: std::collections::HashMap<String, Vec<GhAccount>>,
    }
    let mut s: Status = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(s.hosts.remove("github.com").unwrap_or_default())
}

/// gh 로그인 상태. `gh auth status` 종료 1 → `LoggedOut`.
#[derive(Debug, Clone, PartialEq)]
pub enum GhAuth {
    LoggedOut,
    Accounts(Vec<GhAccount>),
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GithubStatus {
    NotInstalled,
    Outdated {
        version: String,
    },
    NotConnected,
    /// 기록 있음 + gh 로그아웃 또는 state≠"success".
    NeedsLogin {
        recorded: String,
    },
    AccountChanged {
        recorded: String,
        current: String,
    },
    /// `env_token` = 토큰이 환경변수(GH_TOKEN·GITHUB_TOKEN)에서 옴.
    Connected {
        login: String,
        env_token: bool,
    },
}

/// gh 설치·버전 확인을 통과한 뒤의 상태. `recorded` = IEUM 연결 기록 login.
pub fn derive_status(recorded: Option<&str>, auth: &GhAuth) -> GithubStatus {
    let Some(recorded) = recorded else {
        return GithubStatus::NotConnected;
    };
    let needs = || GithubStatus::NeedsLogin {
        recorded: recorded.into(),
    };
    let GhAuth::Accounts(accounts) = auth else {
        return needs();
    };
    let Some(a) = accounts.iter().find(|a| a.active) else {
        return needs();
    };
    if a.login != recorded {
        GithubStatus::AccountChanged {
            recorded: recorded.into(),
            current: a.login.clone(),
        }
    } else if a.state != "success" {
        needs()
    } else {
        GithubStatus::Connected {
            login: a.login.clone(),
            env_token: matches!(a.token_source.as_str(), "GH_TOKEN" | "GITHUB_TOKEN"),
        }
    }
}
