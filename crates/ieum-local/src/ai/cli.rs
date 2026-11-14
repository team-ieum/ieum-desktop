//! 구독 CLI 공급원(claude·codex·agy). 감지·실행 모두 `$SHELL -lc`(사용자 PATH), 프롬프트는 stdin.
//! 빈 폴더에서 도구를 막고 돌린다. 취소·시간 초과 시 프로세스 그룹째 kill.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::{
    AiAnswer, AiError, AiProvider, AiRequest, AiUsage, ChunkFn, check_effort, system_text,
};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
/// 실패 메시지에 붙이는 stderr 끝부분(글자).
const STDERR_TAIL: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliKind {
    Claude,
    /// 도구는 모델 카탈로그를 덮어 없앤다(`lock_codex_catalog`). 남는 것은 `request_user_input` 하나(호출은 거부된다).
    /// 그래도 예상 밖 이벤트가 보이면 그 실행은 즉시 실패한다(`feed_codex`).
    Codex,
    /// 도구 목록은 못 끈다 → 호출 시점에 거부한다: `finish`만 통과시키는 훅(`AGY_GATE`) + deny 규칙(`AGY_SETTINGS`)
    /// + 도구 단계가 보이면 즉시 실패(`feed_agy`). 파일 내용은 보내지 않는다(`run_inner`).
    Agy,
}

/// agy `--settings` — 이 실행에만 적용된다.
pub const AGY_SETTINGS: &str = r#"{"permissions":{"deny":["command(*)","write_file(*)","read_file(*)","read_url(*)","execute_url(*)","mcp(*)"]},"toolPermission":"strict","allowNonWorkspaceAccess":false}"#;

/// agy PreToolUse 훅(`<작업 폴더>/.agents/ieum-gate.sh`). 도구 이름을 JSON으로 읽어 `finish`만 통과시킨다 —
/// 글자 찾기로 하면 인자 속 `"name":"finish"`에 속는다. 못 읽으면 거부.
pub const AGY_GATE: &str = r#"#!/bin/sh
n=$(/usr/bin/plutil -extract toolCall.name raw -o - - 2>/dev/null)
if [ "$n" = finish ]; then printf '%s\n' '{"decision":"allow"}'; else printf '%s\n' '{"decision":"deny","reason":"tools disabled"}'; fi
"#;

