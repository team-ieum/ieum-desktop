//! GitHub 연결(gh CLI). gh 찾기·출력 읽기·연결 상태 판정, IEUM 연결 기록(`github.json`).
//! gh는 사용자 로그인 셸로 부른다. IEUM은 gh에서 로그아웃시키지 않는다 — 해제 = 기록 삭제.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ieum_local::ai::{sh_quote, shell_output, shell_run};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

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
        .then(|| {
            shell_output(
                &format!("{} --version", sh_quote(&app.to_string_lossy())),
                STATUS_TIMEOUT,
            )
        })
        .flatten()
        .and_then(|t| parse_gh_version(t.lines().next()?))
        .map(|version| GhFound { path: app, version });
    pick_gh(path_gh, app_gh)
}

/// `gh auth status --json hosts --hostname github.com`. 종료 1 → `LoggedOut`.
pub fn gh_auth(gh: &Path) -> Result<GhAuth, String> {
    let cmd = format!(
        "{} auth status --json hosts --hostname github.com",
        sh_quote(&gh.to_string_lossy())
    );
    let r = shell_run(&cmd, STATUS_TIMEOUT, &AtomicBool::new(false), &mut |_| {})
        .map_err(|e| format!("gh를 실행하지 못했어요: {e}"))?;
    if r.timed_out {
        return Err("gh가 5초 안에 답하지 않았어요.".into());
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
}

#[derive(Default)]
pub struct GithubJobs {
    pub current: Mutex<Option<GithubJob>>,
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

/// 연결 해제 = IEUM 기록만 삭제. gh에서 로그아웃시키지 않는다.
#[tauri::command]
pub async fn github_disconnect(active: State<'_, Active>) -> Result<GithubStatus, String> {
    clear_record(&active.account_dir().await?)?;
    status_of(&active).await
}

/// [브라우저 열기]가 여는 고정 주소. gh 출력의 주소는 쓰지 않는다.
pub const DEVICE_URL: &str = "https://github.com/login/device";
pub const LOGIN_CMD_ARGS: &str =
    "auth login --web --hostname github.com --git-protocol https --skip-ssh-key";
const NO_CODE: &str = "gh 출력을 읽지 못했어요";
/// 로그인 코드를 기다리는 시간·로그인 전체 상한(device code 만료).
const CODE_WAIT: Duration = Duration::from_secs(10);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// device 로그인. stderr에서 처음 읽은 코드로 `on_code`를 한 번 부른다.
/// `code_wait` 안에 코드가 없으면 `cancel`을 세우고 실패.
pub fn run_login(
    gh: &Path,
    cancel: &AtomicBool,
    code_wait: Duration,
    limit: Duration,
    on_code: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let cmd = format!("{} {LOGIN_CMD_ARGS}", sh_quote(&gh.to_string_lossy()));
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
struct JobGuard<'a>(&'a GithubJobs);

impl<'a> JobGuard<'a> {
    fn start(jobs: &'a GithubJobs, kind: JobKind, cancel: Arc<AtomicBool>) -> Result<Self, String> {
        let mut cur = jobs.current.lock().map_err(|e| e.to_string())?;
        if cur.is_some() {
            return Err("이미 진행 중이에요".into());
        }
        *cur = Some(GithubJob { kind, cancel });
        Ok(Self(jobs))
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
    let app_data = active.app_data().to_path_buf();
    let gh = match blocking(move || find_gh(&app_data)).await? {
        Ok(g) => g,
        Err(s) => return Ok(s),
    };
    let path = gh.path.clone();
    let login = match ready_login(&blocking(move || gh_auth(&path)).await??) {
        Some(l) => l,
        None => {
            let cancel = Arc::new(AtomicBool::new(false));
            let _job = JobGuard::start(&jobs, JobKind::Login, cancel.clone())?;
            let (v0, v1, v2) = gh.version;
            let path = gh.path.clone();
            blocking(move || {
                run_login(&path, &cancel, CODE_WAIT, LOGIN_TIMEOUT, &mut |code| {
                    let _ = app.emit("github-login-code", serde_json::json!({ "code": code }));
                })
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
