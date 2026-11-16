//! gh 출력 읽기·연결 상태 판정(순수 함수).

use ieum_desktop_lib::github::{
    GhAccount, GhAuth, GithubStatus, MIN_GH, derive_status, parse_auth_status, parse_device_code,
    parse_gh_version,
};

fn acct(login: &str, active: bool, state: &str, source: &str) -> GhAccount {
    GhAccount {
        login: login.into(),
        active,
        state: state.into(),
        token_source: source.into(),
    }
}

#[test]
fn parse_version_cases() {
    assert_eq!(
        parse_gh_version("gh version 2.103.3 (2026-10-26)"),
        Some((2, 103, 3))
    );
    assert_eq!(parse_gh_version("gh version 2.81.0"), Some((2, 81, 0)));
    assert_eq!(parse_gh_version("nope"), None);
    assert!((2, 80, 9) < MIN_GH);
    assert!((2, 81, 0) >= MIN_GH);
}

#[test]
fn parse_code_from_gh_stderr() {
    assert_eq!(
        parse_device_code("! One-time code (C0FE-571F) copied to clipboard").as_deref(),
        Some("C0FE-571F")
    );
    assert_eq!(
        parse_device_code(
            "Open this URL to continue in your web browser: https://github.com/login/device"
        ),
        None
    );
    assert_eq!(parse_device_code("XC0FE-571F"), None);
}

#[test]
fn parse_status_json() {
    let json = r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"hodoon","tokenSource":"keyring","scopes":"gist, read:org, repo, workflow","gitProtocol":"https"}]}}"#;
    let v = parse_auth_status(json).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].login, "hodoon");
    assert!(v[0].active);
    assert_eq!(v[0].state, "success");
    assert!(parse_auth_status(r#"{"hosts":{}}"#).unwrap().is_empty());
    assert!(parse_auth_status("{nope").is_err());
}

#[test]
fn derive_not_connected() {
    let auth = GhAuth::Accounts(vec![acct("hodoon", true, "success", "keyring")]);
    assert_eq!(derive_status(None, &auth), GithubStatus::NotConnected);
}

#[test]
fn derive_connected() {
    let auth = GhAuth::Accounts(vec![acct("hodoon", true, "success", "keyring")]);
    assert_eq!(
        derive_status(Some("hodoon"), &auth),
        GithubStatus::Connected {
            login: "hodoon".into(),
            env_token: false
        }
    );
}

#[test]
fn derive_env_token() {
    let auth = GhAuth::Accounts(vec![acct("hodoon", true, "success", "GH_TOKEN")]);
    assert_eq!(
        derive_status(Some("hodoon"), &auth),
        GithubStatus::Connected {
            login: "hodoon".into(),
            env_token: true
        }
    );
}

#[test]
fn derive_account_changed() {
    let auth = GhAuth::Accounts(vec![
        acct("hodoon", false, "success", "keyring"),
        acct("other", true, "success", "keyring"),
    ]);
    assert_eq!(
        derive_status(Some("hodoon"), &auth),
        GithubStatus::AccountChanged {
            recorded: "hodoon".into(),
            current: "other".into()
        }
    );
}

#[test]
fn derive_needs_login() {
    let needs = GithubStatus::NeedsLogin {
        recorded: "hodoon".into(),
    };
    assert_eq!(derive_status(Some("hodoon"), &GhAuth::LoggedOut), needs);
    let auth = GhAuth::Accounts(vec![acct("hodoon", true, "error", "keyring")]);
    assert_eq!(derive_status(Some("hodoon"), &auth), needs);
}

#[test]
fn status_serializes_camel() {
    let s = GithubStatus::Connected {
        login: "hodoon".into(),
        env_token: false,
    };
    assert_eq!(
        serde_json::to_string(&s).unwrap(),
        r#"{"kind":"connected","login":"hodoon","envToken":false}"#
    );
}
