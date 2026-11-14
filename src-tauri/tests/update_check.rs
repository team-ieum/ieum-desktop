//! 새 버전 확인 간격 — 앱을 볼 때마다 확인하되 1시간 안에는 다시 하지 않는다.
use std::time::{Duration, Instant};

use ieum_desktop_lib::update_due;

#[test]
fn update_due_first_time_then_after_an_hour() {
    let t = Instant::now();
    assert!(update_due(None, t)); // 처음
    assert!(!update_due(Some(t), t + Duration::from_secs(59 * 60))); // 1시간 안
    assert!(update_due(Some(t), t + Duration::from_secs(60 * 60))); // 1시간 뒤
}
