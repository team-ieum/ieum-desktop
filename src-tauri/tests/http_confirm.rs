//! HTTP 테스트 확인 대기 — 답·덮어쓰기·시간 초과·멈춤.

use std::time::Duration;

use ieum_desktop_lib::http_confirm::{HTTP_CONFIRM_TIMEOUT, HttpConfirmGate};
use ieum_local::builder::HttpConfirm;

#[tokio::test]
async fn gate_answer_resolves_and_second_ask_cancels_first() {
    let gate = HttpConfirmGate::default();
    assert!(!gate.answer("w", HttpConfirm::Send), "대기 없음");
    let first = gate.ask("w");
    let second = gate.ask("w");
    let other = gate.ask("x");
    assert_eq!(first.await, HttpConfirm::Cancelled);
    assert!(gate.answer("w", HttpConfirm::Send));
    assert_eq!(second.await, HttpConfirm::Send);
    assert!(!gate.answer("w", HttpConfirm::Skip), "이미 답함");
    assert!(gate.answer("x", HttpConfirm::Skip));
    assert_eq!(other.await, HttpConfirm::Skip);
}

#[tokio::test]
async fn gate_times_out_as_cancelled() {
    assert_eq!(HTTP_CONFIRM_TIMEOUT, Duration::from_secs(300));
    let gate = HttpConfirmGate::default();
    let late = gate.ask_within("w", Duration::from_millis(20));
    assert_eq!(late.await, HttpConfirm::Cancelled);
    assert!(
        !gate.answer("w", HttpConfirm::Send),
        "시간 초과 뒤 자리는 치워졌다"
    );
}

#[tokio::test]
async fn gate_close_cancels_waiting_and_later_asks() {
    let gate = HttpConfirmGate::default();
    let waiting = gate.ask("w");
    gate.close();
    assert_eq!(waiting.await, HttpConfirm::Cancelled);
    assert_eq!(gate.ask("w").await, HttpConfirm::Cancelled);
    assert!(!gate.answer("w", HttpConfirm::Send));
}