/// `path`의 내용·권한이 다를 때만, 같은 폴더의 임시 파일에 쓰고 rename으로 바꿔치기한다.
/// 그 자리에서 잘라 쓰면 같은 폴더를 쓰는 다른 실행이 빈·쓰다 만 파일을 읽을 수 있다.
fn replace_file(path: &Path, content: &str, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let same = std::fs::read(path).is_ok_and(|c| c == content.as_bytes())
        && std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o777 == mode);
    if same {
        return Ok(());
    }
    let tmp = path.with_file_name(format!(
        ".ieum-tmp-{}-{}",
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || {
        std::fs::write(&tmp, content)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
        std::fs::rename(&tmp, path)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// agy 작업 폴더의 훅을 매번 맞춰 둔다(누가 고쳐 놓았어도 원래대로). 못 쓰면 Err — 훅 없이 띄우지 않는다.
fn write_agy_gate(cwd: &Path) -> Result<(), AiError> {
    let dir = cwd.join(".agents");
    let gate = dir.join("ieum-gate.sh");
    let path = gate.to_string_lossy().into_owned();
    // 훅 명령은 인용 없이 실행된다 — 공백·셸 글자가 든 경로면 훅이 안 돌 수 있어 띄우지 않는다.
    let plain = |c: char| c.is_alphanumeric() || "/-_.".contains(c);
    if !gate.is_absolute() || !path.chars().all(plain) {
        return Err(AiError(format!(
            "Antigravity 작업 폴더 경로를 쓸 수 없어요: {path}"
        )));
    }
    let hooks = serde_json::json!({"ieum": {"PreToolUse": [{"matcher": "*",
        "hooks": [{"type": "command", "command": path, "timeout": 5}]}]}});
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        // 스크립트가 먼저 — hooks.json이 가리킬 때는 이미 있다.
        replace_file(&gate, AGY_GATE, 0o755)?;
        replace_file(&dir.join("hooks.json"), &hooks.to_string(), 0o644)
    };
    write().map_err(|e| AiError(format!("Antigravity 도구 차단 훅을 못 만들었어요: {e}")))
}

/// 러너에 등록하고 설치를 찾는 구독 CLI.
pub(super) const ENABLED: [CliKind; 3] = [CliKind::Claude, CliKind::Codex, CliKind::Agy];

/// codex exec에서 끄는 기본 켜진 기능(`codex features list` 0.167 기준).
const CODEX_DISABLED_FEATURES: [&str; 21] = [
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
    "collaboration_modes",
];

/// codex exec에 `-c`로 덮는 설정(조사에서 실호출로 검증한 조합).
const CODEX_OVERRIDES: [&str; 9] = [
    "web_search=\"disabled\"",
    "skills.include_instructions=false",
    "skills.bundled.enabled=false",
    "include_permissions_instructions=false",
    "include_environment_context=false",
    "include_apps_instructions=false",
    "include_collaboration_mode_instructions=false",
    "project_doc_max_bytes=0",
    "mcp_servers={}",
];

const CODEX_CATALOG_ERROR: &str =
    "Codex 모델 목록을 읽지 못했어요. Codex CLI 버전을 확인해 주세요.";
/// `codex debug models --bundled` 시간 상한.
const CODEX_CATALOG_TIMEOUT: Duration = Duration::from_secs(10);

/// `codex debug models --bundled` 출력의 모든 모델에서 도구를 만드는 네 칸을 덮는다. 모델이 하나도 없으면 Err.
pub fn lock_codex_catalog(raw: &str) -> Result<Value, AiError> {
    let err = || AiError(CODEX_CATALOG_ERROR.into());
    let mut v: Value = serde_json::from_str(raw).map_err(|_| err())?;
    let models = v
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .filter(|m| !m.is_empty())
        .ok_or_else(err)?;
    for m in models {
        let m = m.as_object_mut().ok_or_else(err)?;
        m.insert("tool_mode".into(), "direct".into());
        m.insert("multi_agent_version".into(), Value::Null);
        m.insert("apply_patch_tool_type".into(), Value::Null);
        m.insert("shell_type".into(), "disabled".into());
        // gpt-6 계열은 여기로 `request_user_input_async`·`clock.curr_time`을 싣는다.
        // 새 칸이 생기면 `live_codex_request_has_no_tools`(모든 모델)가 잡는다.
        m.insert("experimental_supported_tools".into(), Value::Array(vec![]));
    }
    Ok(v)
}

/// 실행 줄에 넘기는 임시 파일.
struct RunFiles<'a> {
    schema: Option<&'a Path>,
    /// codex 잠긴 모델 카탈로그.
    catalog: Option<&'a Path>,
}

impl CliKind {
    /// `local.ai` provider 값.
    pub fn id(self) -> &'static str {
        match self {
            CliKind::Claude => "cli:claude",
            CliKind::Codex => "cli:codex",
            CliKind::Agy => "cli:agy",
        }
    }

    fn program(self) -> &'static str {
        match self {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
            CliKind::Agy => "agy",
        }
    }

    /// 사용자에게 보이는 이름(오류 문구).
    fn label(self) -> &'static str {
        match self {
            CliKind::Claude => "Claude (구독)",
            CliKind::Codex => "Codex (구독)",
            CliKind::Agy => "Antigravity (구독)",
        }
    }

    /// 도구 차단 포함 인자. claude: 내장 도구 0 + MCP 0 + 사용자 설정 파일 안 읽음 + safe-mode(CLAUDE.md·스킬·
    /// 플러그인·훅 끔) + 세션 저장 안 함. codex: 읽기 전용 샌드박스 + 도구를 없앤 모델 카탈로그 + 기능·안내문 끄기.
    fn args(self, req: &AiRequest, files: &RunFiles) -> Vec<String> {
        let mut a: Vec<String> = match self {
            CliKind::Claude => {
                let mut a: Vec<String> = [
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
                    "--append-system-prompt",
                ]
                .map(String::from)
                .to_vec();
                a.push(system_text(req));
                a
            }
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
                ]
                .map(String::from)
                .to_vec();
                for c in CODEX_OVERRIDES {
                    a.push("-c".into());
                    a.push(c.into());
                }
                if let Some(c) = files.catalog {
                    a.push("-c".into());
                    // TOML 문자열 = JSON 문자열 인용.
                    let path = Value::String(c.to_string_lossy().into_owned());
                    a.push(format!("model_catalog_json={path}"));
                }
                for f in CODEX_DISABLED_FEATURES {
                    a.push("--disable".into());
                    a.push(f.into());
                }
                a
            }
            CliKind::Agy => [
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--sandbox",
                "--disable-slash-commands",
                "--settings",
                AGY_SETTINGS,
                "--print-timeout",
                "280s",
            ]
            .map(String::from)
            .to_vec(),
        };
        // agy는 수준이 모델 id에 붙는다(`ask`가 `agy_run_id`로 이미 합쳤다).
        if let Some(e) = &req.effort {
            match self {
                CliKind::Claude => a.extend(["--effort".into(), e.clone()]),
                // TOML 문자열 = JSON 문자열 인용.
                CliKind::Codex => a.extend([
                    "-c".into(),
                    format!("model_reasoning_effort={}", Value::String(e.clone())),
                ]),
                CliKind::Agy => {}
            }
        }
        if let Some(m) = &req.model {
            a.push(
                if self == CliKind::Codex {
                    "-m"
                } else {
                    "--model"
                }
                .into(),
            );
            a.push(m.clone());
        }
        match (self, &req.schema, files.schema) {
            (CliKind::Claude, Some(s), _) => {
                a.push("--json-schema".into());
                a.push(s.to_string());
            }
            (CliKind::Codex, _, Some(f)) => {
                a.push("--output-schema".into());
                a.push(f.to_string_lossy().into_owned());
            }
            (CliKind::Agy, _, Some(f)) => {
                a.push("--json-schema".into());
                a.push(f.to_string_lossy().into_owned());
            }
            _ => {}
        }
        if self == CliKind::Agy {
            a.extend(["-p".into(), String::new()]); // 프롬프트는 stdin NDJSON
        }
        if self == CliKind::Codex {
            a.push("-".into()); // 프롬프트를 stdin에서
        }
        a
    }
}

