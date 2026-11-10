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
    AiError, AiProvider, AiRequest, AiRunner, ChunkFn, CliKind, CliProvider, Skip, file_text,
    parse_stream, read_file_text,
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

#[test]
fn codex_error_line_then_success_is_ok() {
    let lines = [
        r#"{"type":"error","message":"Reconnecting... 1/5"}"#,
        r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"ok"}}"#,
        r#"{"type":"turn.completed"}"#,
    ];
    assert_eq!(
        parse_stream(CliKind::Codex, lines, &|_: &str| {}).unwrap(),
        "ok"
    );
    // 답이 없으면 경고가 실패 이유가 된다.
    let err = parse_stream(CliKind::Codex, [lines[0]], &|_: &str| {}).unwrap_err();
    assert!(err.to_string().contains("Reconnecting"), "{err}");
    let failed = r#"{"type":"turn.failed","error":{"message":"사용 한도 초과"}}"#;
    let err = parse_stream(CliKind::Codex, [failed], &|_: &str| {}).unwrap_err();
    assert!(err.to_string().contains("사용 한도 초과"), "{err}");
}

#[test]
fn claude_prefers_structured_output() {
    let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"설명 글","structured_output":{"color":"파랑"}}"#;
    let r = parse_stream(CliKind::Claude, [line], &|_: &str| {}).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&r).unwrap(),
        json!({"color": "파랑"})
    );
}

/// 정해 둔 답을 차례로 돌려주는 가짜 공급원. `ERR:…`은 공급원 실패(CLI rc≠0 등).
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
        if let Some(e) = a.strip_prefix("ERR:") {
            return Err(AiError(e.to_string()));
        }
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
            json!([{"key": "error", "type": "text"}]),
            "출력 칸 이름 'error'는 예약어라 쓸 수 없습니다.",
        ),
        (
            json!([{"key": "myToken", "type": "text"}]),
            "출력 칸 이름 'myToken'는 민감정보 이름이라",
        ),
        (
            json!([{"key": "API_KEY_x", "type": "text"}]),
            "출력 칸 이름 'API_KEY_x'는 민감정보 이름이라",
        ),
        (
            json!([{"key": "Authorization", "type": "text"}]),
            "출력 칸 이름 'Authorization'는 민감정보 이름이라",
        ),
        (
            json!([{"key": "my_password", "type": "text"}]),
            "출력 칸 이름 'my_password'는 민감정보 이름이라",
        ),
        (
            json!([{"key": "a".repeat(65), "type": "text"}]),
            "영문·숫자·밑줄만 쓰고",
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
    // 64자 이름은 된다.
    let key = "a".repeat(64);
    let fake = Fake::new(vec![Box::leak(
        format!(r#"{{"{key}": "v"}}"#).into_boxed_str(),
    )]);
    let out = run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "outputFields": [{"key": key, "type": "text"}]}),
    )
    .await
    .unwrap();
    assert_eq!(out[&key], "v");
}

