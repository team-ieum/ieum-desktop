//! `shell_run` — 로그인 셸 명령의 stdout·stderr·종료 코드, 시간 초과·취소 시 그룹 kill.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ieum_local::ai::shell_run;

#[test]
fn shell_run_returns_code_and_streams() {
    let cancel = AtomicBool::new(false);
    let mut lines = Vec::new();
    let r = shell_run(
        "echo out; echo e1 1>&2; echo e2 1>&2; exit 3",
        Duration::from_secs(10),
        &cancel,
        &mut |l| lines.push(l.to_string()),
    )
    .unwrap();
    assert_eq!(r.code, Some(3));
    assert_eq!(r.stdout, "out\n");
    assert_eq!(lines, ["e1", "e2"]);
    assert!(r.stderr.contains("e2"));
    assert!(!r.timed_out && !r.cancelled);
}

#[test]
fn shell_run_timeout_kills_group() {
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let r = shell_run(
        "sleep 30 & echo $! ; wait",
        Duration::from_millis(300),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert!(r.timed_out);
    assert!(!r.cancelled);
    assert_eq!(r.code, None);
    assert!(start.elapsed() < Duration::from_secs(2));
    let pid = r.stdout.trim();
    assert!(!pid.is_empty());
    let alive = std::process::Command::new("kill")
        .args(["-0", pid])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "손자 sleep {pid}이 살아 있음");
}

#[test]
fn shell_run_cancel() {
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c.store(true, Ordering::SeqCst);
    });
    let start = Instant::now();
    let r = shell_run("sleep 30", Duration::from_secs(30), &cancel, &mut |_| {}).unwrap();
    assert!(r.cancelled);
    assert!(!r.timed_out);
    assert_eq!(r.code, None);
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn shell_run_stderr_closed_early_no_busy_loop() {
    // ponytail: 바쁜 루프 자체는 측정 못 한다 — 동작(정상 종료·시간)만 고정, 수정은 리뷰로 확인.
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let r = shell_run(
        "exec 2>&-; sleep 1",
        Duration::from_secs(10),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.code, Some(0));
    let took = start.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_secs(3),
        "{took:?}"
    );
}

#[test]
fn shell_run_tail_utf8_boundary() {
    let cancel = AtomicBool::new(false);
    // 30바이트 × 7000 = 210000바이트 — 마지막 64KB 시작이 글자 중간에 걸린다.
    let r = shell_run(
        "i=0; while [ $i -lt 7000 ]; do printf '가나다라마바사아자차'; i=$((i+1)); done",
        Duration::from_secs(20),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.code, Some(0));
    assert!(r.stdout.len() <= 65536, "{}", r.stdout.len());
    assert!(!r.stdout.contains('\u{FFFD}'));
    assert!(r.stdout.ends_with("자차"));
}

#[test]
fn shell_run_keeps_tail() {
    let cancel = AtomicBool::new(false);
    let r = shell_run(
        "i=0; while [ $i -lt 2000 ]; do printf '%0100d\\n' $i; i=$((i+1)); done; echo LAST",
        Duration::from_secs(20),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.code, Some(0));
    assert!(r.stdout.len() <= 65536, "{}", r.stdout.len());
    assert!(r.stdout.ends_with("LAST\n"));
}
