//! 앱 안 터미널 — 사용자의 CLI(Claude Code·Codex·Antigravity)를 그대로 띄운다.
//! 화면은 도구 종류만 넘기고, 실행 줄·환경·작업 폴더는 전부 여기서 만든다.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ieum_local::ai::{CredentialResolver, shell_output};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::active::Active;
use crate::credentials::AccountCredentials;
use crate::skill::install_skill;
use crate::sub_usage::{AntigravityUsageClient, SubscriptionUsage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Codex,
    Agy,
}

impl Tool {
    pub fn program(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Agy => "agy",
        }
    }
}

pub const FIRST_PROMPT: &str =
    "IEUM 워크플로우를 만든다. 먼저 `ieum workflow guide`를 실행해 안내를 읽고 따르라.";
/// 묻지 않고 허용하는 것은 이것뿐 — 권한을 통째로 푸는 옵션은 넘기지 않는다.
pub const CLAUDE_ALLOW: &str = "Bash(ieum workflow *)";

#[derive(Default)]
pub struct Launch {
    /// 앱 번들의 `ieum`이 있는 폴더.
    pub bin_dir: Option<PathBuf>,
    /// 앱 기본 계정의 CLI 설정 폴더.
    pub account_dir: Option<PathBuf>,
}

/// 셸 작은따옴표 인용(`cli.rs`의 것은 비공개).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `$SHELL -lc`에 넘길 한 줄. `PATH`·계정 폴더는 로그인 셸 프로필이 덮지 못하게 줄 안에서 export한다.
pub fn launch_line(tool: Tool, l: &Launch) -> String {
    let mut line = String::new();
    if let Some(bin) = &l.bin_dir {
        line += &format!(
            "export PATH={}:\"$PATH\"; ",
            sh_quote(&bin.to_string_lossy())
        );
    }
    let var = match tool {
        Tool::Claude => Some("CLAUDE_CONFIG_DIR"),
        Tool::Codex => Some("CODEX_HOME"),
        Tool::Agy => None,
    };
    if let (Some(var), Some(dir)) = (var, &l.account_dir) {
        line += &format!("export {var}={}; ", sh_quote(&dir.to_string_lossy()));
    }
    let prompt = sh_quote(FIRST_PROMPT);
    // 반향을 끈다 — agy는 원시 모드로 바꾸기 전에 단말 질의(`ESC[c`)를 보내, 화면의 답(`ESC[?1;2c`)이
    // 반향 모드에 닿으면 `^[[?1;2c`로 찍힌다. CLI는 어차피 원시 모드로 바꾼다.
    line += "stty -echo; ";
    // `exec` — SIGHUP이 셸이 아니라 CLI에 바로 간다.
    line += &match tool {
        // 첫 지시가 `--allowedTools`보다 앞: 그 옵션은 여러 값을 받아 뒤에 오는 글을 삼킨다.
        Tool::Claude => format!(
            "exec claude {prompt} --allowedTools {}",
            sh_quote(CLAUDE_ALLOW)
        ),
        // `--no-daemon` — 공용 데몬(ChatGPT 앱 등이 띄움)에 붙으면 명령이 데몬 환경에서 돌아
        // PATH·IEUM_SOCKET·IEUM_TOKEN이 닿지 않는다(`ieum: command not found`, 11-15 실측).
        // `-a on-request` — 샌드박스가 앱 소켓을 막으므로 ieum 명령마다 사람에게 허용을 묻게 한다.
        // 기본 정책(granular, sandbox_approval=false)이면 묻지 않고 포기한다(11-15 실측).
        Tool::Codex => format!("exec codex --no-daemon -a on-request {prompt}"),
        Tool::Agy => format!("exec agy -i {prompt}"),
    };
    line
}

// ── PTY 세션 ──

/// 터미널 출력을 받는 쪽(화면의 Channel).
pub type Sink = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
type OnEnd = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;
/// (최근 출력, 지금 붙어 있는 sink) — 한 잠금으로 묶어 다시 흘리기와 새 출력 사이에 빠지는 바이트가 없게 한다.
type Out = Arc<Mutex<(VecDeque<u8>, Sink)>>;

