//! gh 출력 읽기·연결 상태 판정, gh 찾기·상태 명령·연결 기록.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ieum_desktop_lib::github::{
    GhAccount, GhAuth, GhFound, GithubStatus, InstallRoute, LOGIN_CMD_ARGS, MIN_GH, checksum_for,
    choose_route, clear_record, derive_status, fetch, gh_auth, install_from_zip, parse_auth_status,
    parse_device_code, parse_gh_version, pick_gh, read_record, release_assets, run_login,
    write_record,
};
use sha2::{Digest, Sha256};

const RELEASE_JSON: &str = r#"{"tag_name":"v2.104.0","assets":[
 {"name":"gh_2.104.0_checksums.txt","browser_download_url":"https://github.com/cli/cli/releases/download/v2.104.0/gh_2.104.0_checksums.txt"},
 {"name":"gh_2.104.0_macOS_amd64.zip","browser_download_url":"https://github.com/cli/cli/releases/download/v2.104.0/gh_2.104.0_macOS_amd64.zip"},
 {"name":"gh_2.104.0_macOS_arm64.zip","browser_download_url":"https://github.com/cli/cli/releases/download/v2.104.0/gh_2.104.0_macOS_arm64.zip"},
 {"name":"gh_2.104.0_macOS_universal.pkg","browser_download_url":"https://github.com/cli/cli/releases/download/v2.104.0/gh_2.104.0_macOS_universal.pkg"}
]}"#;

#[test]
fn release_assets_pick_arch() {
    let base = "https://github.com/cli/cli/releases/download/v2.104.0/";
    let (tag, zip_url, zip_name, sums_url) = release_assets(RELEASE_JSON, "aarch64").unwrap();
    assert_eq!(tag, "v2.104.0");
    assert_eq!(zip_name, "gh_2.104.0_macOS_arm64.zip");
    assert_eq!(zip_url, format!("{base}gh_2.104.0_macOS_arm64.zip"));
    assert_eq!(sums_url, format!("{base}gh_2.104.0_checksums.txt"));
    let (_, _, zip_name, _) = release_assets(RELEASE_JSON, "x86_64").unwrap();
    assert_eq!(zip_name, "gh_2.104.0_macOS_amd64.zip");
    assert!(release_assets(RELEASE_JSON, "riscv64").is_err());
}

#[test]
fn checksum_lookup() {
    let txt = "aaa111  gh_2.104.0_macOS_amd64.zip\nbbb222  gh_2.104.0_macOS_arm64.zip\n";
    assert_eq!(
        checksum_for(txt, "gh_2.104.0_macOS_arm64.zip").as_deref(),
        Some("bbb222")
    );
    assert_eq!(checksum_for(txt, "gh_2.104.0_linux_arm64.tar.gz"), None);
}

#[test]
fn install_bad_checksum_leaves_nothing() {
    let (bytes, _) = gh_zip(|bin| {
        fake_gh(bin, "echo gh version 9.9.9");
    });
    let dir = tempfile::tempdir().unwrap();
    let wrong = "0".repeat(64);
    assert!(install_from_zip(&bytes, &wrong, dir.path()).is_err());
    assert!(!dir.path().join("bin").exists());
}

/// `gh_9.9.9_macOS_arm64/bin/`을 `fill`로 채워 ditto zip을 만든다 → (바이트, sha256).
fn gh_zip(fill: impl FnOnce(&Path)) -> (Vec<u8>, String) {
    let src = tempfile::tempdir().unwrap();
    let top = src.path().join("gh_9.9.9_macOS_arm64");
    std::fs::create_dir_all(top.join("bin")).unwrap();
    fill(&top.join("bin"));
    let zip = src.path().join("gh.zip");
    let ok = std::process::Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&top)
        .arg(&zip)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let bytes = std::fs::read(&zip).unwrap();
    let sha = hex::encode(Sha256::digest(&bytes));
    (bytes, sha)
}

#[test]
fn install_rejects_symlinked_gh() {
    let (bytes, sha) = gh_zip(|bin| {
        std::os::unix::fs::symlink("/bin/sh", bin.join("gh")).unwrap();
    });
    let app = tempfile::tempdir().unwrap();
    assert!(install_from_zip(&bytes, &sha, app.path()).is_err());
    assert!(!app.path().join("bin").join("gh").exists());
}

