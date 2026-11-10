//! AI 노드와 구독 CLI 공급원. 실제 CLI는 `live_claude_answers`(#[ignore])에서만 부른다.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ieum_engine::{ExecEvent, Node, NodeCtx, NodeError, NodeRunner};
use ieum_local::ai::{
    AiError, AiProvider, AiRequest, AiRunner, ChunkFn, CliKind, CliProvider, file_text,
    parse_stream,
};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn parse(kind: CliKind, name: &str) -> (Vec<String>, Result<String, AiError>) {
    let text = fs::read_to_string(fixture(name)).unwrap();
    let chunks = Mutex::new(Vec::new());
    let r = parse_stream(kind, text.lines(), &|c: &str| {
        chunks.lock().unwrap().push(c.to_string())
    });
    (chunks.into_inner().unwrap(), r)
}

#[test]
fn parses_claude_stream() {
    let (chunks, r) = parse(CliKind::Claude, "claude.jsonl");
    assert_eq!(chunks, vec!["파란색"]);
    assert_eq!(r.unwrap(), "파란색");
}

#[test]
fn parses_codex_stream() {
    let (chunks, r) = parse(CliKind::Codex, "codex.jsonl");
    assert_eq!(chunks, vec!["파랑"]);
    assert_eq!(r.unwrap(), "파랑");
}

#[test]
fn claude_error_result_fails() {
    let line =
        r#"{"type":"result","subtype":"error","is_error":true,"result":"로그인이 필요해요"}"#;
    let r = parse_stream(CliKind::Claude, [line], &|_: &str| {});
    assert!(r.unwrap_err().to_string().contains("로그인이 필요해요"));
}

/// 정해 둔 답을 차례로 돌려주는 가짜 공급원.
struct Fake {
    answers: Vec<&'static str>,
    calls: AtomicUsize,
    last: Mutex<Option<AiRequest>>,
}

impl Fake {
    fn new(answers: Vec<&'static str>) -> Arc<Fake> {
        Arc::new(Fake {
            answers,
            calls: AtomicUsize::new(0),
            last: Mutex::new(None),
        })
    }
}

#[async_trait]
impl AiProvider for Fake {
    async fn ask(&self, req: AiRequest, chunk: &ChunkFn<'_>) -> Result<String, AiError> {
        let i = self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last.lock().unwrap() = Some(req);
        let a = self.answers[i.min(self.answers.len() - 1)];
        chunk(a);
        Ok(a.to_string())
    }
}

async fn run_with(
    runner: &AiRunner,
    config: Value,
    cancel: &CancellationToken,
) -> (Result<Map<String, Value>, NodeError>, Vec<ExecEvent>) {
    let node: Node = serde_json::from_value(json!({"id": "n", "event": "local.ai"})).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let r = runner
        .run(NodeCtx {
            node: &node,
            config: config.as_object().unwrap().clone(),
            events: &tx,
            cancel,
        })
        .await;
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (r, events)
}

async fn run(runner: &AiRunner, config: Value) -> Result<Map<String, Value>, NodeError> {
    run_with(runner, config, &CancellationToken::new()).await.0
}

fn fake_runner(fake: &Arc<Fake>) -> AiRunner {
    AiRunner::empty().with_provider("cli:claude", fake.clone())
}

#[tokio::test]
async fn plain_output_and_chunks() {
    let fake = Fake::new(vec!["파랑"]);
    let (r, events) = run_with(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "하늘 색은?"}),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        r.unwrap(),
        json!({"output": "파랑"}).as_object().unwrap().clone()
    );
    assert!(events.iter().any(
        |e| matches!(e, ExecEvent::NodeChunk { node_id, text } if node_id == "n" && text == "파랑")
    ));
    let req = fake.last.lock().unwrap().take().unwrap();
    assert_eq!(req.prompt, "하늘 색은?");
    assert!(req.schema.is_none());
}

#[tokio::test]
async fn prompt_includes_file() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("memo.txt");
    fs::write(&p, "회의록 내용").unwrap();
    let fake = Fake::new(vec!["ok"]);
    run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "요약해", "file": p.to_str().unwrap(), "model": "m1"}),
    )
    .await
    .unwrap();
    let req = fake.last.lock().unwrap().take().unwrap();
    assert_eq!(
        req.prompt,
        "요약해\n\n[파일 이름] memo.txt\n[파일 내용]\n회의록 내용"
    );
    assert_eq!(req.model.as_deref(), Some("m1"));
}