/// agy 모델 id 끝에 붙는 추론 수준(`agy models`의 `gemini-4-argon-high` 식).
pub const AGY_EFFORTS: [&str; 4] = ["low", "medium", "high", "xhigh"];

/// `gemini-4-argon-high` → (`gemini-4-argon`, Some(`high`)). 접미사가 없으면 (id, None).
pub fn agy_split(id: &str) -> (&str, Option<&'static str>) {
    for e in AGY_EFFORTS {
        if let Some(base) = id.strip_suffix(e).and_then(|b| b.strip_suffix('-'))
            && !base.is_empty()
        {
            return (base, Some(e));
        }
    }
    (id, None)
}

/// 실행할 agy 모델 id. 수준을 골랐으면 묶음 이름 + `-수준`. 안 골랐으면(기본):
/// 이미 접미사가 붙은 id(예전 저장값)는 그대로, 아니면 `listed`(`agy models`의 id들)에서 그 묶음의 `-medium`,
/// 없으면 묶음의 첫 것 — agy 기본 모델들이 보통 medium이라 그에 맞춘다. 묶음이 없으면(접미사 없는 모델) 그대로.
pub fn agy_run_id(model: &str, effort: Option<&str>, listed: &[String]) -> String {
    let (base, suffix) = agy_split(model);
    if let Some(e) = effort {
        return format!("{base}-{e}");
    }
    if suffix.is_some() {
        return model.to_string();
    }
    let group: Vec<&String> = listed
        .iter()
        .filter(|id| matches!(agy_split(id), (b, Some(_)) if b == base))
        .collect();
    group
        .iter()
        .find(|id| id.ends_with("-medium"))
        .or(group.first())
        .map_or_else(|| model.to_string(), |id| id.to_string())
}

