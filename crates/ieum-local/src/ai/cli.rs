//! 구독 CLI 공급원(claude·codex). 감지·실행 모두 `$SHELL -lc`(사용자 PATH), 프롬프트는 stdin.
//! 빈 폴더에서 도구를 막고 돌린다. 취소·시간 초과 시 프로세스 그룹째 kill.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::{AiError, AiProvider, AiRequest, ChunkFn};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// 실패 메시지에 붙이는 stderr 끝부분(글자).
const STDERR_TAIL: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    /// 보류(2026-11-10 사용자 결정): 아래 차단 플래그를 다 붙여도 codex 0.167 exec에 도구가 남는다 —
    /// functions.exec(code-mode 호스트를 꺼 실행은 실패)·functions.wait·functions.request_user_input_async·
    /// collaboration.{spawn_agent,send_message,wait_agent,list_agents,interrupt_agent,followup_task}.
    /// 도구를 완전히 끄는 옵션이 생기면 `AiRunner::new`·`detect_clis`·`local.ai.json`에 다시 켠다.
    /// 그때까지 코드·fixture·테스트는 유지한다.
    Codex,
}

/// codex exec에서 끄는 기본 켜진 기능(`codex features list` 0.167 기준).
const CODEX_DISABLED_FEATURES: [&str; 20] = [
    "apps",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "computer_use",
    "shell_tool",
    "unified_exec",
    "multi_agent",
    "image_generation",
    "view_image",
    "plugins",
    "hooks",
    "in_app_browser",
    "in_app_local_automation",
    "sleep_tool",
    "tool_suggest",
    "skill_search",
    "skill_mcp_dependency_install",
    "code_mode_host",
    "goals",
];

impl CliKind {
    /// `local.ai` provider 값.
    pub fn id(self) -> &'static str {
        match self {
            CliKind::Claude => "cli:claude",
            CliKind::Codex => "cli:codex",
        }
    }

    fn program(self) -> &'static str {
        match self {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
        }
    }

    /// 도구 차단 포함 인자. claude: 내장 도구 0 + MCP 0 + 사용자 설정 파일 안 읽음 + safe-mode(CLAUDE.md·스킬·
    /// 플러그인·훅 끔) + 세션 저장 안 함. codex: 읽기 전용 샌드박스.
    fn args(self, req: &AiRequest, schema_file: Option<&Path>) -> Vec<String> {
        let mut a: Vec<String> = match self {
            CliKind::Claude => [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--tools",
                "",
                "--strict-mcp-config",
                "--setting-sources",
                "",
                "--safe-mode",
                "--no-session-persistence",
                "--disable-slash-commands",
            ]
            .map(String::from)
            .to_vec(),
            CliKind::Codex => {
                let mut a: Vec<String> = [
                    "exec",
                    "--json",
                    "--skip-git-repo-check",
                    "-s",
                    "read-only",
                    "--ignore-user-config",
                    "--ignore-rules",
                    "--ephemeral",
                    "-c",
                    "web_search=\"disabled\"",
                ]
                .map(String::from)
                .to_vec();
                for f in CODEX_DISABLED_FEATURES {
                    a.push("--disable".into());
                    a.push(f.into());
                }
                a
            }
        };
        if let Some(m) = &req.model {
            a.push(
                if self == CliKind::Claude {
                    "--model"
                } else {
                    "-m"
                }
                .into(),
            );
            a.push(m.clone());
        }
        match (self, &req.schema, schema_file) {
            (CliKind::Claude, Some(s), _) => {
                a.push("--json-schema".into());
                a.push(s.to_string());
            }
            (CliKind::Codex, _, Some(f)) => {
                a.push("--output-schema".into());
                a.push(f.to_string_lossy().into_owned());
            }
            _ => {}
        }
        if self == CliKind::Codex {
            a.push("-".into()); // 프롬프트를 stdin에서
        }
        a
    }
}

/// 셸 작은따옴표 인용.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

/// JSON 줄을 읽어 조각·최종 답·오류를 모은다.
struct StreamState {
    kind: CliKind,
    answer: Option<String>,
    /// 답이 있어도 실패(claude `is_error`).
    error: Option<String>,
    /// codex `error`·`turn.failed` — 재연결 경고일 수 있어 답이 없거나 rc≠0일 때만 실패 이유로 쓴다.
    warnings: Vec<String>,
}

impl StreamState {
    fn new(kind: CliKind) -> Self {
        StreamState {
            kind,
            answer: None,
            error: None,
            warnings: Vec::new(),
        }
    }