#[tokio::test]
async fn structured_output_extracts_declared_fields() {
    let fake = Fake::new(vec![
        r#"{"folder": "영수증", "score": 4.0, "ok": true, "extra": 1}"#,
    ]);
    let out = run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "분류해", "outputFields": [
            {"key": "folder", "type": "text", "description": "옮길 폴더"},
            {"key": "score", "type": "number"},
            {"key": "ok", "type": "boolean"}
        ]}),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(out),
        json!({"folder": "영수증", "score": 4, "ok": true})
    );
    let schema = fake.last.lock().unwrap().take().unwrap().schema.unwrap();
    assert_eq!(schema["required"], json!(["folder", "score", "ok"]));
    assert_eq!(schema["properties"]["folder"]["description"], "옮길 폴더");
    assert_eq!(schema["properties"]["score"]["type"], "number");
}

#[tokio::test]
async fn structured_output_retries_once_then_fails() {
    let fields = json!([{"key": "folder", "type": "text"}]);
    // 첫 답이 틀리고 두 번째가 맞으면 성공.
    let fake = Fake::new(vec!["그냥 글", r#"{"folder": "문서"}"#]);
    let out = run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "outputFields": fields}),
    )
    .await
    .unwrap();
    assert_eq!(out["folder"], "문서");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);

    // 두 번 다 틀리면 실패, 세 번째는 부르지 않는다.
    let fake = Fake::new(vec![r#"{"folder": 3}"#]);
    let err = run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "outputFields": fields}),
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("출력 필드 'folder'는 text여야 합니다"),
        "{}",
        err.message
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn invalid_output_fields_rejected() {
    let cases = [
        (
            json!("folder"),
            "출력 칸(outputFields)은 목록이어야 합니다.",
        ),
        (
            json!(
                (0..21)
                    .map(|i| json!({"key": format!("k{i}"), "type": "text"}))
                    .collect::<Vec<_>>()
            ),
            "출력 칸은 최대 20개입니다(받은 개수: 21).",
        ),
        (json!(["x"]), "1번째 출력 칸은 객체여야 합니다."),
        (
            json!([{"key": "a", "type": "text", "foo": 1}]),
            "1번째 출력 칸에 알 수 없는 속성이 있습니다",
        ),
        (
            json!([{"key": "1a", "type": "text"}]),
            "영문·숫자·밑줄만 쓰고",
        ),
        (
            json!([{"key": "a.b", "type": "text"}]),
            "영문·숫자·밑줄만 쓰고",
        ),
        (
            json!([{"key": "a", "type": "text"}, {"key": "a", "type": "text"}]),
            "출력 칸 이름 'a'가 중복됐습니다.",
        ),
        (
            json!([{"key": "success", "type": "text"}]),
            "출력 칸 이름 'success'는 예약어라 쓸 수 없습니다.",
        ),
        (
            json!([{"key": "myToken", "type": "text"}]),
            "출력 칸 이름 'myToken'는 민감정보 이름이라",
        ),
        (
            json!([{"key": "a", "type": "string"}]),
            "text·number·boolean 중 하나여야 합니다.",
        ),
        (
            json!([{"key": "a", "type": "text", "description": "가".repeat(501)}]),
            "출력 칸 'a'의 설명은 500자 이하 문자열이어야 합니다.",
        ),
    ];
    for (fields, msg) in cases {
        let fake = Fake::new(vec!["{}"]);
        let err = run(
            &fake_runner(&fake),
            json!({"provider": "cli:claude", "prompt": "p", "outputFields": fields}),
        )
        .await
        .unwrap_err();
        assert!(err.message.contains(msg), "{} ⊉ {}", err.message, msg);
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            0,
            "칸 정의가 틀리면 부르지 않는다"
        );
    }
    // 빈 목록은 기본 칸.
    let fake = Fake::new(vec!["답"]);
    let out = run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "outputFields": []}),
    )
    .await
    .unwrap();
    assert_eq!(out["output"], "답");
}

