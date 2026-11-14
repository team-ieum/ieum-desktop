//! `ieum workflow …` — IEUM 앱 터미널 안에서 AI CLI가 워크플로우를 고치는 유일한 길.
//! 인자를 요청 한 줄로 바꿔 앱의 소켓(`IEUM_SOCKET`·`IEUM_TOKEN`)에 보내고 응답 줄을 찍는다. 편집 로직은 없다.
//! 종료 코드: 0 성공 / 1 실패 / 2 검증·거부 / 3 사용법·세션 없음.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use ieum_local::builder::{Code, Op, Reply, Request};
use serde_json::{Map, Value};

/// 폴더 고르기(5분)와 AI 테스트를 기다릴 수 있어야 한다.
const REPLY_TIMEOUT: Duration = Duration::from_secs(600);
/// 샌드박스가 앱 소켓 연결을 막았을 때 stderr에 찍는다 — 에이전트가 "이 세션에서 못 씀"으로 오해하지 않게.
const SANDBOX_HINT: &str = "IEUM 앱 소켓 연결이 샌드박스에 막혔음(Operation not permitted). \
ieum 명령은 앱과 로컬 소켓으로 통신하므로 샌드박스 밖에서 실행할 것(사용자 승인 필요).";

#[derive(Parser)]
#[command(name = "ieum", about = "IEUM 워크플로우를 고치는 명령")]
struct Cli {
    #[command(subcommand)]
    cmd: Top,
}

#[derive(Subcommand)]
enum Top {
    /// 이 터미널에 묶인 워크플로우를 보고 고친다.
    Workflow {
        #[command(subcommand)]
        cmd: Cmd,
    },
}

#[derive(Subcommand)]
enum Cmd {
    /// 지금 상태와 남은 일.
    Status,
    /// 쓸 수 있는 노드 종류.
    Events,
    /// 완료 요청(켜는 것은 사용자가 앱에서 누른다).
    Finish,
    /// 쓰는 법 안내.
    Guide,
    /// 이름과 단계 계획.
    Outline {
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "step")]
        steps: Vec<String>,
    },
    /// 노드를 더한다.
    Add {
        event: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        after: Option<String>,
        #[arg(long)]
        branch: Option<String>,
    },
    /// 노드의 칸.
    Fields {
        id: String,
    },
    /// 노드를 지운다.
    Remove {
        id: String,
    },
    /// 칸을 채운다: `<칸>=<값>…` 또는 `--json '<객체>'`.
    Set {
        id: String,
        pairs: Vec<String>,
        #[arg(long)]
        json: Option<String>,
    },
    Connect {
        from: String,
        to: String,
        #[arg(long)]
        branch: Option<String>,
    },
    Disconnect {
        from: String,
        to: String,
    },
    Rename {
        id: String,
        label: String,
    },
    /// 폴더 칸은 사용자가 앱 창에서 고른다.
    PickFolder {
        id: String,
        field: String,
    },
    /// 노드까지 실제로 돌려 본다(옮긴 파일은 되돌린다).
    Test {
        id: String,
        #[arg(long)]
        file: Option<String>,
    },
}

/// `set`의 값: `칸=값`(첫 `=`에서만 자르고 값은 글자 그대로) 또는 `--json` 객체 — 둘 중 하나만.
fn set_values(pairs: Vec<String>, json: Option<String>) -> Result<Map<String, Value>, String> {
    match (pairs.is_empty(), json) {
        (true, Some(j)) => match serde_json::from_str(&j) {
            Ok(Value::Object(o)) => Ok(o),
            _ => Err("--json에는 JSON 객체를 넣어야 해요".into()),
        },
        (false, None) => pairs
            .iter()
            .map(|p| {
                p.split_once('=')
                    .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                    .ok_or_else(|| format!("`{p}`: <칸>=<값> 모양이어야 해요"))
            })
            .collect(),
        (true, None) => Err("<칸>=<값>이나 --json 중 하나가 있어야 해요".into()),
        (false, Some(_)) => Err("<칸>=<값>과 --json은 함께 쓸 수 없어요".into()),
    }
}

fn to_op(cmd: Cmd) -> Result<Op, String> {
    Ok(match cmd {
        Cmd::Status => Op::Status {},
        Cmd::Events => Op::Events {},
        Cmd::Finish => Op::Finish {},
        Cmd::Guide => Op::Guide {},
        Cmd::Outline { name, steps } => Op::Outline { name, steps },
        Cmd::Add {
            event,
            label,
            after,
            branch,
        } => Op::Add {
            event,
            label,
            after,
            branch,
        },
        Cmd::Fields { id } => Op::Fields { id },
        Cmd::Remove { id } => Op::Remove { id },
        Cmd::Set { id, pairs, json } => Op::Set {
            id,
            values: set_values(pairs, json)?,
        },
        Cmd::Connect { from, to, branch } => Op::Connect { from, to, branch },
        Cmd::Disconnect { from, to } => Op::Disconnect { from, to },
        Cmd::Rename { id, label } => Op::Rename { id, label },
        Cmd::PickFolder { id, field } => Op::PickFolder { id, field },
        Cmd::Test { id, file } => Op::Test { id, file },
    })
}

/// 응답 한 줄(줄바꿈 없음). 세션이 없으면 `Err(Unauthorized)`, 답이 없으면 `Err(Internal)`.
fn ask(op: Op) -> Result<String, Code> {
    let (Ok(socket), Ok(token)) = (std::env::var("IEUM_SOCKET"), std::env::var("IEUM_TOKEN"))
    else {
        return Err(Code::Unauthorized);
    };
    let mut stream = UnixStream::connect(socket).map_err(|e| {
        // 소켓은 있는데 연결이 거부됨 = 명령을 감싼 샌드박스가 막았다(예: Antigravity 터미널 샌드박스).
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            eprintln!("{SANDBOX_HINT}");
        }
        Code::Unauthorized
    })?;
    stream
        .set_read_timeout(Some(REPLY_TIMEOUT))
        .map_err(|_| Code::Internal)?;
    // 앱이 너무 긴 줄을 중간에 끊으면 쓰기가 실패한다 — 응답은 그래도 온다.
    let _ = stream.write_all(format!("{}\n", Request { token, op }.to_line()).as_bytes());
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|_| Code::Internal)?;
    Ok(line.trim_end().to_string())
}

fn fail(code: Code) -> ExitCode {
    let reply = Reply::err(code);
    println!("{}", reply.to_line());
    ExitCode::from(reply.exit_code() as u8)
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprint!("{e}");
            return fail(Code::BadRequest);
        }
    };
    let Top::Workflow { cmd } = cli.cmd;
    let op = match to_op(cmd) {
        Ok(op) => op,
        Err(e) => {
            eprintln!("{e}");
            return fail(Code::BadRequest);
        }
    };
    let guide = matches!(op, Op::Guide {});
    let line = match ask(op) {
        Ok(l) => l,
        Err(code) => return fail(code),
    };
    let Ok(reply) = serde_json::from_str::<Reply>(&line) else {
        return fail(Code::Internal);
    };
    match reply.data["text"].as_str() {
        Some(text) if guide && reply.ok => println!("{text}"),
        _ => println!("{line}"),
    }
    ExitCode::from(reply.exit_code() as u8)
}