    fn feed(&mut self, line: &str, chunk: &(dyn Fn(&str) + '_)) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let ty = v["type"].as_str().unwrap_or("");
        match (self.kind, ty) {
            (CliKind::Claude, "assistant") => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    if c["type"] == "text"
                        && let Some(t) = c["text"].as_str()
                    {
                        chunk(t);
                    }
                }
            }
            (CliKind::Claude, "result") => {
                if v["is_error"] == true {
                    let msg = v["result"].as_str().or(v["subtype"].as_str());
                    self.error = Some(msg.unwrap_or("알 수 없는 오류").to_string());
                } else if !v["structured_output"].is_null() {
                    self.answer = Some(v["structured_output"].to_string());
                } else if let Some(r) = v["result"].as_str() {
                    self.answer = Some(r.to_string());
                }
            }
            (CliKind::Codex, "item.completed") if v["item"]["type"] == "agent_message" => {
                if let Some(t) = v["item"]["text"].as_str() {
                    chunk(t);
                    self.answer = Some(t.to_string());
                }
            }
            (CliKind::Codex, "turn.failed") => {
                self.warnings
                    .extend(v["error"]["message"].as_str().map(String::from));
            }
            (CliKind::Codex, "error") => {
                self.warnings
                    .extend(v["message"].as_str().map(String::from));
            }
            _ => {}
        }
    }

    fn finish(self, exited_ok: bool, stderr: &str) -> Result<String, AiError> {
        let name = self.kind.program();
        if let Some(e) = self.error {
            return Err(AiError(format!("{name}: {e}")));
        }
        match self.answer {
            Some(a) if exited_ok => Ok(a),
            _ if !self.warnings.is_empty() => {
                Err(AiError(format!("{name}: {}", self.warnings.join(" / "))))
            }
            _ => {
                let tail: String = {
                    let s = stderr.trim();
                    let skip = s.chars().count().saturating_sub(STDERR_TAIL);
                    s.chars().skip(skip).collect()
                };
                Err(AiError(format!("{name} CLI가 답을 주지 못했어요: {tail}")))
            }
        }
    }
}