#[tokio::test]
async fn provider_failure_is_not_retried() {
    let fake = Fake::new(vec!["ERR:claude CLI가 답을 주지 못했어요", r#"{"a": "x"}"#]);
    let err = run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "outputFields": [{"key": "a", "type": "text"}]}),
    )
    .await
    .unwrap_err();
    assert!(
        err.message.contains("답을 주지 못했어요"),
        "{}",
        err.message
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn number_field_range() {
    let fields = json!([{"key": "n", "type": "number"}]);
    // int64 밖 정수·불리언은 number가 아니다(두 번 다 틀려 실패).
    for bad in [
        r#"{"n": 9223372036854775808}"#,
        r#"{"n": true}"#,
        r#"{"n": "3"}"#,
    ] {
        let fake = Fake::new(vec![bad]);
        let err = run(
            &fake_runner(&fake),
            json!({"provider": "cli:claude", "prompt": "p", "outputFields": fields}),
        )
        .await
        .unwrap_err();
        assert!(
            err.message.contains("출력 필드 'n'는 number여야 합니다"),
            "{bad}: {}",
            err.message
        );
    }
    for (good, want) in [
        (r#"{"n": -9223372036854775808}"#, json!(i64::MIN)),
        (r#"{"n": 4.0}"#, json!(4)),
        (r#"{"n": 1.5}"#, json!(1.5)),
        (r#"{"n": 1e20}"#, json!(1e20)),
    ] {
        let fake = Fake::new(vec![good]);
        let out = run(
            &fake_runner(&fake),
            json!({"provider": "cli:claude", "prompt": "p", "outputFields": fields}),
        )
        .await
        .unwrap();
        assert_eq!(out["n"], want, "{good}");
    }
}

fn req(model: Option<&str>) -> AiRequest {
    AiRequest {
        prompt: "p".into(),
        model: model.map(String::from),
        schema: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_model_name_fails() {
    // CliProvider를 직접 써도(러너를 거치지 않아도) 막힌다. 막히면 CLI를 띄우지 않는다.
    let (t, provider) = sleeper(Duration::from_secs(30));
    for m in [
        "--dangerously-skip-permissions",
        "-x",
        "a b",
        "a\nb",
        "a\u{7}",
    ] {
        let err = provider.ask(req(Some(m)), &|_: &str| {}).await.unwrap_err();
        assert!(
            err.to_string().contains("모델 이름이 올바르지 않아요"),
            "{m:?}: {err}"
        );
        assert!(!t.path().join("cwd/cli.pid").exists(), "CLI를 띄웠다");
    }
    // 러너 경로도 같은 검사를 지난다.
    let runner = AiRunner::empty().with_provider("cli:claude", Arc::new(provider));
    let err = run(
        &runner,
        json!({"provider": "cli:claude", "prompt": "p", "model": "-p"}),
    )
    .await
    .unwrap_err();
    assert!(err.message.contains("모델 이름이 올바르지 않아요"));
    // 정상 이름은 통과(가짜 공급원).
    let fake = Fake::new(vec!["ok"]);
    run(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "model": "claude-sonnet-5-5[1m]"}),
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stderr_flood_is_capped_and_drained() {
    let (_t, provider) = fake_cli("fake_cli_stderr.sh", Duration::from_secs(30));
    assert_eq!(provider.ask(req(None), &|_: &str| {}).await.unwrap(), "ok");
}

#[tokio::test]
async fn unsupported_provider_fails() {
    for p in ["cli:codex", "cli:gemini", "api:ollama", "x"] {
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
    // 읽기 상한(200KB)이 글자 중간을 자르는 파일: 잘린 마지막 글자만 버린다.
    let cut = t.path().join("cut.txt");
    fs::write(&cut, "가".repeat(70_000)).unwrap();
    let text = file_text(&cut);
    assert_eq!(text.chars().count(), 50_000);
    assert!(text.chars().all(|c| c == '가'));
}

/// 텍스트 연산이 아주 많은 비압축 PDF(추출이 오래 걸린다).
fn slow_pdf(path: &Path, ops: usize) {
    let mut content = String::new();
    for i in 0..ops {
        content.push_str(&format!("BT /F1 12 Tf {} 700 Td (w{i}) Tj ET\n", i % 500));
    }
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objs.len() + 1
    ));
    for off in offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objs.len() + 1
    ));
    fs::write(path, pdf).unwrap();
}

/// PDF 추출은 프로세스 전역 1개라, PDF를 추출하는 테스트끼리 줄을 세운다.
static PDF_TESTS: Mutex<()> = Mutex::new(());

fn pdf_turn() -> std::sync::MutexGuard<'static, ()> {
    PDF_TESTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// 앞 테스트가 남긴 추출 스레드가 끝날 때까지 기다려 읽는다.
fn read_when_free(p: &Path) -> Result<String, Skip> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match read_file_text(p) {
            Err(Skip::Busy) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50))
            }
            r => return r,
        }
    }
}

fn tiny_pdf(dir: &Path) -> PathBuf {
    let p = dir.join("tiny.pdf");
    slow_pdf(&p, 3);
    p
}

#[test]
fn small_pdf_text_extracted() {
    let _turn = pdf_turn();
    let t = tempfile::tempdir().unwrap();
    let text = read_when_free(&tiny_pdf(t.path())).unwrap();
    assert!(text.contains("w2"), "{text:?}");
}

fn oversized(dir: &Path) -> PathBuf {
    let big = dir.join("big.pdf");
    slow_pdf(&big, 3);
    // 내용은 그대로, 뒤에 0을 붙여 상한 + 1바이트로.
    let f = fs::OpenOptions::new().write(true).open(&big).unwrap();
    f.set_len(ieum_local::ai::MAX_PDF_BYTES + 1).unwrap();
    big
}

