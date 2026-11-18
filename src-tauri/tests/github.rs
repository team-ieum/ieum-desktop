//! gh 출력 읽기·연결 상태 판정, gh 찾기·상태 명령·연결 기록.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ieum_desktop_lib::github::{
    GIT_PROTOCOL_ARG, GhAccount, GhAuth, GhFound, GithubJobView, GithubJobs, GithubStatus,
    InstallRoute, JobGuard, JobKind, LOGIN_CMD_ARGS, MIN_GH, checksum_for, choose_route,
    clear_record, derive_status, disconnect, fetch, gh_auth, install_from_zip, parse_auth_status,
    parse_device_code, parse_gh_version, pick_gh, read_record, ready_login, release_assets,
    run_login, token_cmd, token_login, write_record,
};

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

/// `pid`가 1초 안에 사라지는지(좀비 회수 여유).
fn gone(pid: &str) -> bool {
    let end = Instant::now() + Duration::from_secs(1);
    while Instant::now() < end {
        let alive = std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        if !alive {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn job_view_and_slot() {
    let jobs = GithubJobs::default();
    assert_eq!(jobs.view(), None);
    let cancel = Arc::new(AtomicBool::new(false));
    let guard = JobGuard::start(&jobs, JobKind::Login, cancel.clone()).unwrap();
    assert_eq!(
        jobs.view(),
        Some(GithubJobView {
            kind: "login",
            code: None
        })
    );
    guard.set_code("AB12-CD34");
    assert_eq!(
        serde_json::to_string(&jobs.view().unwrap()).unwrap(),
        r#"{"kind":"login","code":"AB12-CD34"}"#
    );
    assert_eq!(
        JobGuard::start(&jobs, JobKind::Install, cancel.clone()).err(),
        Some("이미 진행 중이에요".to_string())
    );
    drop(guard);
    assert_eq!(jobs.view(), None);
    let _g = JobGuard::start(&jobs, JobKind::Install, cancel).unwrap();
    assert_eq!(
        serde_json::to_string(&jobs.view().unwrap()).unwrap(),
        r#"{"kind":"install","code":null}"#
    );
}

#[test]
fn exit_cancels_running_job() {
    let jobs = GithubJobs::default();
    let cancel = Arc::new(AtomicBool::new(false));
    std::thread::scope(|s| {
        let guard = JobGuard::start(&jobs, JobKind::Login, cancel.clone()).unwrap();
        let c = cancel.clone();
        s.spawn(move || {
            while !c.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(guard);
        });
        let start = Instant::now();
        jobs.cancel_and_wait(Duration::from_millis(300));
        assert!(cancel.load(Ordering::SeqCst));
        assert!(start.elapsed() < Duration::from_millis(300));
        assert_eq!(jobs.view(), None);
    });
    // 작업이 없으면 바로 돌아온다.
    let start = Instant::now();
    jobs.cancel_and_wait(Duration::from_millis(300));
    assert!(start.elapsed() < Duration::from_millis(100));
}

const PROXY_ERR_JSON: &str = r#"{"hosts":{"github.com":[{"state":"error","error":"Get \"https://api.github.com/\": proxyconnect tcp: dial tcp 127.0.0.1:9: connect: connection refused","active":true,"host":"github.com","login":"hodoon","tokenSource":"keyring"}]}}"#;
const BAD_TOKEN_JSON: &str = r#"{"hosts":{"github.com":[{"state":"error","error":"HTTP 401: Bad credentials (https://api.github.com/)","active":true,"host":"github.com","login":"","tokenSource":"GH_TOKEN"}]}}"#;

#[test]
fn derive_unreachable_vs_bad_token() {
    let auth = GhAuth::Accounts(parse_auth_status(PROXY_ERR_JSON).unwrap());
    let GithubStatus::Unreachable { error } = derive_status(Some("hodoon"), &auth) else {
        panic!("{:?}", derive_status(Some("hodoon"), &auth));
    };
    assert!(error.contains("proxyconnect"), "{error}");
    // 기록이 없어도 네트워크 문제로 본다.
    assert!(matches!(
        derive_status(None, &auth),
        GithubStatus::Unreachable { .. }
    ));
    let auth = GhAuth::Accounts(parse_auth_status(BAD_TOKEN_JSON).unwrap());
    assert_eq!(
        derive_status(Some("hodoon"), &auth),
        GithubStatus::NeedsLogin {
            recorded: "hodoon".into()
        }
    );
    // 기록 없음 + 토큰 무효 → [연결]이 터미널 gh 로그인을 바꾼다고 알린다.
    assert_eq!(
        derive_status(None, &auth),
        GithubStatus::NotConnected { relogin: true }
    );
    assert_eq!(
        serde_json::to_string(&GithubStatus::Unreachable { error: "e".into() }).unwrap(),
        r#"{"kind":"unreachable","error":"e"}"#
    );
}

#[test]
fn logged_out_exit0_needs_login() {
    // 실제 gh는 로그아웃이어도 `{"hosts":{}}` + 종료 0.
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(dir.path(), "echo '{\"hosts\":{}}'\nexit 0");
    let auth = gh_auth(&gh).unwrap();
    assert_eq!(
        derive_status(Some("hodoon"), &auth),
        GithubStatus::NeedsLogin {
            recorded: "hodoon".into()
        }
    );
}

#[test]
fn install_cancel_before_rename_leaves_nothing() {
    let (bytes, sha) = gh_zip(|bin| {
        fake_gh(bin, "echo gh version 9.9.9");
    });
    let app = tempfile::tempdir().unwrap();
    let r = install_from_zip(&bytes, &sha, app.path(), &AtomicBool::new(true));
    assert_eq!(r, Err("취소됨".to_string()));
    assert!(!app.path().join("bin").join("gh").exists());
    assert!(!app.path().join("bin").join("gh.tmp").exists());
}
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
    assert!(install_from_zip(&bytes, &wrong, dir.path(), &no_cancel()).is_err());
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
    assert!(install_from_zip(&bytes, &sha, app.path(), &no_cancel()).is_err());
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
    let gh = install_from_zip(&bytes, &sha, app.path(), &no_cancel()).unwrap();
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
    // brew prefix 흉내: <prefix>/Cellar/gh/2.0.0/bin/gh 와 <prefix>/bin/gh → Cellar 링크.
    let root = tempfile::tempdir().unwrap();
    let brew = root.path().join("homebrew");
    let cellar_bin = brew.join("Cellar/gh/2.0.0/bin");
    std::fs::create_dir_all(&cellar_bin).unwrap();
    fake_gh(&cellar_bin, "exit 0");
    std::fs::create_dir_all(brew.join("bin")).unwrap();
    std::os::unix::fs::symlink(cellar_bin.join("gh"), brew.join("bin/gh")).unwrap();
    // 같은 prefix 아래지만 Cellar 링크가 아닌 gh(Intel `/usr/local/bin/gh` 공식 pkg 같은 경우).
    let plain = brew.join("sbin");
    std::fs::create_dir_all(&plain).unwrap();
    let plain_gh = fake_gh(&plain, "exit 0");
    let brew2 = root.path().join("homebrew2/bin");
    std::fs::create_dir_all(&brew2).unwrap();
    let other_gh = fake_gh(&brew2, "exit 0");

    let linked = brew.join("bin/gh");
    assert_eq!(choose_route(None, Some(&linked)), InstallRoute::Release);
    assert_eq!(choose_route(Some(&brew), None), InstallRoute::BrewInstall);
    assert_eq!(
        choose_route(Some(&brew), Some(&linked)),
        InstallRoute::BrewUpgrade
    );
    assert_eq!(
        choose_route(Some(&brew), Some(&plain_gh)),
        InstallRoute::Release
    );
    assert_eq!(
        choose_route(Some(&brew), Some(&other_gh)),
        InstallRoute::Release
    );
    assert_eq!(
        choose_route(Some(&brew), Some(Path::new("/nonexistent/bin/gh"))),
        InstallRoute::Release
    );
}

const CODE_LINE: &str = "! One-time code (AB12-CD34) copied to clipboard";

/// `run_login`을 돌려 결과와 받은 코드들.
fn login(gh: &Path, cancel: &AtomicBool, code_wait: Duration) -> (Result<(), String>, Vec<String>) {
    let (r, codes, _) = login_lines(gh, cancel, code_wait);
    (r, codes)
}

/// `run_login` 결과, 받은 코드들, 흘려 받은 stderr 줄들.
fn login_lines(
    gh: &Path,
    cancel: &AtomicBool,
    code_wait: Duration,
) -> (Result<(), String>, Vec<String>, Vec<String>) {
    let mut codes = Vec::new();
    let mut lines = Vec::new();
    let r = run_login(
        gh,
        true,
        cancel,
        code_wait,
        Duration::from_secs(30),
        &mut |c| codes.push(c.to_string()),
        &mut |l| lines.push(l.to_string()),
    );
    (r, codes, lines)
}

#[test]
fn login_streams_stderr_lines() {
    let dir = tempfile::tempdir().unwrap();
    let gh = fake_gh(
        dir.path(),
        &format!("echo '{CODE_LINE}' 1>&2\necho 'Authentication complete.' 1>&2\nexit 0"),
    );
    let (r, _, lines) = login_lines(&gh, &no_cancel(), Duration::from_secs(5));
    assert_eq!(r, Ok(()));
    assert_eq!(lines, [CODE_LINE, "Authentication complete."]);
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
    assert_eq!(
        r,
        Err("GitHub 응답이 늦어요. 다시 시도해 주세요.".to_string())
    );
    assert!(codes.is_empty());
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn login_cancel_kills() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let gh = fake_gh(
        dir.path(),
        &format!(
            "echo $$ > '{}'\necho '{CODE_LINE}' 1>&2\nexec sleep 30",
            pid_file.display()
        ),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let pf = pid_file.clone();
    // 가짜 gh가 떠서 pid를 남긴 뒤에 취소한다(로그인 셸 시작이 느려도).
    std::thread::spawn(move || {
        while !std::fs::read_to_string(&pf).is_ok_and(|s| s.ends_with('\n')) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(100));
        c.store(true, Ordering::SeqCst);
    });
    let (r, _) = login(&gh, &cancel, Duration::from_secs(5));
    assert_eq!(r, Err("취소됨".to_string()));
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    assert!(gone(pid.trim()), "가짜 gh {pid}가 살아 있음");
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
    assert_eq!(
        LOGIN_CMD_ARGS,
        "auth login --web --hostname github.com --skip-ssh-key --scopes workflow"
    );
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("args.log");
    let gh = fake_gh(
        dir.path(),
        &format!(
            "printf '%s' \"$*\" > '{}'\necho '{CODE_LINE}' 1>&2\nexit 0",
            log.display()
        ),
    );
    let args = |set_protocol| {
        let r = run_login(
            &gh,
            set_protocol,
            &no_cancel(),
            Duration::from_secs(5),
            Duration::from_secs(30),
            &mut |_| {},
            &mut |_| {},
        );
        assert_eq!(r, Ok(()));
        std::fs::read_to_string(&log).unwrap()
    };
    // gh에 github.com 계정 항목이 없음 → 프로토콜을 정한다.
    assert_eq!(args(true), format!("{LOGIN_CMD_ARGS}{GIT_PROTOCOL_ARG}"));
    assert_eq!(
        args(true),
        "auth login --web --hostname github.com --skip-ssh-key --scopes workflow --git-protocol https"
    );
    // 이미 있음 → 사용자의 git_protocol을 덮지 않는다.
    assert_eq!(args(false), LOGIN_CMD_ARGS);
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
        &format!(
            "case \"$*\" in 'auth status --active --json hosts --hostname github.com') echo '{STATUS_JSON}'; exit 0;; esac\nexit 9"
        ),
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
    // 해제 명령이 쓰는 함수 그대로.
    assert_eq!(
        disconnect(dir.path(), &gh).unwrap(),
        GithubStatus::NotConnected { relogin: false }
    );
    assert_eq!(read_record(dir.path()), None);
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
        error: None,
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
    let not = |relogin| GithubStatus::NotConnected { relogin };
    let auth = GhAuth::Accounts(vec![acct("hodoon", true, "success", "keyring")]);
    assert_eq!(derive_status(None, &auth), not(false));
    assert_eq!(derive_status(None, &GhAuth::LoggedOut), not(false));
    assert_eq!(derive_status(None, &GhAuth::Accounts(vec![])), not(false));
    let bad = GhAuth::Accounts(parse_auth_status(BAD_TOKEN_JSON).unwrap());
    assert_eq!(derive_status(None, &bad), not(true));
    assert_eq!(
        serde_json::to_string(&not(true)).unwrap(),
        r#"{"kind":"notConnected","relogin":true}"#
    );
}

#[test]
fn ready_login_needs_name() {
    let ok = GhAuth::Accounts(vec![acct("hodoon", true, "success", "keyring")]);
    assert_eq!(ready_login(&ok).as_deref(), Some("hodoon"));
    // 사용자 없는 호스트 항목: gh가 키체인 기본 토큰으로 login "" + success를 준다.
    let empty = GhAuth::Accounts(vec![acct("", true, "success", "keyring")]);
    assert_eq!(ready_login(&empty), None);
    let bad = GhAuth::Accounts(vec![acct("hodoon", true, "error", "keyring")]);
    assert_eq!(ready_login(&bad), None);
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

fn gh_found() -> Result<GhFound, GithubStatus> {
    Ok(GhFound {
        path: PathBuf::from("/x/gh"),
        version: (2, 103, 3),
    })
}

/// 실패면 (code, message).
fn login_err(
    found: Result<GhFound, GithubStatus>,
    recorded: Option<&str>,
    auth: Result<GhAuth, String>,
) -> (Option<String>, String) {
    let e = token_login(found, recorded, auth).unwrap_err();
    (e.code, e.message)
}

const NOT_CONNECTED: Option<&str> = Some("GITHUB_NOT_CONNECTED");
const UNREACHABLE: Option<&str> = Some("GITHUB_UNREACHABLE");

#[test]
fn token_login_states() {
    let ok = |a: Vec<GhAccount>| Ok(GhAuth::Accounts(a));
    let need_gh = "GitHub를 쓰려면 gh CLI가 필요해요. 설정 > 연동을 확인해 주세요.";
    for f in [
        GithubStatus::NotInstalled,
        GithubStatus::Outdated {
            version: "2.40.0".into(),
        },
    ] {
        let (code, msg) = login_err(Err(f), Some("hodoon"), ok(vec![]));
        assert_eq!((code.as_deref(), msg.as_str()), (NOT_CONNECTED, need_gh));
    }
    let unreachable = "GitHub에 닿지 못했어요. 잠시 뒤 다시 실행해 주세요.";
    let (code, msg) = login_err(gh_found(), Some("hodoon"), Err("gh 꺼짐".into()));
    assert_eq!((code.as_deref(), msg.as_str()), (UNREACHABLE, unreachable));

    let (code, msg) = login_err(
        gh_found(),
        None,
        ok(vec![acct("hodoon", true, "success", "keyring")]),
    );
    assert_eq!(
        (code.as_deref(), msg.as_str()),
        (
            NOT_CONNECTED,
            "GitHub 연결이 필요해요. 설정 > 연동에서 연결해 주세요."
        )
    );

    let (code, msg) = login_err(gh_found(), Some("hodoon"), Ok(GhAuth::LoggedOut));
    assert_eq!(
        (code.as_deref(), msg.as_str()),
        (
            Some("GITHUB_AUTH"),
            "GitHub 로그인이 만료됐어요. 설정 > 연동에서 다시 연결해 주세요."
        )
    );

    // gh 오류 문구(사용자 이름·주소가 들어 있을 수 있음)는 싣지 않는다.
    let mut down = acct("hodoon", true, "error", "keyring");
    down.error = Some("dial tcp: lookup api.github.com secret-proxy".into());
    let (code, msg) = login_err(gh_found(), Some("hodoon"), ok(vec![down]));
    assert_eq!((code.as_deref(), msg.as_str()), (UNREACHABLE, unreachable));
    assert!(!msg.contains("secret-proxy"));

    assert_eq!(
        token_login(
            gh_found(),
            Some("hodoon"),
            ok(vec![acct("hodoon", true, "success", "keyring")])
        )
        .unwrap(),
        (PathBuf::from("/x/gh"), "hodoon".to_string(), false)
    );
    assert_eq!(
        token_login(
            gh_found(),
            Some("hodoon"),
            ok(vec![acct("hodoon", true, "success", "GH_TOKEN")])
        )
        .unwrap(),
        (PathBuf::from("/x/gh"), "hodoon".to_string(), true)
    );
}

/// 연결 뒤 터미널에서 `gh auth switch`로 계정을 바꾸면 다른 계정으로 쓰지 않는다.
#[test]
fn account_changed_refuses() {
    let auth = GhAuth::Accounts(vec![
        acct("hodoon", false, "success", "keyring"),
        acct("other", true, "success", "keyring"),
    ]);
    let (code, msg) = login_err(gh_found(), Some("hodoon"), Ok(auth));
    assert_eq!(
        (code.as_deref(), msg.as_str()),
        (
            NOT_CONNECTED,
            "gh 계정이 연결한 계정과 달라요. 설정 > 연동을 확인해 주세요."
        )
    );
}

/// 환경변수 토큰은 gh가 `--user`로 고를 수 없어 활성 토큰을 그대로 읽는다.
#[test]
fn token_cmd_user_only_without_env_token() {
    let gh = Path::new("/x/gh");
    assert_eq!(
        token_cmd(gh, "hodoon", false),
        "'/x/gh' auth token --hostname github.com --user 'hodoon'"
    );
    assert_eq!(
        token_cmd(gh, "hodoon", true),
        "'/x/gh' auth token --hostname github.com"
    );
}
