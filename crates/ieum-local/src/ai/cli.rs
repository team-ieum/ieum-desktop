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
    Codex,
}

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

    /// 도구 차단 포함 인자. claude: 내장 도구 0 + MCP 0 + 사용자 설정(훅) 안 읽음. codex: 읽기 전용 샌드박스.
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
            ]
            .map(String::from)
            .to_vec(),
            CliKind::Codex => ["exec", "--json", "--skip-git-repo-check", "-s", "read-only"]
                .map(String::from)
                .to_vec(),
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
    error: Option<String>,
}

impl StreamState {
    fn new(kind: CliKind) -> Self {
        StreamState {
            kind,
            answer: None,
            error: None,
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
                self.error = v["error"]["message"].as_str().map(String::from);
            }
            (CliKind::Codex, "error") => {
                self.error = v["message"].as_str().map(String::from);
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
            _ => {
                let tail: String = {
                    let cs: Vec<char> = stderr.trim().chars().collect();
                    cs[cs.len().saturating_sub(STDERR_TAIL)..].iter().collect()
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

/// 버려질 때 프로세스 그룹 전체를 kill한다(CLI가 띄운 자식까지).
// ponytail: 정상 종료 뒤에도 kill(-pgid)을 보낸다 — 남은 손자 정리용. pgid 재사용 경합은 무시할 만큼 짧다.
struct KillGroup(i32);

impl Drop for KillGroup {
    fn drop(&mut self) {
        // SAFETY: 단순 시스템 호출. 그룹이 이미 없으면 ESRCH로 끝난다.
        unsafe {
            kill(-self.0, SIGKILL);
        }
    }
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
        let _group = child.id().map(|pid| KillGroup(pid as i32));
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
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    st.feed(&l, chunk);
                }
                st
            };
            let err = async {
                let mut b = String::new();
                let _ = stderr.read_to_string(&mut b).await;
                b
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

/// 설치된 구독 CLI. 사용자 로그인 셸 PATH로 찾는다.
pub fn detect_clis() -> Vec<DetectedCli> {
    [CliKind::Claude, CliKind::Codex]
        .into_iter()
        .filter_map(|k| {
            let p = k.program();
            let out = std::process::Command::new(shell())
                .arg("-lc")
                .arg(format!("command -v {p} && {p} --version"))
                .current_dir(std::env::temp_dir())
                .stdin(Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())?;
            let text = String::from_utf8_lossy(&out.stdout);
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
