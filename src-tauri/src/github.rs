//! GitHub 연결(gh CLI). gh 찾기·출력 읽기·연결 상태 판정, IEUM 연결 기록(`github.json`).
//! gh는 사용자 로그인 셸로 부른다. IEUM은 gh에서 로그아웃시키지 않는다 — 해제 = 기록 삭제.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ieum_local::ai::{sh_quote, shell_output, shell_run};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::active::Active;

/// gh 찾기·상태 명령 시간 상한.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
/// 계정 폴더의 연결 기록 파일.
const RECORD: &str = "github.json";

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
    /// state가 "error"일 때 gh가 주는 원인(401·네트워크 등).
    #[serde(default)]
    pub error: Option<String>,
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
    /// gh가 GitHub에 닿지 못함(네트워크·프록시 등). 토큰 문제가 아니라 로그인을 권하지 않는다.
    Unreachable {
        error: String,
    },
}

/// 활성 계정이 토큰 문제(401) 아닌 오류면 그 오류 문구.
fn unreachable_error(auth: &GhAuth) -> Option<String> {
    let GhAuth::Accounts(accounts) = auth else {
        return None;
    };
    let a = accounts.iter().find(|a| a.active && a.state != "success")?;
    let e = a.error.as_deref().filter(|e| !e.trim().is_empty())?;
    (!e.contains("401") && !e.contains("Bad credentials")).then(|| e.trim().to_string())
}

