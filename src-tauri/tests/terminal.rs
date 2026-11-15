//! 터미널 — CLI 3종 실행 줄과 도구 enum.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_desktop_lib::credentials::AccountCredentials;
use ieum_desktop_lib::terminal::{
    FIRST_PROMPT, Gate, GateEnv, Launch, SCROLLBACK, Sink, Spawn, Terms, Tool, account_dir,
    launch_line, no_gate, prepare, start, usable,
};
use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::KeyError;

const Q: &str =
    "'IEUM 워크플로우를 만든다. 먼저 `ieum workflow guide`를 실행해 안내를 읽고 따르라.'";

#[test]
fn launch_lines_are_exact() {
    assert_eq!(format!("'{FIRST_PROMPT}'"), Q);
    let none = Launch::default();
    assert_eq!(
        launch_line(Tool::Claude, &none),
        format!("stty -echo; exec claude {Q} --allowedTools 'Bash(ieum workflow *)'")
    );
    assert_eq!(
        launch_line(Tool::Codex, &none),
        format!("stty -echo; exec codex --no-daemon -a on-request {Q}")
    );
    // 반향 끄기 — agy가 원시 모드 전에 보낸 단말 질의의 답이 `^[[?1;2c`로 찍히지 않게.
    assert_eq!(
        launch_line(Tool::Agy, &none),
        format!("stty -echo; exec agy -i {Q}")
    );
}

#[test]
fn launch_line_exports_bin_and_account_dir() {
    let l = Launch {
        bin_dir: Some("/App's/bin".into()),
        account_dir: Some("/a b/c1".into()),
    };
    assert_eq!(
        launch_line(Tool::Claude, &l),
        format!(
            "export PATH='/App'\\''s/bin':\"$PATH\"; export CLAUDE_CONFIG_DIR='/a b/c1'; stty -echo; exec claude {Q} --allowedTools 'Bash(ieum workflow *)'"
        )
    );
    assert!(launch_line(Tool::Codex, &l).contains(
        "; export CODEX_HOME='/a b/c1'; stty -echo; exec codex --no-daemon -a on-request "
    ));
    assert!(!launch_line(Tool::Agy, &l).contains("/a b/c1")); // agy는 계정 폴더 옵션이 없다
}

#[test]
fn launch_never_unlocks_permissions() {
    for t in [Tool::Claude, Tool::Codex, Tool::Agy] {
        let l = launch_line(
            t,
            &Launch {
                bin_dir: Some("/b".into()),
                account_dir: Some("/d".into()),
            },
        );
        for bad in [
            "dangerously",
            "yolo",
            "bypass",
            "approve-for-me",
            "permission-mode",
            "Bash(ieum *)",
            "Bash(*)",
        ] {
            assert!(!l.contains(bad), "{t:?}: {bad}: {l}");
        }
    }
}

#[test]
fn tool_accepts_only_three_names() {
    assert_eq!(serde_json::from_str::<Tool>("\"agy\"").unwrap(), Tool::Agy);
    for bad in [
        "\"shell\"",
        "\"Claude\"",
        "\"claude --x\"",
        "\"/bin/sh\"",
        "\"gemini\"",
    ] {
        assert!(serde_json::from_str::<Tool>(bad).is_err(), "{bad}");
    }
}

// ── PTY 세션 관리자 ──

const W: &str = "11111111-1111-4111-8111-111111111111";

fn sh(script: &str, cwd: &Path) -> Spawn {
    Spawn {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: cwd.into(),
        env: vec![("IEUM_TOKEN".into(), "tok".into())],
        cols: 80,
        rows: 24,
    }
}

fn collect() -> (Sink, Arc<Mutex<Vec<u8>>>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let b = buf.clone();
    (
        Arc::new(move |bytes: Vec<u8>| b.lock().unwrap().extend(bytes)),
        buf,
    )
}