#[test]
fn release_assets_rejects_other_host() {
    let json = RELEASE_JSON.replace(
        "https://github.com/cli/cli/releases/download/v2.104.0/gh_2.104.0_macOS_arm64.zip",
        "https://evil.example/gh_2.104.0_macOS_arm64.zip",
    );
    assert!(release_assets(&json, "aarch64").is_err());
    let json = RELEASE_JSON.replace(
        "https://github.com/cli/cli/releases/download/v2.104.0/gh_2.104.0_checksums.txt",
        "https://github.com/cli/cli/releases/download.evil/gh_2.104.0_checksums.txt",
    );
    assert!(release_assets(&json, "aarch64").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_cancel_while_waiting() {
    // 연결은 받지만 답하지 않는 서버.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().take(1).collect();
        std::thread::sleep(Duration::from_secs(10));
        drop(held);
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c.store(true, Ordering::SeqCst);
    });
    let client = reqwest::Client::new();
    let start = Instant::now();
    let r = fetch(&client, &format!("http://{addr}/"), 1024, &cancel).await;
    assert_eq!(r, Err("취소됨".to_string()));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn install_from_zip_places_executable() {
    let (bytes, sha) = gh_zip(|bin| {
        fake_gh(bin, "echo gh version 9.9.9");
    });

    let app = tempfile::tempdir().unwrap();
    let gh = install_from_zip(&bytes, &sha, app.path()).unwrap();
    assert_eq!(gh, app.path().join("bin").join("gh"));
    let out = std::process::Command::new(&gh)
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "gh version 9.9.9\n");
    assert!(!app.path().join("bin").join("gh.tmp").exists());
}

#[test]
fn choose_route_cases() {
    let brew = Path::new("/opt/homebrew");
    assert_eq!(
        choose_route(None, Some(Path::new("/opt/homebrew/bin/gh"))),
        InstallRoute::Release
    );
    assert_eq!(choose_route(Some(brew), None), InstallRoute::BrewInstall);
    assert_eq!(
        choose_route(Some(brew), Some(Path::new("/opt/homebrew/bin/gh"))),
        InstallRoute::BrewUpgrade
    );
    assert_eq!(
        choose_route(Some(brew), Some(Path::new("/usr/local/bin/gh"))),
        InstallRoute::Release
    );
    assert_eq!(
        choose_route(Some(brew), Some(Path::new("/opt/homebrew2/bin/gh"))),
        InstallRoute::Release
    );
}

const CODE_LINE: &str = "! One-time code (AB12-CD34) copied to clipboard";

/// `run_login`을 돌려 결과와 받은 코드들.
fn login(gh: &Path, cancel: &AtomicBool, code_wait: Duration) -> (Result<(), String>, Vec<String>) {
    let mut codes = Vec::new();
    let r = run_login(gh, cancel, code_wait, Duration::from_secs(30), &mut |c| {
        codes.push(c.to_string())
    });
    (r, codes)
}

#[test]
fn login_emits_code_then_ok() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(
        dir.path(),
        &format!("echo '{CODE_LINE}' 1>&2\necho '{CODE_LINE}' 1>&2\nsleep 0.3\nexit 0"),
    );
    let (r, codes) = login(&gh, &AtomicBool::new(false), Duration::from_secs(5));
    assert_eq!(r, Ok(()));
    assert_eq!(codes, ["AB12-CD34"]);
}

#[test]
fn login_no_code_fails_fast() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path(), "sleep 30");
    let start = Instant::now();
    let (r, codes) = login(&gh, &AtomicBool::new(false), Duration::from_millis(300));
    assert!(r.unwrap_err().contains("gh 출력을 읽지 못했어요"));
    assert!(codes.is_empty());
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn login_cancel_kills() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path(), &format!("echo '{CODE_LINE}' 1>&2\nsleep 30"));
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c.store(true, Ordering::SeqCst);
    });
    let start = Instant::now();
    let (r, _) = login(&gh, &cancel, Duration::from_secs(5));
    assert_eq!(r, Err("취소됨".to_string()));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn login_failure_reason() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(
        dir.path(),
        &format!(
            "echo '{CODE_LINE}' 1>&2\necho 'error: device flow expired' 1>&2\necho 1>&2\nexit 1"
        ),
    );
    let (r, _) = login(&gh, &AtomicBool::new(false), Duration::from_secs(5));
    assert_eq!(r, Err("error: device flow expired".to_string()));
}