pub struct Spawn {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

/// 다시 붙을 때 흘려 줄 최근 출력의 상한.
pub const SCROLLBACK: usize = 256 * 1024;

struct Session {
    tool: Tool,
    /// 작업 폴더 — 같은 폴더를 쓰는 세션이 남아 있으면 비우지 않는다.
    cwd: PathBuf,
    /// 같은 워크플로우에 새로 띄운 세션을 옛 읽기 스레드가 지우지 않게.
    generation: u64,
    master: Box<dyn MasterPty + Send>,
    /// 세션마다 따로 잠근다 — 자식이 입력을 읽지 않으면 쓰기가 멈추는데, 그동안 세션 맵을 쥐고 있으면 안 된다.
    writer: Writer,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    out: Out,
    on_end: OnEnd,
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;
type Map = Arc<Mutex<HashMap<String, Session>>>;

/// 맵 잠금 밖에서 부른다. 자식이 죽으면(kill) 멈춰 있던 쓰기는 오류로 풀린다.
fn write_to(writer: &Writer, data: &[u8]) -> Result<(), String> {
    let mut w = lock(writer);
    w.write_all(data)
        .and_then(|_| w.flush())
        .map_err(|e| e.to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn run_on_end(on_end: &OnEnd) {
    let f = lock(on_end).take();
    if let Some(f) = f {
        f();
    }
}

/// 워크플로우당 세션 하나. 화면(xterm)이 사라져도 PTY는 여기 남는다.
#[derive(Default)]
pub struct Terms {
    map: Map,
    next: AtomicU64,
}

impl Terms {
    /// 이미 세션이 있으면 Err. `on_end`는 세션이 끝날 때(스스로 끝남·kill) 정확히 한 번.
    pub fn spawn(
        &self,
        workflow_id: &str,
        tool: Tool,
        s: Spawn,
        sink: Sink,
        on_end: Box<dyn FnOnce() + Send>,
    ) -> Result<(), String> {
        // 끝까지 잠근다 — 읽기 스레드가 맵에 들어가기 전의 세션을 지우려 들지 않게.
        let mut map = lock(&self.map);
        if map.contains_key(workflow_id) {
            return Err("이 워크플로우의 터미널이 이미 열려 있어요.".into());
        }
        let pair = native_pty_system()
            .openpty(size(s.cols, s.rows))
            .map_err(|e| format!("터미널을 열지 못했어요: {e}"))?;
        // 부모 환경은 물려받되(`CLAUDE_CONFIG_DIR` 등은 그대로) 앱을 띄운 CLI 세션의 표식은 뺀다.
        let mut cmd = CommandBuilder::new(&s.program);
        for (k, _) in std::env::vars_os() {
            if k.to_str().is_some_and(session_marker) {
                cmd.env_remove(&k);
            }
        }
        cmd.args(&s.args);
        cmd.cwd(&s.cwd);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &s.env {
            cmd.env(k, v);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("터미널을 열지 못했어요: {e}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("터미널을 열지 못했어요: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("터미널을 열지 못했어요: {e}"))?;
        let generation = self.next.fetch_add(1, Ordering::Relaxed);
        let out: Out = Arc::new(Mutex::new((VecDeque::new(), sink)));
        let on_end: OnEnd = Arc::new(Mutex::new(Some(on_end)));
        map.insert(
            workflow_id.to_string(),
            Session {
                tool,
                cwd: s.cwd.clone(),
                generation,
                master: pair.master,
                writer: Arc::new(Mutex::new(writer)),
                child,
                out: out.clone(),
                on_end: on_end.clone(),
            },
        );
        let (map, id) = (self.map.clone(), workflow_id.to_string());
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let mut o = lock(&out);
                o.0.extend(&buf[..n]);
                let over = o.0.len().saturating_sub(SCROLLBACK);
                o.0.drain(..over);
                (o.1)(buf[..n].to_vec());
            }
            // 스스로 끝남. kill이 먼저 뺐거나 새 세션이 들어왔으면 건드리지 않는다.
            let ended = {
                let mut map = lock(&map);
                match map.get(&id) {
                    Some(s) if s.generation == generation => map.remove(&id),
                    _ => None,
                }
            };
            if let Some(mut s) = ended {
                let _ = s.child.wait(); // 좀비를 남기지 않는다
            }
            run_on_end(&on_end);
        });
        Ok(())
    }

    /// 살아 있는 세션에 붙는다: 쌓인 출력(최대 `SCROLLBACK`)을 `sink`로 한 번 흘리고 이후 출력을 `sink`로 돌린다.
    pub fn attach(&self, workflow_id: &str, sink: Sink) -> Option<Tool> {
        let map = lock(&self.map);
        let s = map.get(workflow_id)?;
        let mut o = lock(&s.out);
        if !o.0.is_empty() {
            sink(o.0.iter().copied().collect());
        }
        o.1 = sink;
        Some(s.tool)
    }

    pub fn running(&self, workflow_id: &str) -> Option<Tool> {
        lock(&self.map).get(workflow_id).map(|s| s.tool)
    }

    /// 자식이 입력을 읽지 않으면 멈출 수 있다 — 맵 잠금은 쥐지 않으므로 그동안에도 kill·다른 세션은 된다.
    pub fn write(&self, workflow_id: &str, data: &[u8]) -> Result<(), String> {
        write_to(&self.writer(workflow_id)?, data)
    }

    fn writer(&self, workflow_id: &str) -> Result<Writer, String> {
        let map = lock(&self.map);
        Ok(map.get(workflow_id).ok_or(NO_SESSION)?.writer.clone())
    }

    pub fn resize(&self, workflow_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let map = lock(&self.map);
        let s = map.get(workflow_id).ok_or(NO_SESSION)?;
        s.master.resize(size(cols, rows)).map_err(|e| e.to_string())
    }

    /// 없으면 아무 일도 안 한다.
    pub fn kill(&self, workflow_id: &str) {
        let s = lock(&self.map).remove(workflow_id);
        if let Some(s) = s {
            end(s);
        }
    }

    pub fn kill_all(&self) {
        let all: Vec<Session> = lock(&self.map).drain().map(|(_, s)| s).collect();
        all.into_iter().for_each(end);
    }

    /// 작업 폴더 `dir`을 쓰는 세션이 없을 때만 그 안의 내용을 비운다(폴더는 남긴다).
    pub fn clear_work_if_idle(&self, dir: &Path) {
        clear_if_idle(&self.map, dir);
    }
}

/// 맵을 쥔 채 비운다 — 그사이 같은 폴더에 새 세션이 들어오지 못한다.
fn clear_if_idle(map: &Map, dir: &Path) {
    let map = lock(map);
    if !map.values().any(|s| s.cwd == dir) {
        clear_work(dir);
    }
}

/// 앱을 띄운 CLI가 자식에게 남기는 세션 표식 — 터미널의 CLI가 자기를 하위 세션으로 착각한다.
/// Claude Code: `CLAUDECODE`, `CLAUDE_CODE_*`. Codex·agy: 자식 셸에 넣는 것(바이너리 문자열로 확인).
pub fn session_marker(key: &str) -> bool {
    key == "CLAUDECODE"
        || key.starts_with("CLAUDE_CODE_")
        || matches!(
            key,
            "CODEX_SANDBOX"
                | "CODEX_SANDBOX_NETWORK_DISABLED"
                | "CODEX_THREAD_ID"
                | "CODEX_TOOL_CALL_ID"
                | "ANTIGRAVITY_AGENT"
                | "ANTIGRAVITY_CONVERSATION_ID"
        )
}

const NO_SESSION: &str = "열려 있는 터미널이 없어요.";

/// 맵에서 뺀 세션을 끝낸다. EOF를 기다리지 않는다 — 손자가 PTY를 물고 있으면 EOF가 안 온다.
// ponytail: 죽이는 것은 셸이 `exec`한 CLI 하나다(SIGHUP, 0.25초 뒤에도 살아 있으면 SIGKILL — portable-pty).
// CLI가 띄운 손자가 SIGHUP을 무시하면 남을 수 있다. 필요해지면 프로세스 그룹째 kill.
fn end(mut s: Session) {
    let _ = s.child.kill();
    let _ = s.child.wait();
    run_on_end(&s.on_end);
}

// ── 세션 환경(소켓 토큰 이음매) ──

pub struct GateEnv {
    pub socket: PathBuf,
    pub token: String,
    /// 앱 번들의 `ieum`이 있는 폴더.
    pub bin_dir: PathBuf,
}

type GateOpen = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<Option<GateEnv>, String>> + Send>>
        + Send
        + Sync,
>;

/// 소켓 통로(`Channel`)와의 이음매: `open(workflow_id)` = 그 세션 전용 토큰을 만들어 워크플로우에 묶고 환경을 준다,
/// `close(token)` = 폐기. 통로가 붙기 전에는 `no_gate()`.
#[derive(Clone)]
pub struct Gate {
    pub open: GateOpen,
    pub close: Arc<dyn Fn(&str) + Send + Sync>,
}

/// 환경 없이 터미널만 열린다.
pub fn no_gate() -> Gate {
    Gate {
        open: Arc::new(|_| Box::pin(async { Ok(None) })),
        close: Arc::new(|_| {}),
    }
}

pub struct Prepared {
    pub spawn: Spawn,
    pub token: Option<String>,
}

/// 계정의 터미널 작업 폴더: `<앱 캐시 폴더>/chat-work/<계정 키>`. 앱 데이터 폴더 밖이라
/// 터미널의 CLI가 상대 경로로 실행 기록·워크플로우 파일에 닿지 못한다.
/// 폴더는 늘 같은 자리에 남는다 — CLI의 "이 폴더를 신뢰하나요?"를 한 번만 묻게.
pub fn work_root(cache_dir: &Path, account_key: &str) -> PathBuf {
    cache_dir.join("chat-work").join(account_key)
}

/// 작업 폴더 안의 내용만 지운다. 폴더 자체가 심볼릭 링크면 건드리지 않고,
/// 안의 링크는 링크만 지운다(가리키는 곳은 그대로 — `remove_dir_all`도 링크를 따라가지 않는다).
fn clear_work(dir: &Path) {
    if !std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let r = match e.file_type() {
            Ok(t) if t.is_dir() => std::fs::remove_dir_all(&p),
            _ => std::fs::remove_file(&p),
        };
        if let Err(e) = r {
            eprintln!(
                "[ieum] 터미널 작업 폴더를 비우지 못했어요: {}: {e}",
                p.display()
            );
        }
    }
}

/// 예전 위치(`<계정 폴더>/chat-work/<id>`)에 남은 작업 폴더를 비어 있는 것만 지운다.
pub fn clean_old_work(account_root: &Path) {
    let old = account_root.join("chat-work");
    // `remove_dir`는 빈 폴더만 지운다.
    for e in std::fs::read_dir(&old).into_iter().flatten().flatten() {
        let _ = std::fs::remove_dir(e.path());
    }
    let _ = std::fs::remove_dir(&old);
}

/// 계정의 작업 폴더(`work_root`, 본인 전용)를 만들고 실행 줄·환경을 조립한다. `work_root` = `work_root()`.
/// 폴더를 비우는 것은 `start`가 한다(그 계정에 살아 있는 세션이 없을 때만).
/// 토큰은 프로세스 환경으로만 넘긴다 — 실행 줄에 실으면 `ps`에 보인다.
pub async fn prepare(
    work_root: &Path,
    workflow_id: &str,
    tool: Tool,
    account_dir: Option<PathBuf>,
    gate: &Gate,
    cols: u16,
    rows: u16,
) -> Result<Prepared, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    // 저장소와 같은 검사(UUID 정규형) — id가 작업 폴더 자리 밖을 가리키지 못한다.
    if !uuid::Uuid::try_parse(workflow_id).is_ok_and(|u| u.to_string() == workflow_id) {
        return Err(format!("잘못된 id예요: {workflow_id:?}"));
    }
    let (mut env, mut token, mut bin_dir) = (Vec::new(), None, None);
    if let Some(g) = (gate.open)(workflow_id.to_string()).await? {
        env.push((
            "IEUM_SOCKET".to_string(),
            g.socket.to_string_lossy().into_owned(),
        ));
        env.push(("IEUM_TOKEN".to_string(), g.token.clone()));
        token = Some(g.token);
        bin_dir = Some(g.bin_dir);
    }
    // 거절된 세션이 폴더를 만들지 않게 관문 뒤에 만든다. 이미 있던 폴더도 본인 전용으로 맞춘다.
    let cwd = work_root.to_path_buf();
    let made = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&cwd)
        .and_then(|()| std::fs::set_permissions(&cwd, std::fs::Permissions::from_mode(0o700)));
    if let Err(e) = made {
        if let Some(t) = &token {
            (gate.close)(t);
        }
        return Err(format!("{}: {e}", cwd.display()));
    }
    Ok(Prepared {
        spawn: Spawn {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()),
            args: vec![
                "-lc".into(),
                launch_line(
                    tool,
                    &Launch {
                        bin_dir,
                        account_dir,
                    },
                ),
            ],
            cwd,
            env,
            cols,
            rows,
        },
        token,
    })
}