/// `agy models` 출력(`<id>\t<이름>` 줄들) → id 목록. 앞의 안내 줄은 탭이 없어 빠진다.
pub fn agy_listed_ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| Some(l.split_once('\t')?.0.trim().to_string()))
        .filter(|id| !id.is_empty())
        .collect()
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
    /// 허용 목록 밖 이벤트를 봤다 — 더 읽지 않고 프로세스 그룹을 죽인다.
    fatal: bool,
    /// claude `result`·codex `turn.completed`의 `usage`. `finish`에서 종류별로 바꾼다.
    usage: Option<Value>,
    /// 요청한 모델(사용량 기록용). 비우면 CLI 기본 모델.
    model: Option<String>,
}

impl StreamState {
    fn new(kind: CliKind) -> Self {
        StreamState {
            kind,
            answer: None,
            error: None,
            warnings: Vec::new(),
            fatal: false,
            usage: None,
            model: None,
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
                if v["usage"].is_object() {
                    self.usage = Some(v["usage"].clone());
                }
                if v["is_error"] == true {
                    let msg = v["result"].as_str().or(v["subtype"].as_str());
                    self.error = Some(msg.unwrap_or("알 수 없는 오류").to_string());
                } else if !v["structured_output"].is_null() {
                    self.answer = Some(v["structured_output"].to_string());
                } else if let Some(r) = v["result"].as_str() {
                    self.answer = Some(r.to_string());
                }
            }
            (CliKind::Claude, _) => {}
            (CliKind::Codex, _) => self.feed_codex(ty, &v, chunk),
            (CliKind::Agy, _) => self.feed_agy(&v),
        }
    }

    /// codex는 허용 목록이다 — 답·생각·오류 밖의 이벤트(되살아난 도구 등)가 보이면 그 실행은 실패다.
    fn feed_codex(&mut self, ty: &str, v: &Value, chunk: &(dyn Fn(&str) + '_)) {
        let item = v["item"]["type"].as_str();
        match (ty, item) {
            ("thread.started" | "turn.started", _) => {}
            ("turn.completed", _) => {
                if v["usage"].is_object() {
                    self.usage = Some(v["usage"].clone());
                }
            }
            ("turn.failed", _) => {
                self.warnings
                    .extend(v["error"]["message"].as_str().map(String::from));
            }
            ("error", _) => {
                self.warnings
                    .extend(v["message"].as_str().map(String::from));
            }
            ("item.completed", Some("agent_message")) => {
                if let Some(t) = v["item"]["text"].as_str() {
                    chunk(t);
                    self.answer = Some(t.to_string());
                }
            }
            (
                "item.started" | "item.updated" | "item.completed",
                Some("agent_message" | "reasoning" | "error"),
            ) => {}
            _ => self.deny(item.unwrap_or(ty)),
        }
    }

    /// agy도 허용 목록이다. 도구 단계는 거부된 것(`ERROR`)이라도 보이면 실패 — 거부된 도구를 모델이 되풀이하면
    /// 토큰이 불어나고, deny 규칙을 지나는 도구(`search_web`)는 훅이 유일한 차단이다.
    fn feed_agy(&mut self, v: &Value) {
        let event = v["event"].as_str().unwrap_or("");
        match event {
            "init" => {}
            "step_update" => {
                let ty = v["step_update"]["step_type"].as_str();
                if !matches!(ty, Some("user_input" | "agent_response" | "finish")) {
                    self.deny(ty.unwrap_or(event));
                }
            }
            "result" => {
                let r = &v["result"];
                if r["usage"].is_object() {
                    self.usage = Some(r["usage"].clone());
                }
                let response = r["response"].as_str();
                if r["status"] != "SUCCESS" {
                    if !self.fatal {
                        let msg = response.filter(|s| !s.trim().is_empty());
                        let msg = msg.or(r["status"].as_str()).unwrap_or("알 수 없는 오류");
                        self.error = Some(msg.to_string());
                    }
                } else if !r["structured_output"].is_null() {
                    self.answer = Some(r["structured_output"].to_string());
                } else if let Some(t) = response {
                    self.answer = Some(t.to_string());
                }
            }
            _ => self.deny(event),
        }
    }

    /// 허용하지 않은 이벤트 — 답이 있어도 실패하고, 읽는 쪽이 곧바로 프로세스를 죽인다.
    fn deny(&mut self, what: &str) {
        self.error = Some(format!("허용하지 않은 동작({what})이 감지돼 멈췄어요."));
        self.fatal = true;
    }

    fn finish(self, exited_ok: bool, stderr: &str) -> Result<AiAnswer, AiError> {
        let name = self.kind.label();
        if let Some(e) = self.error {
            return Err(AiError(format!("{name}: {e}")));
        }
        let (kind, model) = (self.kind, self.model.as_deref());
        let usage = self.usage.map(|u| match kind {
            CliKind::Claude => AiUsage::from_anthropic(kind.id(), model, &u),
            CliKind::Codex => AiUsage::from_codex(model, &u),
            CliKind::Agy => AiUsage::from_agy(model, &u),
        });
        match self.answer {
            Some(text) if exited_ok => Ok(AiAnswer { text, usage }),
            _ if !self.warnings.is_empty() => {
                Err(AiError(format!("{name}: {}", self.warnings.join(" / "))))
            }
            _ => {
                let tail: String = {
                    let s = stderr.trim();
                    let skip = s.chars().count().saturating_sub(STDERR_TAIL);
                    s.chars().skip(skip).collect()
                };
                Err(AiError(format!("{name}이 답을 주지 못했어요: {tail}")))
            }
        }
    }
}