#[test]
fn oversized_pdf_skipped() {
    let t = tempfile::tempdir().unwrap();
    let p = oversized(t.path());
    let started = Instant::now();
    assert_eq!(read_file_text(&p), Err(Skip::TooBig));
    assert_eq!(file_text(&p), "");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn skipped_file_is_announced() {
    let t = tempfile::tempdir().unwrap();
    let p = oversized(t.path());
    let bin = t.path().join("a.bin");
    fs::write(&bin, [0u8, 1, 2]).unwrap();
    for (file, notice) in [
        (&p, "파일이 커서 내용 없이 이름으로만 판단해요."),
        (&bin, "파일 내용을 읽지 못해 이름으로만 판단해요."),
    ] {
        let fake = Fake::new(vec!["ok"]);
        let (r, events) = run_with(
            &fake_runner(&fake),
            json!({"provider": "cli:claude", "prompt": "p", "file": file.to_str().unwrap()}),
            &CancellationToken::new(),
        )
        .await;
        r.unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ExecEvent::NodeChunk { text, .. } if text == notice)),
            "{notice} 알림이 없다: {events:?}"
        );
    }
    // 읽은 파일은 알리지 않는다.
    let txt = t.path().join("a.txt");
    fs::write(&txt, "내용").unwrap();
    let fake = Fake::new(vec!["ok"]);
    let (_, events) = run_with(
        &fake_runner(&fake),
        json!({"provider": "cli:claude", "prompt": "p", "file": txt.to_str().unwrap()}),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(events.len(), 1, "답 조각만: {events:?}");
}

#[test]
fn concurrent_pdf_extraction_capped_to_one() {
    let _turn = pdf_turn();
    let t = tempfile::tempdir().unwrap();
    let tiny = tiny_pdf(t.path());
    read_when_free(&tiny).unwrap(); // 남은 추출이 없을 때 시작
    let slow = t.path().join("slow.pdf");
    slow_pdf(&slow, 100_000);
    let worker = std::thread::spawn(move || read_file_text(&slow));
    // slow가 자리를 잡을 시간(자리 잡기는 µs, 추출은 수백 ms 이상). 여기서 폴링하면 tiny가 자리를 뺏는다.
    std::thread::sleep(Duration::from_millis(100));
    // 앞의 추출이 도는 동안 다른 PDF는 추출하지 않고 바로 Busy.
    let started = Instant::now();
    assert_eq!(read_file_text(&tiny), Err(Skip::Busy));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(worker.join().unwrap().is_ok());
    assert!(read_file_text(&tiny).unwrap().contains("w2"));
}

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::await_holding_lock)] // 테스트 하나가 PDF 차례를 통째로 쥔다(다른 PDF 테스트는 동기)
async fn pdf_extraction_respects_cancel() {
    let _turn = pdf_turn();
    let t = tempfile::tempdir().unwrap();
    read_when_free(&tiny_pdf(t.path())).unwrap();
    let p = t.path().join("slow.pdf");
    slow_pdf(&p, 100_000);
    let fake = Fake::new(vec!["ok"]);
    let runner = fake_runner(&fake);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        c2.cancel();
    });
    let started = Instant::now();
    let (r, _) = run_with(
        &runner,
        json!({"provider": "cli:claude", "prompt": "p", "file": p.to_str().unwrap()}),
        &cancel,
    )
    .await;
    assert!(r.unwrap_err().message.contains("취소"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        0,
        "추출이 끝나기 전에 취소됐다"
    );
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
    fake_cli("fake_cli_sleep.sh", timeout)
}

fn fake_cli(name: &str, timeout: Duration) -> (tempfile::TempDir, CliProvider) {
    let t = tempfile::tempdir().unwrap();
    let script = t.path().join("fake-cli");
    fs::copy(fixture(name), &script).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let cwd = t.path().join("cwd");
    fs::create_dir(&cwd).unwrap();
    let provider = CliProvider::new(CliKind::Claude)
        .program(script.to_str().unwrap())
        .cwd(cwd)
        .timeout(timeout);
    (t, provider)
}

#[tokio::test(flavor = "multi_thread")]
async fn stdout_cap_kills_flooding_cli() {
    let (_t, provider) = fake_cli("fake_cli_flood.sh", Duration::from_secs(60));
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
    assert!(err.to_string().contains("10MB"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(30));
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
