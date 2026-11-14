//! `ieum workflow …` — 인자를 요청 한 줄로 바꿔 소켓에 보내고 응답 줄을 그대로 찍는다.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::JoinHandle;

use ieum_local::builder::{Op, Request};
use serde_json::json;
use tempfile::TempDir;

/// 연결 한 번을 받아 한 줄 읽고 `reply`를 준다. 받은 줄을 돌려준다.
fn serve(reply: &str) -> (TempDir, PathBuf, JoinHandle<String>) {
    let t = tempfile::Builder::new()
        .prefix("i")
        .tempdir_in("/tmp")
        .unwrap();
    let sock = t.path().join("s.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let reply = format!("{reply}\n");
    let got = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        stream.write_all(reply.as_bytes()).unwrap();
        line
    });
    (t, sock, got)
}

fn ieum(sock: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ieum"));
    cmd.args(args)
        .env_remove("IEUM_SOCKET")
        .env_remove("IEUM_TOKEN");
    if let Some(s) = sock {
        cmd.env("IEUM_SOCKET", s).env("IEUM_TOKEN", "tk");
    }
    cmd.output().unwrap()
}

#[test]
fn args_become_request_lines() {
    let s = |v: &str| v.to_string();
    let cases: Vec<(Vec<&str>, Op)> = vec![
        (vec!["status"], Op::Status {}),
        (
            vec![
                "outline",
                "--name",
                "정리",
                "--step",
                "새 파일",
                "--step",
                "옮기기",
            ],
            Op::Outline {
                name: Some(s("정리")),
                steps: vec![s("새 파일"), s("옮기기")],
            },
        ),
        (
            vec!["add", "local.ai", "--label", "분류", "--after", "t"],
            Op::Add {
                event: s("local.ai"),
                label: s("분류"),
                after: Some(s("t")),
                branch: None,
            },
        ),
        (
            vec!["set", "a", "prompt=이름이 a=b 인 파일", "sendContent=false"],
            Op::Set {
                id: s("a"),
                values: json!({"prompt": "이름이 a=b 인 파일", "sendContent": "false"})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        ),
        (
            vec!["set", "c", "--json", r#"{"branches":[{"name":"PDF"}]}"#],
            Op::Set {
                id: s("c"),
                values: json!({"branches": [{"name": "PDF"}]})
                    .as_object()
                    .unwrap()
                    .clone(),
            },
        ),
        (
            vec!["connect", "c", "m", "--branch", "else"],
            Op::Connect {
                from: s("c"),
                to: s("m"),
                branch: Some(s("else")),
            },
        ),
        (
            vec!["pick-folder", "t", "folder"],
            Op::PickFolder {
                id: s("t"),
                field: s("folder"),
            },
        ),
        (
            vec!["test", "m", "--file", "/h/in/a.pdf"],
            Op::Test {
                id: s("m"),
                file: Some(s("/h/in/a.pdf")),
            },
        ),
        (vec!["finish"], Op::Finish {}),
    ];
    for (args, op) in cases {
        let (_t, sock, got) = serve(r#"{"ok":true}"#);
        let out = ieum(Some(&sock), &[&["workflow"], &args[..]].concat());
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        let line = got.join().unwrap();
        assert!(!line.trim_end().contains('\n'));
        let req = Request::parse(line.trim_end()).unwrap();
        assert_eq!((req.token.as_str(), &req.op), ("tk", &op), "{args:?}");
    }
}

#[test]
fn prints_reply_line_and_exit_code_follows_reply() {
    for (reply, code) in [
        (r#"{"ok":true,"data":{"id":"n1","needsAi":false}}"#, 0),
        (r#"{"ok":false,"code":"NO_SAMPLE_FILE","message":"m"}"#, 1),
        (r#"{"ok":false,"code":"FIELD_LOCKED","message":"m"}"#, 2),
        (r#"{"ok":false,"code":"UNAUTHORIZED","message":"m"}"#, 3),
    ] {
        let (_t, sock, _got) = serve(reply);
        let out = ieum(Some(&sock), &["workflow", "status"]);
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{reply}\n"));
        assert_eq!(out.status.code(), Some(code), "{reply}");
    }
    let (_t, sock, _got) = serve("garbage");
    let out = ieum(Some(&sock), &["workflow", "status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains(r#""code":"INTERNAL""#));
}

#[test]
fn guide_prints_text_only() {
    let (_t, sock, _got) = serve(r#"{"ok":true,"data":{"text":"안내\n둘째 줄"}}"#);
    let out = ieum(Some(&sock), &["workflow", "guide"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "안내\n둘째 줄\n");
}

#[test]
fn outside_app_terminal_exits_3() {
    let stdout = |o: &Output| String::from_utf8_lossy(&o.stdout).to_string();
    let out = ieum(None, &["workflow", "status"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(
        stdout(&out).contains(r#""code":"UNAUTHORIZED""#)
            && stdout(&out).contains("IEUM 앱의 터미널")
    );
    // 끝난 세션·꺼진 앱
    let out = ieum(
        Some(Path::new("/tmp/ieum-no-such.sock")),
        &["workflow", "status"],
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(stdout(&out).contains(r#""code":"UNAUTHORIZED""#));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("샌드박스"));
}

/// 소켓 연결을 샌드박스가 막으면(EPERM) stderr로 이유를 알린다 — Antigravity 터미널 샌드박스가 그렇다.
#[test]
fn sandbox_blocked_socket_says_why() {
    let (_t, sock, _got) = serve(r#"{"ok":true,"data":{}}"#);
    let out = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", "(version 1)(allow default)(deny network-outbound)"])
        .arg(env!("CARGO_BIN_EXE_ieum"))
        .args(["workflow", "status"])
        .env("IEUM_SOCKET", &sock)
        .env("IEUM_TOKEN", "tk")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stdout).contains(r#""code":"UNAUTHORIZED""#));
    assert!(String::from_utf8_lossy(&out.stderr).contains("샌드박스 밖에서 실행"));
}

#[test]
fn only_workflow_edit_commands_exist() {
    // 소켓이 없어도 UNAUTHORIZED가 아니라 BAD_REQUEST = 연결하기 전에 거부했다.
    for args in [
        vec!["run", "wf.json"],
        vec!["conformance", "d"],
        vec!["workflow", "enable"],
        vec!["workflow", "confirm-finish"],
        vec!["workflow", "set", "a"],
        vec!["workflow", "set", "a", "novalue"],
        vec!["workflow", "set", "a", "--json", "[1]"],
        vec!["workflow", "set", "a", "k=v", "--json", "{}"],
        vec!["workflow", "add", "local.ai"],
    ] {
        let out = ieum(Some(Path::new("/tmp/ieum-no-such.sock")), &args);
        assert_eq!(out.status.code(), Some(3), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(r#""code":"BAD_REQUEST""#),
            "{args:?}"
        );
    }
}