/// 세션을 띄운다. 작업 폴더(`p.spawn.cwd`)는 시작 전과 끝난 뒤(스스로 끝남·kill·띄우지 못함) 비운다 —
/// 같은 폴더를 쓰는 다른 세션이 살아 있으면 그대로 둔다. 끝나면 토큰을 닫고 `on_exit`를 부른다.
pub fn start(
    terms: &Terms,
    workflow_id: &str,
    tool: Tool,
    p: Prepared,
    gate: &Gate,
    sink: Sink,
    on_exit: Box<dyn FnOnce() + Send>,
) -> Result<(), String> {
    let (close, token) = (gate.close.clone(), p.token.clone());
    let (work, map) = (p.spawn.cwd.clone(), terms.map.clone());
    terms.clear_work_if_idle(&work);
    let at_end = work.clone();
    let started = terms.spawn(
        workflow_id,
        tool,
        p.spawn,
        sink,
        Box::new(move || {
            if let Some(t) = &token {
                close(t);
            }
            clear_if_idle(&map, &at_end);
            on_exit();
        }),
    );
    if started.is_err() {
        if let Some(t) = &p.token {
            (gate.close)(t);
        }
        terms.clear_work_if_idle(&work);
    }
    started
}

/// 고를 수 있는 CLI인가 — 설치됐고 로그아웃 상태가 아니다. 확인 불가(`unavailable`)는 숨기지 않는다.
pub fn usable(installed: bool, plan_state: &str) -> bool {
    installed && plan_state != "not_logged_in"
}