/// gh 설치·버전 확인을 통과한 뒤의 상태. `recorded` = IEUM 연결 기록 login.
pub fn derive_status(recorded: Option<&str>, auth: &GhAuth) -> GithubStatus {
    if let Some(error) = unreachable_error(auth) {
        return GithubStatus::Unreachable { error };
    }
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
    // 토큰이 죽은 계정은 login이 비어 오기도 해서(401) 계정 비교보다 먼저 본다.
    if a.state != "success" {
        needs()
    } else if a.login != recorded {
        GithubStatus::AccountChanged {
            recorded: recorded.into(),
            current: a.login.clone(),
        }
    } else {
        GithubStatus::Connected {
            login: a.login.clone(),
            env_token: matches!(a.token_source.as_str(), "GH_TOKEN" | "GITHUB_TOKEN"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GhFound {
    pub path: PathBuf,
    pub version: (u32, u32, u32),
}

/// PATH 쪽이 충분히 새것이면 PATH, 아니면 앱 폴더 쪽(이것도 충분할 때).
/// 없음 → `NotInstalled`, 둘 다 낮음 → `Outdated`(더 높은 버전).
pub fn pick_gh(path_gh: Option<GhFound>, app_gh: Option<GhFound>) -> Result<GhFound, GithubStatus> {
    let all = [path_gh, app_gh];
    if let Some(g) = all.iter().flatten().find(|g| g.version >= MIN_GH) {
        return Ok(g.clone());
    }
    match all.iter().flatten().map(|g| g.version).max() {
        Some((a, b, c)) => Err(GithubStatus::Outdated {
            version: format!("{a}.{b}.{c}"),
        }),
        None => Err(GithubStatus::NotInstalled),
    }
}

/// 사용자 PATH의 gh와 앱 폴더 `<app_data>/bin/gh` 중 쓸 것.
pub fn find_gh(app_data: &Path) -> Result<GhFound, GithubStatus> {
    let path_gh = shell_output("command -v gh && gh --version", STATUS_TIMEOUT).and_then(|t| {
        let mut lines = t.lines();
        let path = PathBuf::from(lines.next()?.trim());
        Some(GhFound {
            path,
            version: parse_gh_version(lines.next()?)?,
        })
    });
    let app = app_data.join("bin").join("gh");
    let app_gh = app
        .is_file()
        .then(|| version_of(&app))
        .flatten()
        .map(|version| GhFound { path: app, version });
    pick_gh(path_gh, app_gh)
}

/// 절대 경로 gh의 `--version`(로그인 셸 없이). 5초를 넘기면 kill하고 None.
fn version_of(gh: &Path) -> Option<(u32, u32, u32)> {
    let mut child = std::process::Command::new(gh)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + STATUS_TIMEOUT;
    // ponytail: 20ms 폴링 — 출력이 한두 줄이라 파이프가 차지 않는다.
    while child.try_wait().ok()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    parse_gh_version(out.lines().next()?)
}

/// `gh auth status` 상한. gh가 GitHub API로 토큰을 검증해 1~2초 걸린다.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

/// `gh auth status --active --json hosts --hostname github.com`(활성 계정만 검증). 종료 1 → `LoggedOut`.
/// 실제 gh는 로그아웃이어도 `{"hosts":{}}` + 종료 0이라 보통은 빈 목록으로 온다.
pub fn gh_auth(gh: &Path) -> Result<GhAuth, String> {
    let cmd = format!(
        "{} auth status --active --json hosts --hostname github.com",
        sh_quote(&gh.to_string_lossy())
    );
    let r = shell_run(&cmd, AUTH_TIMEOUT, &AtomicBool::new(false), &mut |_| {})
        .map_err(|e| format!("gh를 실행하지 못했어요: {e}"))?;
    if r.timed_out {
        return Err("gh가 10초 안에 답하지 않았어요.".into());
    }
    match r.code {
        Some(0) => parse_auth_status(&r.stdout).map(GhAuth::Accounts),
        Some(1) => Ok(GhAuth::LoggedOut),
        _ => Err(format!("gh 상태를 읽지 못했어요: {}", r.stderr.trim())),
    }
}

/// `<dir>/github.json`의 login. 없거나 깨졌으면 None.
pub fn read_record(dir: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Record {
        login: String,
    }
    let text = std::fs::read_to_string(dir.join(RECORD)).ok()?;
    serde_json::from_str::<Record>(&text).ok().map(|r| r.login)
}

/// 연결 기록을 쓴다(임시 파일 + rename). 토큰은 저장하지 않는다.
pub fn write_record(dir: &Path, login: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{RECORD}.tmp"));
    let body = serde_json::json!({ "login": login }).to_string();
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(RECORD)).map_err(|e| e.to_string())
}

/// 연결 기록만 지운다(gh 로그인은 그대로). 없으면 Ok.
pub fn clear_record(dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(dir.join(RECORD)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JobKind {
    Login,
    Install,
}

/// 진행 중인 gh 작업(로그인·설치) 하나.
pub struct GithubJob {
    pub kind: JobKind,
    pub cancel: Arc<AtomicBool>,
    /// 로그인 중 받은 device 코드(다시 연 화면이 코드 카드를 그린다).
    pub code: Option<String>,
}

#[derive(Default)]
pub struct GithubJobs {
    pub current: Mutex<Option<GithubJob>>,
}

/// 화면에 주는 진행 중 작업. `kind`는 `"login"`·`"install"`.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubJobView {
    pub kind: &'static str,
    pub code: Option<String>,
}

impl GithubJobs {
    pub fn view(&self) -> Option<GithubJobView> {
        let cur = self.current.lock().ok()?;
        cur.as_ref().map(|j| GithubJobView {
            kind: match j.kind {
                JobKind::Login => "login",
                JobKind::Install => "install",
            },
            code: j.code.clone(),
        })
    }

    /// 진행 중 작업을 취소하고 슬롯이 빌 때까지 최대 `max` 기다린다(앱 종료 때 — gh·brew가 고아로 남지 않게).
    pub fn cancel_and_wait(&self, max: Duration) {
        match self.current.lock() {
            Ok(cur) => match cur.as_ref() {
                Some(j) => j.cancel.store(true, Ordering::SeqCst),
                None => return,
            },
            Err(_) => return,
        }
        let end = Instant::now() + max;
        while Instant::now() < end && self.current.lock().is_ok_and(|c| c.is_some()) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// gh 찾기 → 로그인 상태 → 기록과 비교.
fn status_at(app_data: &Path, dir: &Path) -> Result<GithubStatus, String> {
    let gh = match find_gh(app_data) {
        Ok(g) => g,
        Err(s) => return Ok(s),
    };
    let auth = gh_auth(&gh.path)?;
    Ok(derive_status(read_record(dir).as_deref(), &auth))
}

async fn status_of(active: &Active) -> Result<GithubStatus, String> {
    let dir = active.account_dir().await?;
    let app_data = active.app_data().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || status_at(&app_data, &dir))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn github_status(active: State<'_, Active>) -> Result<GithubStatus, String> {
    status_of(&active).await
}

/// 연결 해제 본문: IEUM 기록만 지우고 상태를 다시 읽는다. gh에서 로그아웃시키지 않는다.
pub fn disconnect(dir: &Path, gh: &Path) -> Result<GithubStatus, String> {
    clear_record(dir)?;
    Ok(derive_status(read_record(dir).as_deref(), &gh_auth(gh)?))
}

#[tauri::command]
pub async fn github_disconnect(active: State<'_, Active>) -> Result<GithubStatus, String> {
    let dir = active.account_dir().await?;
    let app_data = active.app_data().to_path_buf();
    blocking(move || match find_gh(&app_data) {
        Ok(g) => disconnect(&dir, &g.path),
        Err(s) => clear_record(&dir).map(|()| s),
    })
    .await?
}

/// 진행 중인 gh 작업. 다시 연 화면이 [취소]·코드 카드를 그리는 데 쓴다.
#[tauri::command]
pub async fn github_job(jobs: State<'_, GithubJobs>) -> Result<Option<GithubJobView>, String> {
    Ok(jobs.view())
}

/// [브라우저 열기]가 여는 고정 주소. gh 출력의 주소는 쓰지 않는다.
pub const DEVICE_URL: &str = "https://github.com/login/device";
/// `--scopes workflow`: 터미널 기본 로그인과 같게(빠지면 `.github/workflows` push가 거절된다).
pub const LOGIN_CMD_ARGS: &str =
    "auth login --web --hostname github.com --skip-ssh-key --scopes workflow";
/// gh에 github.com 계정 항목이 없을 때만 붙인다 — 있으면 사용자의 git_protocol을 덮지 않는다.
pub const GIT_PROTOCOL_ARG: &str = " --git-protocol https";
const NO_CODE: &str = "gh 출력을 읽지 못했어요";
/// 로그인 코드를 기다리는 시간·로그인 전체 상한(device code 만료).
const CODE_WAIT: Duration = Duration::from_secs(10);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// device 로그인. stderr 줄마다 `on_line`, 처음 읽은 코드로 `on_code`를 한 번 부른다.
/// `code_wait` 안에 코드가 없으면 `cancel`을 세우고 실패. `set_protocol`이면 `GIT_PROTOCOL_ARG`를 붙인다.
pub fn run_login(
    gh: &Path,
    set_protocol: bool,
    cancel: &AtomicBool,
    code_wait: Duration,
    limit: Duration,
    on_code: &mut dyn FnMut(&str),
    on_line: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let protocol = if set_protocol { GIT_PROTOCOL_ARG } else { "" };
    let cmd = format!(
        "{} {LOGIN_CMD_ARGS}{protocol}",
        sh_quote(&gh.to_string_lossy())
    );
    let (got, done, expired) = (
        AtomicBool::new(false),
        AtomicBool::new(false),
        AtomicBool::new(false),
    );
    let mut last = String::new();
    let r = std::thread::scope(|s| {
        s.spawn(|| {
            let end = Instant::now() + code_wait;
            while !done.load(Ordering::SeqCst) && !got.load(Ordering::SeqCst) {
                if Instant::now() >= end {
                    expired.store(true, Ordering::SeqCst);
                    cancel.store(true, Ordering::SeqCst);
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let r = shell_run(&cmd, limit, cancel, &mut |line| {
            on_line(line);
            if !line.trim().is_empty() {
                last = line.trim().to_string();
            }
            if !got.load(Ordering::SeqCst)
                && let Some(code) = parse_device_code(line)
            {
                got.store(true, Ordering::SeqCst);
                on_code(&code);
            }
        });
        done.store(true, Ordering::SeqCst);
        r
    })
    .map_err(|e| format!("gh를 실행하지 못했어요: {e}"))?;
    if expired.load(Ordering::SeqCst) {
        Err(NO_CODE.into())
    } else if r.cancelled {
        Err("취소됨".into())
    } else if r.timed_out {
        Err("로그인 시간이 끝났어요(15분). 다시 시도해 주세요.".into())
    } else if r.code == Some(0) {
        Ok(())
    } else if last.is_empty() {
        Err(format!("gh 로그인이 실패했어요(종료 코드 {:?}).", r.code))
    } else {
        Err(last)
    }
}

/// 활성이고 로그인이 살아 있는 계정.
fn ready_login(auth: &GhAuth) -> Option<String> {
    match auth {
        GhAuth::Accounts(a) => a
            .iter()
            .find(|a| a.active && a.state == "success")
            .map(|a| a.login.clone()),
        GhAuth::LoggedOut => None,
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

/// jobs 슬롯을 차지하고, 버려질 때(성공·실패·취소) 비운다.
pub struct JobGuard<'a>(&'a GithubJobs);

impl<'a> JobGuard<'a> {
    pub fn start(
        jobs: &'a GithubJobs,
        kind: JobKind,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let mut cur = jobs.current.lock().map_err(|e| e.to_string())?;
        if cur.is_some() {
            return Err("이미 진행 중이에요".into());
        }
        *cur = Some(GithubJob {
            kind,
            cancel,
            code: None,
        });
        Ok(Self(jobs))
    }

    /// 로그인 코드를 작업에 남긴다.
    pub fn set_code(&self, code: &str) {
        self.0.set_code(code);
    }
}

impl GithubJobs {
    fn set_code(&self, code: &str) {
        if let Ok(mut cur) = self.current.lock()
            && let Some(j) = cur.as_mut()
        {
            j.code = Some(code.to_string());
        }
    }
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut cur) = self.0.current.lock() {
            *cur = None;
        }
    }
}

/// [연결]. gh가 이미 로그인돼 있으면(환경변수 토큰 포함) 바로 기록, 아니면 device 로그인 뒤 기록.
#[tauri::command]
pub async fn github_connect(
    app: AppHandle,
    active: State<'_, Active>,
    jobs: State<'_, GithubJobs>,
) -> Result<GithubStatus, String> {
    let dir = active.account_dir().await?;
    // 상태 확인 구간부터 슬롯을 잡는다 — 그 사이 [취소]·화면 떠남도 먹게.
    let cancel = Arc::new(AtomicBool::new(false));
    let _job = JobGuard::start(&jobs, JobKind::Login, cancel.clone())?;
    let is_cancelled = || -> Result<(), String> {
        if cancel.load(Ordering::SeqCst) {
            Err("취소됨".into())
        } else {
            Ok(())
        }
    };
    let app_data = active.app_data().to_path_buf();
    let gh = match blocking(move || find_gh(&app_data)).await? {
        Ok(g) => g,
        Err(s) => return Ok(s),
    };
    is_cancelled()?;
    let path = gh.path.clone();
    let auth = blocking(move || gh_auth(&path)).await??;
    is_cancelled()?;
    if let Some(error) = unreachable_error(&auth) {
        return Ok(GithubStatus::Unreachable { error });
    }
    let login = match ready_login(&auth) {
        Some(l) => l,
        None => {
            let (v0, v1, v2) = gh.version;
            let path = gh.path.clone();
            let cancel = cancel.clone();
            // github.com 계정 항목이 이미 있으면(state 무관) 프로토콜은 그대로 둔다.
            let set_protocol = !matches!(&auth, GhAuth::Accounts(a) if !a.is_empty());
            blocking(move || {
                let mut emit_line = |line: &str| progress(&app, line);
                run_login(
                    &path,
                    set_protocol,
                    &cancel,
                    CODE_WAIT,
                    LOGIN_TIMEOUT,
                    &mut |code| {
                        app.state::<GithubJobs>().set_code(code);
                        let _ = app.emit("github-login-code", serde_json::json!({ "code": code }));
                    },
                    &mut emit_line,
                )
            })
            .await?
            .map_err(|e| {
                if e == NO_CODE {
                    format!("{e} (gh {v0}.{v1}.{v2})")
                } else {
                    e
                }
            })?;
            let path = gh.path.clone();
            ready_login(&blocking(move || gh_auth(&path)).await??)
                .ok_or("로그인 뒤에도 gh 계정을 찾지 못했어요.")?
        }
    };
    write_record(&dir, &login)?;
    status_of(&active).await
}

/// 진행 중인 gh 작업(로그인·설치)을 멈춘다.
#[tauri::command]
pub async fn github_cancel(jobs: State<'_, GithubJobs>) -> Result<(), String> {
    if let Some(j) = jobs.current.lock().map_err(|e| e.to_string())?.as_ref() {
        j.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn github_open_device() -> Result<(), String> {
    crate::commands::open_url(DEVICE_URL)
}

/// 설치·업데이트 전체 상한.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// 받는 zip 크기 상한. checksums·릴리스 JSON은 훨씬 작다.
const MAX_ZIP: usize = 50 * 1024 * 1024;
const MAX_SMALL: usize = 1024 * 1024;
const LATEST_RELEASE: &str = "https://api.github.com/repos/cli/cli/releases/latest";
/// 받는 zip·checksums 주소는 이 아래만.
const DOWNLOAD_PREFIX: &str = "https://github.com/cli/cli/releases/download/";

/// 최신 릴리스 JSON에서 (태그, zip 주소, zip 이름, checksums 주소). `arch`는 `std::env::consts::ARCH`.
pub fn release_assets(
    release_json: &str,
    arch: &str,
) -> Result<(String, String, String, String), String> {
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        assets: Vec<Asset>,
    }
    let r: Release = serde_json::from_str(release_json)
        .map_err(|e| format!("릴리스 정보를 읽지 못했어요: {e}"))?;
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => return Err(format!("이 Mac({other})용 gh가 없어요.")),
    };
    let ver = r.tag_name.trim_start_matches('v');
    let url = |name: &str| {
        r.assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
            .filter(|u| u.starts_with(DOWNLOAD_PREFIX))
            .ok_or_else(|| format!("릴리스에 {name}이 없어요."))
    };
    let zip_name = format!("gh_{ver}_macOS_{arch}.zip");
    let zip_url = url(&zip_name)?;
    let sums_url = url(&format!("gh_{ver}_checksums.txt"))?;
    Ok((r.tag_name.clone(), zip_url, zip_name, sums_url))
}

/// `<sha256>  <file>` 줄에서 `file`의 sha256.
pub fn checksum_for(checksums_txt: &str, file: &str) -> Option<String> {
    checksums_txt.lines().find_map(|l| {
        let (sha, name) = l.split_once(char::is_whitespace)?;
        (name.trim() == file).then(|| sha.to_string())
    })
}

/// 버려질 때 지우는 임시 폴더.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// sha256이 맞으면 zip의 `*/bin/gh`를 `<app_data>/bin/gh`에 둔다. 틀리면 아무것도 쓰지 않는다.
/// 바꿔 넣기 직전에 `cancel`이면 `gh.tmp`를 지우고 `"취소됨"`.
pub fn install_from_zip(
    zip: &[u8],
    expected_sha256: &str,
    app_data: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    if !hex::encode(Sha256::digest(zip)).eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err("받은 gh 파일의 체크섬이 맞지 않아요.".into());
    }
    let err = |e: std::io::Error| format!("gh를 설치하지 못했어요: {e}");
    let tmp = new_temp_dir().map_err(err)?;
    let out = tmp.0.join("out");
    std::fs::create_dir(&out).map_err(err)?;
    let zip_path = tmp.0.join("gh.zip");
    std::fs::write(&zip_path, zip).map_err(err)?;
    let ok = std::process::Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip_path)
        .arg(&out)
        .status()
        .map_err(err)?
        .success();
    if !ok {
        return Err("gh 압축을 풀지 못했어요.".into());
    }
    // 링크는 따라가지 않는다 — 압축 안 링크가 밖의 파일(/bin/sh 등)을 gh로 둘 수 있다.
    let kind = |p: &Path| std::fs::symlink_metadata(p).map(|m| m.file_type());
    let src = std::fs::read_dir(&out)
        .map_err(err)?
        .flatten()
        .map(|e| e.path())
        .filter(|top| kind(top).is_ok_and(|t| t.is_dir()))
        .filter(|top| kind(&top.join("bin")).is_ok_and(|t| t.is_dir()))
        .map(|top| top.join("bin").join("gh"))
        .find(|p| kind(p).is_ok_and(|t| t.is_file()))
        .ok_or("압축 안에 gh 실행 파일이 없어요.")?;
    let bin = app_data.join("bin");
    std::fs::create_dir_all(&bin).map_err(err)?;
    let staged = bin.join("gh.tmp");
    let dest = bin.join("gh");
    let staged_ok = std::fs::copy(&src, &staged)
        .and_then(|_| std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)));
    // 바꿔 넣기 직전이 마지막 취소 지점.
    let cancelled = cancel.load(Ordering::SeqCst);
    let placed = staged_ok.and_then(|()| {
        if cancelled {
            Ok(())
        } else {
            std::fs::rename(&staged, &dest)
        }
    });
    if cancelled || placed.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    match placed {
        Err(e) => Err(err(e)),
        Ok(()) if cancelled => Err("취소됨".into()),
        Ok(()) => Ok(dest),
    }
}

/// 시스템 임시 폴더 아래 새 폴더(배타 생성, 이미 있으면 다른 이름).
fn new_temp_dir() -> std::io::Result<TempDir> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut n = 0;
    loop {
        let p = std::env::temp_dir().join(format!("ieum-gh-{}-{nanos}-{n}", std::process::id()));
        match std::fs::create_dir(&p) {
            Ok(()) => return Ok(TempDir(p)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) => return Err(e),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstallRoute {
    BrewInstall,
    BrewUpgrade,
    Release,
}

/// brew가 있고 gh가 없거나 brew 것이면 brew, 그 밖엔 공식 릴리스(기존 gh는 둔다).
/// "brew 것" = 링크를 풀면 `<brew prefix>/Cellar/` 아래(Intel은 prefix가 `/usr/local`이라 prefix만으론 모른다).
pub fn choose_route(brew_prefix: Option<&Path>, current_gh: Option<&Path>) -> InstallRoute {
    let Some(prefix) = brew_prefix else {
        return InstallRoute::Release;
    };
    let Some(gh) = current_gh else {
        return InstallRoute::BrewInstall;
    };
    let cellar = prefix
        .canonicalize()
        .unwrap_or_else(|_| prefix.to_path_buf())
        .join("Cellar");
    match gh.canonicalize() {
        Ok(real) if real.starts_with(&cellar) => InstallRoute::BrewUpgrade,
        _ => InstallRoute::Release,
    }
}

/// 설치·로그인 진행 줄(화면의 "출력 보기").
fn progress(app: &AppHandle, line: &str) {
    let _ = app.emit("github-progress", serde_json::json!({ "line": line }));
}

fn run_brew(cmd: &str, cancel: &AtomicBool, app: &AppHandle) -> Result<(), String> {
    let mut last = String::new();
    // brew는 진행을 stdout에 쓴다 — stderr로 합쳐야 줄 단위로, 순서대로 흘러온다.
    let r = shell_run(
        &format!("{cmd} 1>&2"),
        INSTALL_TIMEOUT,
        cancel,
        &mut |line| {
            if !line.trim().is_empty() {
                last = line.trim().to_string();
            }
            progress(app, line);
        },
    )
    .map_err(|e| format!("brew를 실행하지 못했어요: {e}"))?;
    if r.cancelled {
        Err("취소됨".into())
    } else if r.timed_out {
        Err("설치 시간이 끝났어요(10분).".into())
    } else if r.code == Some(0) {
        Ok(())
    } else if last.is_empty() {
        Err(format!("brew가 실패했어요(종료 코드 {:?}).", r.code))
    } else {
        Err(last)
    }
}

/// `cancel`이 서면 끝나는 future(50ms 간격 확인).
async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `max`바이트를 넘거나 취소되면 멈추고 실패. 연결·응답을 기다리는 중에도 취소된다.
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    max: usize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    tokio::select! {
        r = fetch_inner(client, url, max) => r,
        () = cancelled(cancel) => Err("취소됨".into()),
    }
}

async fn fetch_inner(client: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>, String> {
    let net = |e: reqwest::Error| format!("gh를 받지 못했어요: {e}");
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?;
    let too_big = || format!("받는 파일이 너무 커요({}MB 넘음).", max / 1024 / 1024);
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(too_big());
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(net)? {
        buf.extend_from_slice(&chunk);
        if buf.len() > max {
            return Err(too_big());
        }
    }
    Ok(buf)
}

/// 공식 릴리스 zip을 받아 체크섬을 확인하고 앱 폴더에 둔다. 받기 단계에 10분 상한.
async fn install_release(
    app: &AppHandle,
    app_data: &Path,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let (zip, sha) = tokio::time::timeout(INSTALL_TIMEOUT, download_release(app, cancel))
        .await
        .map_err(|_| "설치 시간이 끝났어요(10분).".to_string())??;
    let (app_data, cancel) = (app_data.to_path_buf(), cancel.clone());
    blocking(move || install_from_zip(&zip, &sha, &app_data, &cancel)).await??;
    progress(app, "설치 완료");
    Ok(())
}

/// 최신 릴리스 zip과 그 sha256.
async fn download_release(
    app: &AppHandle,
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, String), String> {
    let client = reqwest::Client::builder()
        .user_agent("ieum-desktop")
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let json = fetch(&client, LATEST_RELEASE, MAX_SMALL, cancel).await?;
    let (tag, zip_url, zip_name, sums_url) =
        release_assets(&String::from_utf8_lossy(&json), std::env::consts::ARCH)?;
    progress(app, &format!("gh {tag} 받는 중…"));
    let zip = fetch(&client, &zip_url, MAX_ZIP, cancel).await?;
    let sums = fetch(&client, &sums_url, MAX_SMALL, cancel).await?;
    progress(app, "검증 중…");
    let sha = checksum_for(&String::from_utf8_lossy(&sums), &zip_name)
        .ok_or("체크섬 목록에 gh 파일이 없어요.")?;
    Ok((zip, sha))
}

/// [설치]·[업데이트]. brew가 있으면 brew로, 아니면 공식 릴리스를 앱 폴더에.
#[tauri::command]
pub async fn github_install(
    app: AppHandle,
    active: State<'_, Active>,
    jobs: State<'_, GithubJobs>,
) -> Result<GithubStatus, String> {
    let app_data = active.app_data().to_path_buf();
    let cancel = Arc::new(AtomicBool::new(false));
    let _job = JobGuard::start(&jobs, JobKind::Install, cancel.clone())?;
    let (brew, current) = blocking(|| {
        let line = |cmd| {
            shell_output(cmd, STATUS_TIMEOUT)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        };
        (line("brew --prefix"), line("command -v gh"))
    })
    .await?;
    let route = choose_route(brew.as_deref(), current.as_deref());
    let mut released = route == InstallRoute::Release;
    if released {
        install_release(&app, &app_data, &cancel).await?;
    } else {
        let cmd = if route == InstallRoute::BrewInstall {
            "brew install gh"
        } else {
            "brew upgrade gh"
        };
        let (app2, c) = (app.clone(), cancel.clone());
        let brewed = blocking(move || run_brew(cmd, &c, &app2)).await?;
        match brewed {
            Err(e) if e == "취소됨" => return Err(e),
            // brew가 안 되면(다른 계정 소유·"not installed" 등) 공식 릴리스로 한 번 더.
            Err(e) => {
                progress(&app, &format!("brew 실패({e}) — 공식 릴리스로 설치해요."));
                released = true;
                install_release(&app, &app_data, &cancel).await?;
            }
            Ok(()) => {}
        }
    }
    let ad = app_data.clone();
    if let Err(s) = blocking(move || find_gh(&ad)).await? {
        // 이번에 앱 폴더에 둔 gh가 쓸 수 없으면 지운다.
        if released {
            let _ = std::fs::remove_file(app_data.join("bin").join("gh"));
        }
        return Err(match s {
            GithubStatus::Outdated { version } => {
                format!("설치했지만 gh가 2.81.0보다 낮아요(gh {version}).")
            }
            _ => "설치했지만 gh를 찾지 못했어요.".into(),
        });
    }
    status_of(&active).await
}
