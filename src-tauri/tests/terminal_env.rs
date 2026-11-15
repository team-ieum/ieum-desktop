//! 터미널 자식이 앱의 CLI 세션 표식을 물려받지 않는다.
//! 프로세스 환경을 바꾸므로 테스트 하나뿐인 바이너리로 둔다(다른 테스트와 동시에 돌지 않게).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_desktop_lib::terminal::{Spawn, Terms, Tool};

#[test]
fn pty_child_does_not_inherit_session_markers() {
    unsafe {
        std::env::set_var("CLAUDECODE", "1");
        std::env::set_var("CLAUDE_CODE_CHILD_SESSION", "1");
        std::env::set_var("CODEX_THREAD_ID", "t");
        std::env::set_var("ANTIGRAVITY_AGENT", "1");
        std::env::set_var("CLAUDE_CONFIG_DIR", "/keep");
    }
    let (t, terms) = (tempfile::tempdir().unwrap(), Terms::default());
    let buf = Arc::new(Mutex::new(Vec::new()));
    let b = buf.clone();
    let script = "printf \"[${CLAUDECODE:-_}${CLAUDE_CODE_CHILD_SESSION:-_}${CODEX_THREAD_ID:-_}${ANTIGRAVITY_AGENT:-_}|$CLAUDE_CONFIG_DIR|$IEUM_TOKEN]\"; cat";
    terms
        .spawn(
            "11111111-1111-4111-8111-111111111111",
            Tool::Claude,
            Spawn {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), script.into()],
                cwd: t.path().into(),
                env: vec![("IEUM_TOKEN".into(), "tok".into())],
                cols: 80,
                rows: 24,
            },
            Arc::new(move |bytes: Vec<u8>| b.lock().unwrap().extend(bytes)),
            Box::new(|| {}),
        )
        .unwrap();
    let want = "[____|/keep|tok]";
    let seen = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        String::from_utf8_lossy(&buf.lock().unwrap()).contains(want)
    });
    terms.kill_all();
    assert!(seen, "{}", String::from_utf8_lossy(&buf.lock().unwrap()));
}
