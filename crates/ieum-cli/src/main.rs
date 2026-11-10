//! 내부 CLI `ieum` — 화면 없이 워크플로우를 돌리고 적합성 묶음을 검사한다(스펙 §4.7, 공개하지 않음).
//! 종료 코드: 0 성공 / 1 실행 실패 / 2 검증 에러·거부 / 3 사용법·입출력.
//! `--json`: 표준 출력은 `ExecEvent` 한 줄씩 + 마지막 줄 `RunResult`, 에러는 표준 에러에 `{code, message}` 한 줄씩.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use async_trait::async_trait;
use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use ieum_engine::{
    BuiltinRunners, Catalog, ExecEvent, NodeCtx, NodeError, NodeRunner, NodeStatus, RejectReason,
    RunResult, RunStatus, Runners, Workflow, run_workflow,
};
use ieum_local::ai::AiRunner;
use ieum_local::{
    FileMoveRunner, FileRenameRunner, FileTrashRunner, Journal, RecentWrites, roots_for_workflow,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "ieum", about = "IEUM 내부 CLI(개발용)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 워크플로우 하나를 실행한다(실제 파일·AI 노드).
    Run {
        workflow: PathBuf,
        /// `{"trigger": {...}}` — trigger가 트리거 노드의 출력이 된다.
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// 묻지 않는다(1단계엔 확인 질문이 없어 받기만 한다).
        #[arg(long)]
        yes: bool,
    },
    /// 적합성 묶음 `<dir>/<case>/{workflow,input,expected}.json`을 검사한다.
    Conformance {
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

/// 사용법·입출력 에러(종료 코드 3).
struct Fail {
    code: &'static str,
    message: String,
}

fn io(message: String) -> Fail {
    Fail {
        code: "IO",
        message,
    }
}

fn report(json: bool, code: &str, message: &str) {
    if json {
        eprintln!("{}", json!({"code": code, "message": message}));
    } else {
        eprintln!("에러 [{code}] {message}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            let json = std::env::args().any(|a| a == "--json");
            report(json, "USAGE", e.to_string().trim());
            return ExitCode::from(3);
        }
    };
    let (json, result) = match cli.cmd {
        Cmd::Run {
            workflow,
            input,
            json,
            yes: _,
        } => (json, run(&workflow, input.as_deref(), json).await),
        Cmd::Conformance { dir, json } => (json, conformance(&dir, json).await),
    };
    result.unwrap_or_else(|f| {
        report(json, f.code, &f.message);
        ExitCode::from(3)
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, Fail> {
    let text = std::fs::read_to_string(path).map_err(|e| io(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| io(format!("{}: {e}", path.display())))
}

/// input 모양 검사 후 트리거 출력. 최상위·`trigger`·`nodeOutputs`가 객체가 아니면 사용법 에러(키는 없어도 된다).
fn trigger_of(input: &Value) -> Result<Map<String, Value>, Fail> {
    let usage = |message: &str| Fail {
        code: "USAGE",
        message: message.into(),
    };
    let input = input
        .as_object()
        .ok_or_else(|| usage("input은 객체여야 해요."))?;
    if input.get("nodeOutputs").is_some_and(|v| !v.is_object()) {
        return Err(usage("input의 nodeOutputs는 객체여야 해요."));
    }
    match input.get("trigger") {
        None => Ok(Map::new()),
        Some(Value::Object(m)) => Ok(m.clone()),
        Some(_) => Err(usage("input의 trigger는 객체여야 해요.")),
    }
}

fn catalog() -> Result<Catalog, Fail> {
    Catalog::embedded().map_err(|e| io(format!("노드 정의를 읽지 못했어요: {e}")))
}

// ── ieum run ─────────────────────────────────────────────

async fn run(workflow: &Path, input: Option<&Path>, json: bool) -> Result<ExitCode, Fail> {
    let wf: Workflow = read_json(workflow)?;
    let input: Value = input.map_or(Ok(json!({})), read_json)?;
    let trigger = trigger_of(&input)?;
    let catalog = catalog()?;

    let roots = roots_for_workflow(&wf);
    let (journal, recent) = (Journal::default(), RecentWrites::default());
    let mut runners = Runners::with_builtin();
    let local: [(&str, Arc<dyn NodeRunner>); 4] = [
        (
            "local.ai",
            Arc::new(AiRunner::new().with_roots(roots.clone())),
        ),
        (
            "local.file_move",
            Arc::new(FileMoveRunner::new(
                journal.clone(),
                recent.clone(),
                roots.clone(),
            )),
        ),
        (
            "local.file_rename",
            Arc::new(FileRenameRunner::new(
                journal.clone(),
                recent.clone(),
                roots.clone(),
            )),
        ),
        (
            "local.file_trash",
            Arc::new(FileTrashRunner::new(journal, recent, roots)),
        ),
    ];
    runners.extend(local.into_iter().map(|(k, v)| (k.to_string(), v)));

    let names: HashMap<&str, &str> = wf
        .nodes
        .iter()
        .map(|n| {
            let label = n.label.as_deref().filter(|l| !l.is_empty());
            (n.id.as_str(), label.unwrap_or(&n.id))
        })
        .collect();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let print_events = async {
        while let Some(e) = rx.recv().await {
            if json {
                println!("{}", serde_json::to_string(&e).unwrap());
            } else if let Some(line) = human_line(&e, &names) {
                println!("{line}");
            }
        }
    };
    let (result, ()) = tokio::join!(
        run_workflow(
            &wf,
            &catalog,
            &runners,
            trigger,
            tx,
            CancellationToken::new()
        ),
        print_events
    );

    if json {
        println!("{}", serde_json::to_string(&result).unwrap());
    }
    for (code, message) in reject_errors(&result) {
        report(json, &code, &message);
    }
    if !json {
        let status = match result.status {
            RunStatus::Succeeded => "성공",
            RunStatus::Failed => "실패",
            RunStatus::Cancelled => "취소",
            RunStatus::Rejected => "거부",
        };
        println!("실행 결과: {status}");
    }
    Ok(ExitCode::from(match result.status {
        RunStatus::Succeeded => 0,
        RunStatus::Failed | RunStatus::Cancelled => 1,
        RunStatus::Rejected => 2,
    }))
}

fn human_line(e: &ExecEvent, names: &HashMap<&str, &str>) -> Option<String> {
    let name = |id: &String| names.get(id.as_str()).copied().unwrap_or(id).to_string();
    Some(match e {
        ExecEvent::NodeSucceeded { node_id, .. } => format!("성공    {}", name(node_id)),
        ExecEvent::NodeFailed { node_id, message } => {
            format!("실패    {} — {message}", name(node_id))
        }
        ExecEvent::NodeSkipped { node_id } => format!("건너뜀  {}", name(node_id)),
        _ => return None,
    })
}

/// 거부 사유 → (code, message). 검증 에러는 하나씩.
fn reject_errors(r: &RunResult) -> Vec<(String, String)> {
    let one = |code: &str, msg: String| vec![(code.to_string(), msg)];
    match &r.reject_reason {
        None => Vec::new(),
        Some(RejectReason::TooLarge) => one(
            "REJECTED_TOO_LARGE",
            "노드·연결 수가 상한(노드 500·연결 2000)을 넘었어요.".into(),
        ),
        Some(RejectReason::UnknownEvent { event }) => one(
            "REJECTED_UNKNOWN_EVENT",
            format!("실행할 수 없는 노드 종류예요: {event}"),
        ),
        Some(RejectReason::Cycle) => one("REJECTED_CYCLE", "연결이 순환해요.".into()),
        Some(RejectReason::Invalid) => r
            .validation_errors
            .iter()
            .map(|e| {
                let code = serde_json::to_value(e.code).unwrap();
                let at = match (&e.node_id, &e.field) {
                    (Some(n), Some(f)) => format!(" (노드 {n}, {f})"),
                    (Some(n), None) => format!(" (노드 {n})"),
                    _ => String::new(),
                };
                (
                    code.as_str().unwrap_or_default().to_string(),
                    format!("{}{at}", e.message),
                )
            })
            .collect(),
    }
}

// ── ieum conformance ─────────────────────────────────────

/// 조건 분기 밖의 노드 — `input.nodeOutputs[id]`를 그대로 출력(없으면 빈 맵).
struct Stub(Value);

#[async_trait]
impl NodeRunner for Stub {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        Ok(self
            .0
            .get(&ctx.node.id)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default())
    }
}

async fn conformance(dir: &Path, json: bool) -> Result<ExitCode, Fail> {
    let catalog = catalog()?;
    let mut cases: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(format!("{}: {e}", dir.display())))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    if cases.is_empty() {
        return Err(io(format!("{}: 케이스가 없어요.", dir.display())));
    }
    cases.sort();
    let mut failed = 0;
    for case in &cases {
        let name = case.file_name().unwrap_or_default().to_string_lossy();
        let diffs = check_case(case, &catalog)
            .await
            .unwrap_or_else(|f| vec![f.message]);
        if json {
            println!(
                "{}",
                json!({"case": name, "ok": diffs.is_empty(), "diffs": diffs})
            );
        } else {
            println!("{} {name}", if diffs.is_empty() { "통과" } else { "실패" });
            for d in &diffs {
                println!("  - {d}");
            }
        }
        failed += usize::from(!diffs.is_empty());
    }
    if !json {
        println!("{}개 중 {}개 통과", cases.len(), cases.len() - failed);
    }
    Ok(ExitCode::from(u8::from(failed > 0)))
}

fn status_name(s: NodeStatus) -> &'static str {
    match s {
        NodeStatus::Succeeded => "SUCCEEDED",
        NodeStatus::Failed => "FAILED",
        NodeStatus::Skipped => "SKIPPED",
        NodeStatus::NotRun => "NOT_RUN",
    }
}

/// 케이스 하나의 차이 목록(비면 통과). expected에 없는 노드는 NOT_RUN이어야 한다.
async fn check_case(case: &Path, catalog: &Catalog) -> Result<Vec<String>, Fail> {
    let wf: Workflow = read_json(&case.join("workflow.json"))?;
    let input: Value = read_json(&case.join("input.json"))?;
    let expected: Value = read_json(&case.join("expected.json"))?;

    let stub: Arc<dyn NodeRunner> = Arc::new(Stub(input["nodeOutputs"].clone()));
    let mut runners = Runners::with_builtin();
    for event in [
        "local.ai",
        "local.file_move",
        "local.file_rename",
        "local.file_trash",
    ] {
        runners.insert(event.to_string(), stub.clone());
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = run_workflow(
        &wf,
        catalog,
        &runners,
        trigger_of(&input)?,
        tx,
        CancellationToken::new(),
    )
    .await;
    let mut started = Vec::new();
    while let Ok(e) = rx.try_recv() {
        if let ExecEvent::NodeStarted { node_id } = e {
            started.push(node_id);
        }
    }

    let mut diffs = Vec::new();
    let errors: Vec<Value> = result
        .validation_errors
        .iter()
        .map(|e| json!({"nodeId": e.node_id, "field": e.field, "code": e.code}))
        .collect();
    let want_errors = expected["validationErrors"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if errors != want_errors {
        diffs.push(format!(
            "검증 에러: 기대 {} / 실제 {}",
            Value::from(want_errors),
            Value::from(errors)
        ));
    }

    let want_nodes = expected["nodes"].as_object().cloned().unwrap_or_default();
    for id in want_nodes
        .keys()
        .filter(|id| !result.nodes.contains_key(*id))
    {
        diffs.push(format!("{id}: 워크플로우에 없는 노드"));
    }
    for (id, rec) in &result.nodes {
        let want = want_nodes.get(id);
        let want_status = want.and_then(|w| w["status"].as_str()).unwrap_or("NOT_RUN");
        let got = status_name(rec.status);
        if got != want_status {
            diffs.push(format!("{id}: 상태 기대 {want_status} / 실제 {got}"));
        }
        if let Some(want_out) = want.and_then(|w| w.get("output")) {
            let got_out = rec.output.clone().map_or(Value::Null, Value::Object);
            if &got_out != want_out {
                diffs.push(format!("{id}: 출력 기대 {want_out} / 실제 {got_out}"));
            }
        }
    }

    // 순서는 의존 관계만 — 둘 다 시작한 엣지는 source가 먼저
    let pos = |id: &str| started.iter().position(|s| s == id);
    for e in &wf.edges {
        if let (Some(s), Some(t)) = (pos(&e.source), pos(&e.target))
            && s > t
        {
            diffs.push(format!("순서: {}가 {}보다 늦게 시작", e.source, e.target));
        }
    }
    Ok(diffs)
}