/// CLI 카드의 상태: `ready`(로그인됨) · `unknown`(확인 불가, 고를 수는 있음) · `notInstalled` · `loggedOut`.
pub fn cli_state(installed: bool, plan_state: &str) -> &'static str {
    match (installed, plan_state) {
        (false, _) => "notInstalled",
        (_, "not_logged_in") => "loggedOut",
        (_, "ok") => "ready",
        _ => "unknown",
    }
}

// ── Tauri 명령 ──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermStart {
    pub tool: Tool,
    /// 살아 있던 세션에 붙었다(새로 띄우지 않았다).
    pub attached: bool,
    pub skill_installed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TermTools {
    /// 고를 수 있는 것(`usable`).
    pub tools: Vec<Tool>,
    /// 세 CLI 모두, 카드 순서대로 상태(`cli_state`).
    pub clis: Vec<TermCli>,
    pub running: Option<Tool>,
}

#[derive(Serialize)]
pub struct TermCli {
    pub tool: Tool,
    pub state: &'static str,
}

/// 앱 기본 계정의 CLI 설정 폴더. 이 Mac의 로그인이거나 못 구하면 None(agy는 계정 폴더가 없다). 파일을 읽는다.
pub fn account_dir(creds: &AccountCredentials, tool: Tool) -> Option<PathBuf> {
    match tool {
        Tool::Claude => creds.claude_config_dir(None).ok().flatten(),
        // `list()`는 API 키(키체인)까지 읽는다 — 파일만 읽는 쪽으로.
        Tool::Codex => creds.codex_home(None).ok().flatten(),
        Tool::Agy => None,
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

/// 터미널을 띄우거나, 이 워크플로우의 세션이 살아 있으면 거기에 붙는다(최근 출력을 다시 흘린다).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn term_spawn(
    app: AppHandle,
    active: State<'_, Active>,
    terms: State<'_, Terms>,
    gate: State<'_, Gate>,
    workflow_id: String,
    tool: Tool,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Result<TermStart, String> {
    let old_root = {
        let sup = active.sup().await?;
        sup.store().get_workflow(&workflow_id)?; // 없는 워크플로우 거부
        sup.store().root().to_path_buf()
    };
    let key = active
        .account_key()
        .await
        .ok_or(crate::active::NOT_SIGNED_IN)?;
    let work = work_root(
        &app.path().app_cache_dir().map_err(|e| e.to_string())?,
        &key,
    );
    let sink: Sink = Arc::new(move |b| {
        let _ = on_data.send(InvokeResponseBody::Raw(b));
    });
    if let Some(tool) = terms.attach(&workflow_id, sink.clone()) {
        return Ok(TermStart {
            tool,
            attached: true,
            skill_installed: false,
        });
    }
    let creds = active.credentials().await?;
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let (account, skill_installed) = blocking(move || {
        clean_old_work(&old_root);
        let account = account_dir(&creds, tool);
        let (mut claude, mut codex) = (vec![home.join(".claude")], vec![home.join(".codex")]);
        match (tool, &account) {
            (Tool::Claude, Some(d)) => claude.push(d.clone()),
            (Tool::Codex, Some(d)) => codex.push(d.clone()),
            _ => {}
        }
        let installed = match install_skill(&home, &claude, &codex) {
            Ok(r) => r.installed,
            Err(e) => {
                eprintln!("[ieum] {e}");
                false
            }
        };
        (account, installed)
    })
    .await?;
    let p = prepare(&work, &workflow_id, tool, account, &gate, cols, rows).await?;
    let id = workflow_id.clone();
    start(
        &terms,
        &workflow_id,
        tool,
        p,
        &gate,
        sink,
        Box::new(move || {
            let _ = app.emit("term-exit", json!({"workflowId": id}));
        }),
    )?;
    Ok(TermStart {
        tool,
        attached: false,
        skill_installed,
    })
}

#[tauri::command]
pub async fn term_write(
    terms: State<'_, Terms>,
    workflow_id: String,
    data: String,
) -> Result<(), String> {
    // 멈출 수 있는 쓰기라 런타임 스레드 밖에서.
    let writer = terms.writer(&workflow_id)?;
    blocking(move || write_to(&writer, data.as_bytes())).await?
}

#[tauri::command]
pub async fn term_resize(
    terms: State<'_, Terms>,
    workflow_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    terms.resize(&workflow_id, cols, rows)
}

#[tauri::command]
pub async fn term_kill(terms: State<'_, Terms>, workflow_id: String) -> Result<(), String> {
    terms.kill(&workflow_id);
    Ok(())
}

async fn installed(tool: Tool) -> bool {
    blocking(move || {
        shell_output(
            &format!("command -v {}", tool.program()),
            Duration::from_secs(5),
        )
        .is_some()
    })
    .await
    .unwrap_or(false)
}

/// 쓸 수 있는 CLI(설치 + 로그인)와 이 워크플로우에 살아 있는 세션.
#[tauri::command]
pub async fn term_tools(
    active: State<'_, Active>,
    terms: State<'_, Terms>,
    usage: State<'_, SubscriptionUsage>,
    agy: State<'_, AntigravityUsageClient>,
    workflow_id: String,
) -> Result<TermTools, String> {
    let running = terms.running(&workflow_id);
    let creds = active.credentials().await?;
    let (claude_dir, codex_dir) = blocking(move || {
        (
            account_dir(&creds, Tool::Claude),
            account_dir(&creds, Tool::Codex),
        )
    })
    .await?;
    // 상태를 못 읽으면 `unavailable` — 숨기지 않는다.
    let sub = |kind: &'static str, dir| {
        let usage = &usage;
        async move {
            usage
                .get(kind, dir)
                .await
                .map_or("unavailable", |u| u.state)
        }
    };
    let (claude, codex, agy) = tokio::join!(
        async {
            (
                installed(Tool::Claude).await,
                sub("claude", claude_dir).await,
            )
        },
        async { (installed(Tool::Codex).await, sub("codex", codex_dir).await) },
        async { (installed(Tool::Agy).await, agy.get().await.state) },
    );
    let all = [
        (Tool::Claude, claude),
        (Tool::Codex, codex),
        (Tool::Agy, agy),
    ];
    let tools = all
        .iter()
        .filter_map(|&(t, (i, s))| usable(i, s).then_some(t))
        .collect();
    let clis = all
        .iter()
        .map(|&(tool, (i, s))| TermCli {
            tool,
            state: cli_state(i, s),
        })
        .collect();
    Ok(TermTools {
        tools,
        clis,
        running,
    })
}
