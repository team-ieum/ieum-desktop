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
    // `exec` — SIGHUP이 셸이 아니라 CLI에 바로 간다.
    line += &match tool {
        // 첫 지시가 `--allowedTools`보다 앞: 그 옵션은 여러 값을 받아 뒤에 오는 글을 삼킨다.
        Tool::Claude => format!(
            "exec claude {prompt} --allowedTools {}",
            sh_quote(CLAUDE_ALLOW)
        ),
        Tool::Codex => format!("exec codex {prompt}"),
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
        // 부모 환경은 그대로 물려받는다(`CLAUDE_CONFIG_DIR` 등을 지우지 않는다).
        let mut cmd = CommandBuilder::new(&s.program);
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

/// 작업 폴더를 만들고 실행 줄·환경을 조립한다. `root` = 계정 폴더(`Store::root`).
/// 토큰은 프로세스 환경으로만 넘긴다 — 실행 줄에 실으면 `ps`에 보인다.
pub async fn prepare(
    root: &Path,
    workflow_id: &str,
    tool: Tool,
    account_dir: Option<PathBuf>,
    gate: &Gate,
    cols: u16,
    rows: u16,
) -> Result<Prepared, String> {
    // 저장소와 같은 검사(UUID 정규형) — id가 계정 폴더 밖을 가리키지 못한다.
    if !uuid::Uuid::try_parse(workflow_id).is_ok_and(|u| u.to_string() == workflow_id) {
        return Err(format!("잘못된 id예요: {workflow_id:?}"));
    }
    let cwd = root.join("chat-work").join(workflow_id);
    std::fs::create_dir_all(&cwd).map_err(|e| format!("{}: {e}", cwd.display()))?;
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

/// 세션을 띄운다. 세션이 끝나면(스스로 끝남·kill) 토큰을 닫고 `on_exit`를 부른다. 띄우지 못해도 토큰은 닫는다.
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
    let started = terms.spawn(
        workflow_id,
        tool,
        p.spawn,
        sink,
        Box::new(move || {
            if let Some(t) = &token {
                close(t);
            }
            on_exit();
        }),
    );
    if let (Err(_), Some(t)) = (&started, &p.token) {
        (gate.close)(t);
    }
    started
}

/// 드롭다운에 보일 CLI인가 — 설치됐고 로그아웃 상태가 아니다. 확인 불가(`unavailable`)는 숨기지 않는다.
pub fn usable(installed: bool, plan_state: &str) -> bool {
    installed && plan_state != "not_logged_in"
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
    pub tools: Vec<Tool>,
    pub running: Option<Tool>,
}

/// 앱 기본 계정의 CLI 설정 폴더. 이 Mac의 로그인이거나 못 구하면 None(agy는 계정 폴더가 없다). 파일을 읽는다.
fn account_dir(creds: &AccountCredentials, tool: Tool) -> Option<PathBuf> {
    match tool {
        Tool::Claude => creds.claude_config_dir(None).ok().flatten(),
        Tool::Codex => {
            let id = creds
                .list()
                .codex_accounts
                .into_iter()
                .find(|m| m.is_default)?
                .id;
            creds.cli_dir("codex", &id).ok().flatten()
        }
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
    let root = {
        let sup = active.sup().await?;
        sup.store().get_workflow(&workflow_id)?; // 없는 워크플로우 거부
        sup.store().root().to_path_buf()
    };
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
    let p = prepare(&root, &workflow_id, tool, account, &gate, cols, rows).await?;
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
            usable(
                installed(Tool::Claude).await,
                sub("claude", claude_dir).await,
            )
        },
        async { usable(installed(Tool::Codex).await, sub("codex", codex_dir).await) },
        async { usable(installed(Tool::Agy).await, agy.get().await.state) },
    );
    let tools = [
        (Tool::Claude, claude),
        (Tool::Codex, codex),
        (Tool::Agy, agy),
    ]
    .into_iter()
    .filter_map(|(t, ok)| ok.then_some(t))
    .collect();
    Ok(TermTools { tools, running })
}