#[tokio::test]
async fn unsupported_provider_fails() {
    for p in ["cli:gemini", "api:anthropic", "x"] {
        let err = run(&AiRunner::new(), json!({"provider": p, "prompt": "p"}))
            .await
            .unwrap_err();
        assert!(
            err.message.contains("지원하지 않는 공급원"),
            "{}",
            err.message
        );
    }
}

#[test]
fn file_text_caps_at_50000() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("long.txt");
    fs::write(&p, "가".repeat(60_000)).unwrap();
    assert_eq!(file_text(&p).chars().count(), 50_000);
    let short = t.path().join("short.md");
    fs::write(&short, "안녕").unwrap();
    assert_eq!(file_text(&short), "안녕");
}

#[test]
fn binary_file_gives_empty_text() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("a.bin");
    fs::write(&p, [0x89u8, b'P', b'N', b'G', 0, 1, 2, 0xff, 0xfe]).unwrap();
    assert_eq!(file_text(&p), "");
    assert_eq!(file_text(&t.path().join("없음.txt")), "");
    let pdf = t.path().join("broken.pdf");
    fs::write(&pdf, b"%PDF-1.4 garbage").unwrap();
    assert_eq!(file_text(&pdf), "");
}

/// 가짜 CLI를 실행 가능하게 만들고, 그 CLI를 쓰는 공급원과 cwd를 돌려준다.
fn sleeper(timeout: Duration) -> (tempfile::TempDir, CliProvider) {
    let t = tempfile::tempdir().unwrap();
    let script = t.path().join("fake-cli");
    fs::copy(fixture("fake_cli_sleep.sh"), &script).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let cwd = t.path().join("cwd");
    fs::create_dir(&cwd).unwrap();
    let provider = CliProvider::new(CliKind::Claude)
        .program(script.to_str().unwrap())
        .cwd(cwd)
        .timeout(timeout);
    (t, provider)
}

fn read_pid(dir: &Path, name: &str) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(s) = fs::read_to_string(dir.join(name))
            && let Ok(pid) = s.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "{name} 기다리다 시간 초과");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

async fn assert_gone(pids: &[u32]) {
    for _ in 0..100 {
        if pids.iter().all(|p| !alive(*p)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("프로세스가 남았다: {pids:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_kills_cli_process() {
    let (t, provider) = sleeper(Duration::from_secs(300));
    let runner = AiRunner::empty().with_provider("cli:claude", Arc::new(provider));
    let cancel = CancellationToken::new();
    let cwd = t.path().join("cwd");
    let c2 = cancel.clone();
    let watcher = tokio::task::spawn_blocking(move || {
        let pids = [read_pid(&cwd, "cli.pid"), read_pid(&cwd, "child.pid")];
        c2.cancel();
        pids
    });
    let (r, _) = run_with(
        &runner,
        json!({"provider": "cli:claude", "prompt": "p"}),
        &cancel,
    )
    .await;
    assert!(r.unwrap_err().message.contains("취소"));
    assert_gone(&watcher.await.unwrap()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn timeout_kills_cli_process() {
    let (t, provider) = sleeper(Duration::from_secs(3));
    let started = Instant::now();
    let err = provider
        .ask(
            AiRequest {
                prompt: "p".into(),
                model: None,
                schema: None,
            },
            &|_: &str| {},
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("시간"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(30));
    let cwd = t.path().join("cwd");
    assert_gone(&[read_pid(&cwd, "cli.pid"), read_pid(&cwd, "child.pid")]).await;
}

/// 이 PC의 claude 구독으로 한 번 실제 호출한다: `cargo test -p ieum-local --test ai_cli -- --ignored`
#[tokio::test]
#[ignore]
async fn live_claude_answers() {
    let runner = AiRunner::new();
    let out = run(
        &runner,
        json!({"provider": "cli:claude", "prompt": "하늘 색을 한 단어로 답해.", "outputFields": [
            {"key": "color", "type": "text", "description": "색 이름 한 단어"}
        ]}),
    )
    .await
    .unwrap();
    let color = out["color"].as_str().unwrap();
    assert!(!color.is_empty());
    eprintln!("live claude: {color}");
}