/// 50ms 간격, 5초 상한.
fn until(f: impl Fn() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn wait_for(buf: &Arc<Mutex<Vec<u8>>>, needle: &str) -> bool {
    until(|| String::from_utf8_lossy(&buf.lock().unwrap()).contains(needle))
}

fn ends(n: &Arc<AtomicUsize>) -> Box<dyn FnOnce() + Send> {
    let n = n.clone();
    Box::new(move || {
        n.fetch_add(1, Ordering::SeqCst);
    })
}

/// `kill -0 <pid>`가 5초 안에 실패한다 = 프로세스가 없다.
fn gone(pid: &str) -> bool {
    until(|| {
        !std::process::Command::new("/bin/kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    })
}

#[test]
fn session_gets_env_and_input() {
    let (t, terms, (sink, buf)) = (tempfile::tempdir().unwrap(), Terms::default(), collect());
    terms
        .spawn(
            W,
            Tool::Claude,
            sh("printf \"$IEUM_TOKEN:$TERM:\"; cat", t.path()),
            sink,
            Box::new(|| {}),
        )
        .unwrap();
    assert!(wait_for(&buf, "tok:xterm-256color:"));
    terms.write(W, b"hi\n").unwrap();
    assert!(wait_for(&buf, "hi"));
    terms.kill_all();
}

#[test]
fn one_session_per_workflow_and_attach_replays() {
    let (t, terms, (sink, buf)) = (tempfile::tempdir().unwrap(), Terms::default(), collect());
    terms
        .spawn(
            W,
            Tool::Codex,
            sh("printf first; cat", t.path()),
            sink.clone(),
            Box::new(|| {}),
        )
        .unwrap();
    assert!(wait_for(&buf, "first"));
    assert!(
        terms
            .spawn(W, Tool::Claude, sh("cat", t.path()), sink, Box::new(|| {}))
            .is_err()
    );
    let (sink2, buf2) = collect();
    assert_eq!(terms.attach(W, sink2), Some(Tool::Codex)); // 처음 띄운 도구 그대로
    assert!(wait_for(&buf2, "first")); // 지난 출력 다시
    let before = buf.lock().unwrap().len();
    terms.write(W, b"x\n").unwrap();
    assert!(wait_for(&buf2, "x"));
    assert_eq!(buf.lock().unwrap().len(), before); // 옛 sink로는 더 안 간다
    assert_eq!(terms.attach("none", collect().0), None);
    terms.kill_all();
}

#[test]
fn scrollback_keeps_only_last_256k() {
    let (t, terms) = (tempfile::tempdir().unwrap(), Terms::default());
    let (sink, buf) = collect();
    terms
        .spawn(
            W,
            Tool::Agy,
            sh(
                "head -c 400000 /dev/zero | tr '\\0' x; printf END; cat",
                t.path(),
            ),
            sink,
            Box::new(|| {}),
        )
        .unwrap();
    assert!(wait_for(&buf, "END"));
    let (sink2, buf2) = collect();
    terms.attach(W, sink2);
    let b = buf2.lock().unwrap();
    assert!(b.len() <= SCROLLBACK && b.ends_with(b"END"), "{}", b.len());
    drop(b);
    terms.kill_all();
}

#[test]
fn kill_ends_child_and_runs_on_end_once() {
    let (t, terms, (sink, buf), n) = (
        tempfile::tempdir().unwrap(),
        Terms::default(),
        collect(),
        Arc::new(AtomicUsize::new(0)),
    );
    terms
        .spawn(
            W,
            Tool::Claude,
            sh("echo $$ > pid; printf up; cat", t.path()),
            sink,
            ends(&n),
        )
        .unwrap();
    assert!(wait_for(&buf, "up"));
    let pid = std::fs::read_to_string(t.path().join("pid"))
        .unwrap()
        .trim()
        .to_string();
    terms.kill(W);
    terms.kill(W); // 두 번 눌러도
    assert_eq!(terms.running(W), None);
    assert!(terms.write(W, b"x").is_err() && terms.resize(W, 10, 10).is_err());
    assert!(gone(&pid), "자식이 남았다");
    std::thread::sleep(Duration::from_millis(300)); // 읽기 스레드의 EOF 정리까지
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[test]
fn natural_exit_cleans_up() {
    let (t, terms, (sink, buf), n) = (
        tempfile::tempdir().unwrap(),
        Terms::default(),
        collect(),
        Arc::new(AtomicUsize::new(0)),
    );
    terms
        .spawn(W, Tool::Claude, sh("printf bye", t.path()), sink, ends(&n))
        .unwrap();
    assert!(wait_for(&buf, "bye"));
    assert!(until(
        || terms.running(W).is_none() && n.load(Ordering::SeqCst) == 1
    ));
    // 끝난 뒤엔 다시 띄울 수 있다
    terms
        .spawn(
            W,
            Tool::Claude,
            sh("cat", t.path()),
            collect().0,
            Box::new(|| {}),
        )
        .unwrap();
    terms.kill_all();
}

#[test]
fn resize_reaches_the_pty_and_kill_all_ends_everything() {
    let (t, terms, (sink, buf)) = (tempfile::tempdir().unwrap(), Terms::default(), collect());
    terms
        .spawn(
            W,
            Tool::Claude,
            sh("read _; stty size; cat", t.path()),
            sink,
            Box::new(|| {}),
        )
        .unwrap();
    terms
        .spawn(
            "w2",
            Tool::Codex,
            sh("cat", t.path()),
            collect().0,
            Box::new(|| {}),
        )
        .unwrap();
    terms.resize(W, 100, 30).unwrap();
    terms.write(W, b"\n").unwrap();
    assert!(wait_for(&buf, "30 100"));
    terms.kill_all();
    assert!(terms.running(W).is_none() && terms.running("w2").is_none());
}

/// `f`가 3초 안에 돌아오는가(다른 스레드에서 돌린다 — 멈추면 테스트가 같이 멈추지 않게).
fn returns_in_time(f: impl FnOnce() + Send + 'static) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(3)).is_ok()
}

#[test]
fn stuck_write_does_not_block_kill() {
    let (t, terms) = (tempfile::tempdir().unwrap(), Arc::new(Terms::default()));
    // stdin을 읽지 않는 자식 — 입력 큐가 차면 PTY 쓰기가 멈춘다.
    terms
        .spawn(
            W,
            Tool::Claude,
            sh("sleep 30", t.path()),
            collect().0,
            Box::new(|| {}),
        )
        .unwrap();
    for id in ["w2", "w3"] {
        terms
            .spawn(
                id,
                Tool::Codex,
                sh("cat", t.path()),
                collect().0,
                Box::new(|| {}),
            )
            .unwrap();
    }
    let (tx, written) = std::sync::mpsc::channel();
    let writer = terms.clone();
    std::thread::spawn(move || {
        let _ = writer.write(W, &b"xxxxxxx\n".repeat(128 * 1024)); // 1MB
        let _ = tx.send(());
    });
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        written.try_recv().is_err(),
        "쓰기가 멈추지 않았다 — 이 테스트가 아무것도 확인하지 못한다"
    );
    // 멈춘 쓰기가 있어도 다른 워크플로우의 동작과 종료는 바로 된다.
    let other = terms.clone();
    assert!(returns_in_time(move || other.kill("w2")), "kill이 멈췄다");
    assert_eq!(terms.running("w2"), None);
    let all = terms.clone();
    assert!(returns_in_time(move || all.kill_all()), "kill_all이 멈췄다");
    assert!(terms.running(W).is_none() && terms.running("w3").is_none());
    // 자식이 죽으면 멈췄던 쓰기도 풀린다(스레드가 새지 않는다).
    assert!(
        written.recv_timeout(Duration::from_secs(3)).is_ok(),
        "멈춘 쓰기가 풀리지 않았다"
    );
}

// ── 세션 환경과 토큰 ──

fn gate(log: &Arc<Mutex<Vec<String>>>) -> Gate {
    let (o, c) = (log.clone(), log.clone());
    Gate {
        open: Arc::new(move |w| {
            o.lock().unwrap().push(format!("open:{w}"));
            Box::pin(async move {
                Ok(Some(GateEnv {
                    socket: "/acct/ieum.sock".into(),
                    token: "T1".into(),
                    bin_dir: "/App/bin".into(),
                }))
            })
        }),
        close: Arc::new(move |t| c.lock().unwrap().push(format!("close:{t}"))),
    }
}

#[tokio::test]
async fn prepare_builds_workdir_env_and_line() {
    let (t, log) = (
        tempfile::tempdir().unwrap(),
        Arc::new(Mutex::new(Vec::new())),
    );
    let p = prepare(
        t.path(),
        W,
        Tool::Claude,
        Some("/accts/claude/a1b2c3d4".into()),
        &gate(&log),
        90,
        30,
    )
    .await
    .unwrap();
    assert_eq!(p.spawn.cwd, t.path()); // 계정의 고정 작업 폴더
    assert_eq!(p.spawn.args[0], "-lc");
    assert!(p.spawn.args[1].starts_with(
        "export PATH='/App/bin':\"$PATH\"; export CLAUDE_CONFIG_DIR='/accts/claude/a1b2c3d4'; stty -echo; exec claude "
    ));
    assert_eq!(
        p.spawn.env,
        vec![
            ("IEUM_SOCKET".to_string(), "/acct/ieum.sock".to_string()),
            ("IEUM_TOKEN".to_string(), "T1".to_string())
        ]
    );
    assert!(!p.spawn.args[1].contains("T1")); // 토큰은 실행 줄(ps)에 안 실린다
    assert_eq!(
        (p.spawn.cols, p.spawn.rows, p.token.as_deref()),
        (90, 30, Some("T1"))
    );
    assert_eq!(*log.lock().unwrap(), vec![format!("open:{W}")]);
}

#[tokio::test]
async fn prepare_without_gate_still_opens_terminal() {
    let t = tempfile::tempdir().unwrap();
    let p = prepare(t.path(), W, Tool::Agy, None, &no_gate(), 80, 24)
        .await
        .unwrap();
    assert!(p.spawn.env.is_empty() && p.token.is_none());
    assert!(p.spawn.args[1].starts_with("stty -echo; exec agy -i "));
}

#[tokio::test]
async fn prepare_rejects_ids_that_leave_the_account_folder() {
    let t = tempfile::tempdir().unwrap();
    for bad in ["../x", "a/b", "", ".."] {
        assert!(
            prepare(t.path(), bad, Tool::Claude, None, &no_gate(), 80, 24)
                .await
                .is_err(),
            "{bad}"
        );
    }
    assert!(std::fs::read_dir(t.path()).unwrap().next().is_none());
}

#[test]
fn only_installed_and_logged_in_tools_are_usable() {
    assert!(usable(true, "ok"));
    assert!(usable(true, "unavailable")); // 확인 불가(오프라인·키체인 거부)는 숨기지 않는다
    assert!(!usable(true, "not_logged_in"));
    assert!(!usable(false, "ok"));
    use ieum_desktop_lib::terminal::cli_state;
    assert_eq!(cli_state(true, "ok"), "ready");
    assert_eq!(cli_state(true, "unavailable"), "unknown");
    assert_eq!(cli_state(true, "not_logged_in"), "loggedOut");
    assert_eq!(cli_state(false, "not_logged_in"), "notInstalled");
}

#[tokio::test]
async fn token_is_closed_when_the_session_ends() {
    let (t, log, terms) = (
        tempfile::tempdir().unwrap(),
        Arc::new(Mutex::new(Vec::new())),
        Terms::default(),
    );
    let g = gate(&log);
    let mut p = prepare(t.path(), W, Tool::Claude, None, &g, 80, 24)
        .await
        .unwrap();
    p.spawn.program = "/bin/sh".into();
    p.spawn.args = vec!["-c".into(), "cat".into()];
    start(&terms, W, Tool::Claude, p, &g, collect().0, Box::new(|| {})).unwrap(); // 명령이 쓰는 것과 같은 함수
    terms.kill(W);
    assert_eq!(
        *log.lock().unwrap(),
        vec![format!("open:{W}"), "close:T1".to_string()]
    );
}

#[tokio::test]
async fn token_is_closed_when_the_terminal_cannot_start() {
    let (t, log, terms) = (
        tempfile::tempdir().unwrap(),
        Arc::new(Mutex::new(Vec::new())),
        Terms::default(),
    );
    let g = gate(&log);
    let mut p = prepare(t.path(), W, Tool::Claude, None, &g, 80, 24)
        .await
        .unwrap();
    p.spawn.program = "/no/such/program".into();
    assert!(start(&terms, W, Tool::Claude, p, &g, collect().0, Box::new(|| {})).is_err());
    assert_eq!(
        *log.lock().unwrap(),
        vec![format!("open:{W}"), "close:T1".to_string()]
    );
}

// ── 작업 폴더(앱 데이터 폴더 밖) ──

/// 작업 폴더는 넘긴 계정 폴더 그 자체(워크플로우마다 같다), 본인 전용(0700)이다.
#[tokio::test]
async fn work_folder_is_private_and_fixed_per_account() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("chat-work/acct");
    let p = prepare(&root, W, Tool::Agy, None, &no_gate(), 80, 24)
        .await
        .unwrap();
    let q = prepare(&root, W2, Tool::Claude, None, &no_gate(), 80, 24)
        .await
        .unwrap();
    assert_eq!((&p.spawn.cwd, &q.spawn.cwd), (&root, &root));
    let mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

const W2: &str = "22222222-2222-4222-8222-222222222222";

fn cat(mut p: ieum_desktop_lib::terminal::Prepared) -> ieum_desktop_lib::terminal::Prepared {
    p.spawn.program = "/bin/sh".into();
    p.spawn.args = vec!["-c".into(), "cat".into()];
    p
}

fn is_empty(d: &Path) -> bool {
    std::fs::read_dir(d).unwrap().next().is_none()
}

/// 시작·끝에 안의 내용만 비우고 폴더는 남긴다(한 번 신뢰하면 다시 묻지 않게).
/// 같은 계정의 다른 세션이 살아 있으면 비우지 않는다. 띄우지 못해도 비운다.
#[tokio::test]
async fn work_folder_cleared_only_when_no_session_of_the_account_lives() {
    let (t, terms, g) = (tempfile::tempdir().unwrap(), Terms::default(), no_gate());
    let work = t.path().join("acct");
    std::fs::create_dir_all(work.join("old/sub")).unwrap();
    std::fs::write(work.join("old/sub/f.txt"), "지난 세션").unwrap();
    let p = prepare(&work, W, Tool::Claude, None, &g, 80, 24)
        .await
        .unwrap();
    start(
        &terms,
        W,
        Tool::Claude,
        cat(p),
        &g,
        collect().0,
        Box::new(|| {}),
    )
    .unwrap();
    assert!(work.is_dir() && is_empty(&work)); // 시작 때 비움

    std::fs::write(work.join("a.txt"), "첫 세션 파일").unwrap();
    let p = prepare(&work, W2, Tool::Codex, None, &g, 80, 24)
        .await
        .unwrap();
    start(
        &terms,
        W2,
        Tool::Codex,
        cat(p),
        &g,
        collect().0,
        Box::new(|| {}),
    )
    .unwrap();
    assert!(work.join("a.txt").is_file()); // 첫 세션이 살아 있어 그대로
    terms.kill(W);
    assert!(work.join("a.txt").is_file()); // 둘째가 아직 살아 있다
    terms.kill(W2);
    assert!(work.is_dir() && is_empty(&work)); // 마지막 세션이 끝나면 비움, 폴더는 남김

    std::fs::write(work.join("b.txt"), "x").unwrap();
    let mut p = prepare(&work, W, Tool::Claude, None, &g, 80, 24)
        .await
        .unwrap();
    p.spawn.program = "/no/such/program".into();
    assert!(start(&terms, W, Tool::Claude, p, &g, collect().0, Box::new(|| {})).is_err());
    assert!(work.is_dir() && is_empty(&work));
}

/// 비울 때 링크는 링크만 지운다 — 폴더 밖 대상은 그대로. 작업 폴더 자체가 링크면 손대지 않는다.
#[tokio::test]
async fn clearing_never_follows_symlinks() {
    use std::os::unix::fs::symlink;
    let (t, terms) = (tempfile::tempdir().unwrap(), Terms::default());
    let outside = t.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "사용자 파일").unwrap();
    let work = t.path().join("acct");
    std::fs::create_dir_all(&work).unwrap();
    symlink(&outside, work.join("dir-link")).unwrap();
    symlink(outside.join("keep.txt"), work.join("file-link")).unwrap();
    terms.clear_work_if_idle(&work);
    assert!(is_empty(&work));
    assert!(outside.join("keep.txt").is_file());

    let linked = t.path().join("linked-acct");
    symlink(&outside, &linked).unwrap();
    terms.clear_work_if_idle(&linked);
    assert!(outside.join("keep.txt").is_file());
}

/// 앱을 띄운 CLI의 세션 표식은 터미널로 넘기지 않는다. 계정 폴더 변수는 그대로.
#[test]
fn session_markers_are_not_inherited() {
    use ieum_desktop_lib::terminal::session_marker;
    for k in [
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CODEX_SANDBOX",
        "CODEX_SANDBOX_NETWORK_DISABLED",
        "CODEX_THREAD_ID",
        "CODEX_TOOL_CALL_ID",
        "ANTIGRAVITY_AGENT",
        "ANTIGRAVITY_CONVERSATION_ID",
    ] {
        assert!(session_marker(k), "{k}");
    }
    for k in [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "PATH",
        "HOME",
        "CLAUDECODEX",
    ] {
        assert!(!session_marker(k), "{k}");
    }
    // 실제 PTY 자식에 안 보이는지는 `terminal_env.rs`(환경을 바꾸므로 따로 된 테스트 바이너리).
}

/// 예전 위치(`<계정 폴더>/chat-work`)는 비어 있는 것만 지운다.
#[test]
fn old_work_folders_removed_only_when_empty() {
    use ieum_desktop_lib::terminal::clean_old_work;
    let t = tempfile::tempdir().unwrap();
    clean_old_work(t.path()); // 없어도 된다
    let old = t.path().join("chat-work");
    let (empty, used) = (
        old.join(W),
        old.join("22222222-2222-4222-8222-222222222222"),
    );
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::create_dir_all(&used).unwrap();
    std::fs::write(used.join("keep.txt"), "사용자 파일").unwrap();
    clean_old_work(t.path());
    assert!(!empty.exists());
    assert!(used.join("keep.txt").is_file());
    std::fs::remove_file(used.join("keep.txt")).unwrap();
    clean_old_work(t.path());
    assert!(!old.exists());
}

/// `get`만 세는 키 저장소 — 키체인(허용 창)을 건드렸는지 본다.
#[derive(Default)]
struct CountingKeys(AtomicUsize);

impl KeyStore for CountingKeys {
    fn get(&self, _account: &str) -> Result<Option<String>, KeyError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
    fn set(&self, _account: &str, _key: &str) -> Result<(), KeyError> {
        Ok(())
    }
    fn delete(&self, _account: &str) -> Result<(), KeyError> {
        Ok(())
    }
}

#[test]
fn codex_account_dir_reads_files_only() {
    let t = tempfile::tempdir().unwrap();
    let keys = Arc::new(CountingKeys::default());
    let c = AccountCredentials::new(t.path().to_path_buf(), "aaaaaaaaaaaaaaaa", keys.clone());
    let b = c.begin_cli_account("codex", "회사").unwrap();
    let dir = c.pending_dir("codex", &b.id).unwrap();
    // [확인]을 거친 것처럼 대기 계정을 기본 계정으로 옮긴다.
    let file = t.path().join("credentials.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let mut m = v["pendingCodexAccounts"][0].take();
    m["isDefault"] = true.into();
    v["pendingCodexAccounts"] = serde_json::json!([]);
    v["codexAccounts"] = serde_json::json!([m]);
    std::fs::write(&file, v.to_string()).unwrap();
    keys.0.store(0, Ordering::SeqCst);
    assert_eq!(account_dir(&c, Tool::Codex), Some(dir));
    assert_eq!(keys.0.load(Ordering::SeqCst), 0); // 키체인은 안 읽는다
}