#[test]
fn login_args_exact() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("args.log");
    let gh = fake_gh(
        dir.path(),
        &format!(
            "printf '%s' \"$*\" > '{}'\necho '{CODE_LINE}' 1>&2\nexit 0",
            log.display()
        ),
    );
    let (r, _) = login(&gh, &AtomicBool::new(false), Duration::from_secs(5));
    assert_eq!(r, Ok(()));
    assert_eq!(std::fs::read_to_string(&log).unwrap(), LOGIN_CMD_ARGS);
}

const STATUS_JSON: &str = r#"{"hosts":{"github.com":[{"state":"success","active":true,"login":"hodoon","tokenSource":"keyring"}]}}"#;

/// `dir/gh` 실행 스크립트를 만든다. `body`는 `#!/bin/sh` 뒤 본문.
fn fake_gh(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("gh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn found(v: (u32, u32, u32), p: &str) -> GhFound {
    GhFound {
        path: PathBuf::from(p),
        version: v,
    }
}

#[test]
fn pick_gh_prefers_new_enough() {
    let path = |r: Result<GhFound, GithubStatus>| r.unwrap().path;
    assert_eq!(
        path(pick_gh(
            Some(found((2, 60, 0), "/path/gh")),
            Some(found((2, 104, 0), "/app/gh"))
        )),
        PathBuf::from("/app/gh")
    );
    assert_eq!(
        path(pick_gh(
            Some(found((2, 103, 3), "/path/gh")),
            Some(found((2, 104, 0), "/app/gh"))
        )),
        PathBuf::from("/path/gh")
    );
    assert_eq!(pick_gh(None, None).unwrap_err(), GithubStatus::NotInstalled);
    assert_eq!(
        pick_gh(Some(found((2, 60, 0), "/path/gh")), None).unwrap_err(),
        GithubStatus::Outdated {
            version: "2.60.0".into()
        }
    );
}

#[test]
fn gh_auth_with_fake_gh() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(
        dir.path(),
        &format!("case \"$*\" in *'auth status'*) echo '{STATUS_JSON}'; exit 0;; esac\nexit 9"),
    );
    match gh_auth(&gh).unwrap() {
        GhAuth::Accounts(a) => assert_eq!(a.len(), 1),
        other => panic!("{other:?}"),
    }
    let gh = fake_gh(
        dir.path(),
        "echo 'You are not logged into any GitHub hosts.' 1>&2; exit 1",
    );
    assert_eq!(gh_auth(&gh).unwrap(), GhAuth::LoggedOut);
    let gh = fake_gh(dir.path(), "exit 2");
    assert!(gh_auth(&gh).is_err());
}

#[test]
fn record_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    write_record(dir.path(), "hodoon").unwrap();
    assert_eq!(read_record(dir.path()).as_deref(), Some("hodoon"));
    clear_record(dir.path()).unwrap();
    assert_eq!(read_record(dir.path()), None);
    clear_record(dir.path()).unwrap();
    std::fs::write(dir.path().join("github.json"), "{nope").unwrap();
    assert_eq!(read_record(dir.path()), None);
}

#[test]
fn disconnect_keeps_gh() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let gh = fake_gh(
        dir.path(),
        &format!(
            "echo \"$*\" >> '{}'\ncase \"$*\" in *'auth status'*) echo '{STATUS_JSON}'; exit 0;; esac\nexit 0",
            log.display()
        ),
    );
    write_record(dir.path(), "hodoon").unwrap();
    // 해제 명령 본문: 기록 지우기 → 상태 다시 읽기.
    clear_record(dir.path()).unwrap();
    let auth = gh_auth(&gh).unwrap();
    assert_eq!(
        derive_status(read_record(dir.path()).as_deref(), &auth),
        GithubStatus::NotConnected
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("auth status"), "{calls}");
    assert!(!calls.contains("logout"), "{calls}");
}

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