/// 저장된 출력 줄(fixture 등)을 해석한다 — 실행 때와 같은 해석기.
pub fn parse_stream<'a>(
    kind: CliKind,
    lines: impl IntoIterator<Item = &'a str>,
    chunk: &ChunkFn<'_>,
) -> Result<AiAnswer, AiError> {
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

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 실행 한 번 동안만 쓰는 임시 파일(스키마·codex 카탈로그). 버려질 때 지운다.
struct TempFile(PathBuf);

impl TempFile {
    /// `<임시 폴더>/<prefix>-<pid>-<seq>.json` — 아직 만들지 않는다.
    fn named(prefix: &str) -> Self {
        TempFile(std::env::temp_dir().join(format!(
            "{prefix}-{}-{}.json",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn schema(schema: &Value) -> Result<Self, AiError> {
        // codex(OpenAI strict)는 additionalProperties: false를 요구한다.
        let mut s = schema.clone();
        s["additionalProperties"] = Value::Bool(false);
        let f = Self::named("ieum-schema");
        std::fs::write(&f.0, s.to_string())
            .map_err(|e| AiError(format!("스키마 임시 파일을 못 만들었어요: {e}")))?;
        Ok(f)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 이 codex의 내장 모델 목록을 받아 도구를 없앤 패치본 파일을 만든다. 못 만들면 Err — 잠금 없이 띄우지 않는다.
// ponytail: 호출마다 만든다(로그인 셸 1회 추가). 느리면 codex --version을 키로 캐시.
async fn codex_catalog(program: &str) -> Result<TempFile, AiError> {
    let err = || AiError(CODEX_CATALOG_ERROR.into());
    // 출력이 약 745KB라 `shell_output`(64KB 파이프 한도)이 아니라 파일로 받는다.
    let raw = TempFile::named("ieum-codex-bundled");
    let line = format!(
        "{} debug models --bundled > {}",
        sh_quote(program),
        sh_quote(&raw.0.to_string_lossy())
    );
    let mut child = Command::new(shell())
        .arg("-lc")
        .arg(&line)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| err())?;
    let _group = child.id().map(|pid| KillGroup(pid as i32));
    let done = tokio::time::timeout(CODEX_CATALOG_TIMEOUT, child.wait()).await;
    if !matches!(done, Ok(Ok(s)) if s.success()) {
        return Err(err());
    }
    let text = std::fs::read_to_string(&raw.0).map_err(|_| err())?;
    let locked = lock_codex_catalog(&text)?;
    let out = TempFile::named("ieum-codex-catalog");
    std::fs::write(&out.0, locked.to_string()).map_err(|_| err())?;
    Ok(out)
}

pub struct CliProvider {
    kind: CliKind,
    program: String,
    cwd: PathBuf,
    timeout: Duration,
    extra: Vec<String>,
}

impl CliProvider {
    pub fn new(kind: CliKind) -> Self {
        CliProvider {
            kind,
            program: kind.program().into(),
            // agy 폴더에는 훅 파일이 있다 — claude·codex의 빈 폴더와 따로.
            cwd: std::env::temp_dir().join(if kind == CliKind::Agy {
                "ieum-ai-cwd-agy"
            } else {
                "ieum-ai-cwd"
            }),
            timeout: DEFAULT_TIMEOUT,
            extra: Vec::new(),
        }
    }

    /// 테스트용 — codex 인자 끝 `-` 앞에 덧붙인다(실호출 테스트가 요청을 가로챌 공급자 설정).
    #[doc(hidden)]
    pub fn extra_args(mut self, extra: Vec<String>) -> Self {
        self.extra = extra;
        self
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
    async fn ask(&self, mut req: AiRequest, chunk: &ChunkFn<'_>) -> Result<AiAnswer, AiError> {
        let name = self.kind.label();
        if let Some(m) = req.model.as_deref().filter(|m| !valid_model(m)) {
            return Err(AiError(format!(
                "모델 이름이 올바르지 않아요(‘-’로 시작하거나 공백·제어문자가 있음): {m:?}"
            )));
        }
        check_effort(req.effort.as_deref())?;
        if self.kind == CliKind::Agy
            && let Some(m) = req.model.take()
        {
            // 기본 수준인데 묶음 이름만 있으면 목록을 읽어 실제 id를 고른다. 못 읽으면 그대로(agy가 알린다).
            let listed = if req.effort.is_none() && agy_split(&m).1.is_none() {
                let line = format!("{} models", sh_quote(&self.program));
                tokio::task::spawn_blocking(move || shell_output(&line, DETECT_TIMEOUT * 4))
                    .await
                    .ok()
                    .flatten()
                    .map(|out| agy_listed_ids(&out))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            req.model = Some(agy_run_id(&m, req.effort.as_deref(), &listed));
        }
        std::fs::create_dir_all(&self.cwd)
            .map_err(|e| AiError(format!("작업 폴더를 못 만들었어요: {e}")))?;
        let catalog = match self.kind {
            CliKind::Codex => Some(codex_catalog(&self.program).await?),
            CliKind::Claude | CliKind::Agy => None,
        };
        let schema_file = match (&req.schema, self.kind) {
            (Some(s), CliKind::Codex | CliKind::Agy) => Some(TempFile::schema(s)?),
            _ => None,
        };
        // 고른 구독 계정의 설정 폴더는 명령 줄 앞에 — 로그인 셸이 읽는 프로필이 덮지 못하게.
        // 없으면 앱이 물려받은 값만 지운다(사용자 프로필의 값은 셸이 다시 넣는다).
        let mut cmd = Command::new(shell());
        let mut line = match self.kind {
            CliKind::Agy => {
                write_agy_gate(&self.cwd)?;
                // 계정 폴더 옵션이 없다(이 Mac의 로그인 하나).
                "AGY_CLI_DISABLE_AUTO_UPDATE=true ".to_string()
            }
            CliKind::Claude | CliKind::Codex => {
                let home_var = if self.kind == CliKind::Claude {
                    "CLAUDE_CONFIG_DIR"
                } else {
                    "CODEX_HOME"
                };
                match &req.config_dir {
                    Some(d) => format!("{home_var}={} ", sh_quote(&d.to_string_lossy())),
                    None => {
                        cmd.env_remove(home_var);
                        String::new()
                    }
                }
            }
        };
        line.push_str(&sh_quote(&self.program));
        let files = RunFiles {
            schema: schema_file.as_ref().map(|f| f.0.as_path()),
            catalog: catalog.as_ref().map(|f| f.0.as_path()),
        };
        let mut args = self.kind.args(&req, &files);
        if !self.extra.is_empty() {
            let at = args.len() - 1; // codex의 마지막 인자 `-` 앞
            args.splice(at..at, self.extra.iter().cloned());
        }
        for a in args {
            line.push(' ');
            line.push_str(&sh_quote(&a));
        }
        let mut child = cmd
            .arg("-lc")
            .arg(&line)
            .current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AiError(format!("{name}을 실행하지 못했어요: {e}")))?;
        let pgid = child.id().map(|pid| pid as i32);
        let _group = pgid.map(KillGroup);
        let (mut stdin, stdout, mut stderr) = (
            child.stdin.take().expect("piped"),
            child.stdout.take().expect("piped"),
            child.stderr.take().expect("piped"),
        );
        let kind = self.kind;
        // codex에는 system 자리 플래그가 없다 → 프롬프트 맨 앞에.
        let prompt = match kind {
            CliKind::Claude => req.prompt,
            CliKind::Codex => format!("{}\n\n{}", system_text(&req), req.prompt),
            CliKind::Agy => {
                let content = format!("{}\n\n{}", system_text(&req), req.prompt);
                let msg = serde_json::json!({"event": "user", "message": {"role": "user", "content": content}});
                format!("{msg}\n")
            }
        };
        let model = req.model;

        let work = async {
            let write = async {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                drop(stdin); // EOF
            };
            let read = async {
                let mut st = StreamState::new(kind);
                st.model = model;
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
                    if st.fatal {
                        if let Some(g) = pgid {
                            kill_group(g);
                            // 그 순간 fork 중이던 자식은 첫 kill을 빠져나가 stderr를 물고 남는다
                            // (그러면 끝까지 기다리게 된다) → 잠깐 뒤 한 번 더.
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            kill_group(g);
                        }
                        break;
                    }
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
                    "{name}이 {}초 안에 답하지 않아 멈췄어요(시간 초과).",
                    self.timeout.as_secs()
                ))
            })?;
        st.finish(ok, &err)
    }
}

#[derive(Debug, Clone)]
pub struct DetectedCli {
    /// `cli:claude`·`cli:codex`·`cli:agy`
    pub id: &'static str,
    pub path: PathBuf,
    pub version: String,
}

/// 셸 명령의 stdout(사용자 로그인 셸 PATH). 실패하거나 시간 상한을 넘으면(그룹째 kill) None.
pub fn shell_output(cmd: &str, limit: Duration) -> Option<String> {
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

/// 설치된 구독 CLI. 사용자 로그인 셸 PATH로 찾는다. CLI당 5초 상한.
pub fn detect_clis() -> Vec<DetectedCli> {
    ENABLED
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