/// 저장된 출력 줄(fixture 등)을 해석한다 — 실행 때와 같은 해석기.
pub fn parse_stream<'a>(
    kind: CliKind,
    lines: impl IntoIterator<Item = &'a str>,
    chunk: &ChunkFn<'_>,
) -> Result<String, AiError> {
    let mut st = StreamState::new(kind);
    for l in lines {
        st.feed(l, chunk);
    }
    st.finish(true, "")
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
const SIGKILL: i32 = 9;

fn kill_group(pgid: i32) {
    // SAFETY: 단순 시스템 호출. 그룹이 이미 없으면 ESRCH로 끝난다.
    unsafe {
        kill(-pgid, SIGKILL);
    }
}

/// 버려질 때 프로세스 그룹 전체를 kill한다(CLI가 띄운 자식까지).
// ponytail: 정상 종료 뒤에도 kill(-pgid)을 보낸다 — 남은 손자 정리용. pgid 재사용 경합은 무시할 만큼 짧다.
struct KillGroup(i32);

impl Drop for KillGroup {
    fn drop(&mut self) {
        kill_group(self.0);
    }
}

/// CLI stdout 전체 상한. 넘으면 kill하고 실패.
const MAX_STDOUT: u64 = 10 * 1024 * 1024;
/// `detect_clis` 한 CLI당 시간 상한.
const DETECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 모아 두는 stderr 상한(실패 메시지에는 끝 500자만 쓴다).
const MAX_STDERR: u64 = 1024 * 1024;

/// 모델 이름이 CLI 플래그로 읽히거나 인자를 깨지 않게. CLI를 띄우는 모든 경로(`ask`)에서 검사한다.
fn valid_model(m: &str) -> bool {
    !m.starts_with('-') && !m.chars().any(|c| c.is_whitespace() || c.is_control())
}

static SCHEMA_SEQ: AtomicU64 = AtomicU64::new(0);

/// codex `--output-schema`용 임시 파일. 버려질 때 지운다.
struct SchemaFile(PathBuf);

impl SchemaFile {
    fn write(schema: &Value) -> Result<Self, AiError> {
        // codex(OpenAI strict)는 additionalProperties: false를 요구한다.
        let mut s = schema.clone();
        s["additionalProperties"] = Value::Bool(false);
        let p = std::env::temp_dir().join(format!(
            "ieum-schema-{}-{}.json",
            std::process::id(),
            SCHEMA_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&p, s.to_string())
            .map_err(|e| AiError(format!("스키마 임시 파일을 못 만들었어요: {e}")))?;
        Ok(SchemaFile(p))
    }
}

impl Drop for SchemaFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub struct CliProvider {
    kind: CliKind,
    program: String,
    cwd: PathBuf,
    timeout: Duration,
}

impl CliProvider {
    pub fn new(kind: CliKind) -> Self {
        CliProvider {
            kind,
            program: kind.program().into(),
            cwd: std::env::temp_dir().join("ieum-ai-cwd"),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// 실행할 프로그램(이름 또는 경로).
    pub fn program(mut self, program: &str) -> Self {
        self.program = program.into();
        self
    }

    /// CLI를 돌릴 빈 폴더(없으면 만든다). 사용자 파일이 있는 곳을 주지 않는다.
    pub fn cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = cwd;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl AiProvider for CliProvider {
    async fn ask(&self, req: AiRequest, chunk: &ChunkFn<'_>) -> Result<String, AiError> {
        let name = self.kind.program();
        if let Some(m) = req.model.as_deref().filter(|m| !valid_model(m)) {
            return Err(AiError(format!(
                "모델 이름이 올바르지 않아요(‘-’로 시작하거나 공백·제어문자가 있음): {m:?}"
            )));
        }
        std::fs::create_dir_all(&self.cwd)
            .map_err(|e| AiError(format!("작업 폴더를 못 만들었어요: {e}")))?;
        let schema_file = match (&req.schema, self.kind) {
            (Some(s), CliKind::Codex) => Some(SchemaFile::write(s)?),
            _ => None,
        };
        let mut line = sh_quote(&self.program);
        for a in self
            .kind
            .args(&req, schema_file.as_ref().map(|f| f.0.as_path()))
        {
            line.push(' ');
            line.push_str(&sh_quote(&a));
        }
        let mut child = Command::new(shell())
            .arg("-lc")
            .arg(&line)
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AiError(format!("{name} CLI를 실행하지 못했어요: {e}")))?;
        let pgid = child.id().map(|pid| pid as i32);
        let _group = pgid.map(KillGroup);
        let (mut stdin, stdout, mut stderr) = (
            child.stdin.take().expect("piped"),
            child.stdout.take().expect("piped"),
            child.stderr.take().expect("piped"),
        );
        let kind = self.kind;
        let prompt = req.prompt;

        let work = async {
            let write = async {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                drop(stdin); // EOF
            };
            let read = async {
                let mut st = StreamState::new(kind);
                let mut out = BufReader::new(stdout.take(MAX_STDOUT));
                let (mut buf, mut total) = (Vec::new(), 0u64);
                // UTF-8이 아닌 줄도 끝까지 비운다(읽기를 멈추면 자식이 파이프에 막힌다).
                while let Ok(n) = out.read_until(b'\n', &mut buf).await {
                    if n == 0 {
                        break;
                    }
                    total += n as u64;
                    st.feed(String::from_utf8_lossy(&buf).trim_end(), chunk);
                    buf.clear();
                }
                if total >= MAX_STDOUT {
                    if let Some(g) = pgid {
                        kill_group(g);
                    }
                    st.error = Some(format!(
                        "출력이 {}MB를 넘어 멈췄어요.",
                        MAX_STDOUT / 1024 / 1024
                    ));
                }
                st
            };
            let err = async {
                let mut b = Vec::new();
                let _ = (&mut stderr).take(MAX_STDERR).read_to_end(&mut b).await;
                // 넘는 부분은 버리고 계속 비운다(안 비우면 자식이 파이프에 막힌다).
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
                String::from_utf8_lossy(&b).into_owned()
            };
            let ((), st, err) = tokio::join!(write, read, err);
            let ok = child.wait().await.is_ok_and(|s| s.success());
            (st, ok, err)
        };
        let (st, ok, err) = tokio::time::timeout(self.timeout, work)
            .await
            .map_err(|_| {
                AiError(format!(
                    "{name} CLI가 {}초 안에 답하지 않아 멈췄어요(시간 초과).",
                    self.timeout.as_secs()
                ))
            })?;
        st.finish(ok, &err)
    }
}

#[derive(Debug, Clone)]
pub struct DetectedCli {
    /// `cli:claude`·`cli:codex`
    pub id: &'static str,
    pub path: PathBuf,
    pub version: String,
}

/// 셸 명령의 stdout. 시간 상한을 넘으면 그룹째 kill하고 None.
fn shell_output(cmd: &str, limit: Duration) -> Option<String> {
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(shell())
        .arg("-lc")
        .arg(cmd)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let group = KillGroup(child.id() as i32);
    let deadline = std::time::Instant::now() + limit;
    // ponytail: 50ms 폴링. 출력이 파이프 버퍼(64KB)를 넘는 명령엔 쓰지 않는다.
    loop {
        match child.try_wait().ok()? {
            Some(s) if s.success() => break,
            Some(_) => return None,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    // 읽기 전에 남은 손자를 죽인다 — stdout을 물고 있으면 읽기가 시간 상한 없이 멈춘다.
    drop(group);
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

/// 설치된 구독 CLI. 사용자 로그인 셸 PATH로 찾는다. CLI당 5초 상한. codex는 보류라 claude만.
pub fn detect_clis() -> Vec<DetectedCli> {
    [CliKind::Claude]
        .into_iter()
        .filter_map(|k| {
            let p = k.program();
            let text = shell_output(&format!("command -v {p} && {p} --version"), DETECT_TIMEOUT)?;
            let mut lines = text.lines();
            let path = PathBuf::from(lines.next()?.trim());
            let version = lines.next().unwrap_or("").trim().to_string();
            Some(DetectedCli {
                id: k.id(),
                path,
                version,
            })
        })
        .collect()
}
