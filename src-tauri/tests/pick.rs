//! 폴더 고르기 창 — 한 번에 하나, 제한 시간이 지나면 취소로 답한다.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ieum_desktop_lib::pick::{Done, PICK_TIMEOUT, pick_serial};

#[tokio::test]
async fn picked_path_cancel_and_broken_dialog() {
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let ok = pick_serial(lock.clone(), Duration::from_secs(1), |done| {
        done(Some("/h/in".into()))
    })
    .await;
    assert_eq!(ok, Some(PathBuf::from("/h/in")));
    // 취소
    assert_eq!(
        pick_serial(lock.clone(), Duration::from_secs(1), |done| done(None)).await,
        None
    );
    // 창을 못 띄움
    assert_eq!(
        pick_serial(lock.clone(), Duration::from_secs(1), drop).await,
        None
    );
    // 잠금이 남지 않는다
    assert!(lock.try_lock().is_ok());
}

#[tokio::test]
async fn times_out_and_a_lingering_dialog_holds_the_next_one() {
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    let kept: Arc<Mutex<Option<Done>>> = Arc::default();
    let opened = Arc::new(AtomicUsize::new(0));
    let (k, o) = (kept.clone(), opened.clone());
    let started = Instant::now();
    let r = pick_serial(lock.clone(), Duration::from_millis(80), move |done| {
        o.fetch_add(1, SeqCst);
        *k.lock().unwrap() = Some(done);
    })
    .await;
    assert_eq!(r, None); // 5분 제한의 축소판
    assert!(started.elapsed() < Duration::from_secs(1));
    // 창이 아직 떠 있다 → 다음 고르기는 창을 띄우지 못하고 시간만 쓴다.
    let o2 = opened.clone();
    assert_eq!(
        pick_serial(lock.clone(), Duration::from_millis(80), move |done| {
            o2.fetch_add(1, SeqCst);
            done(Some("/x".into()));
        })
        .await,
        None
    );
    assert_eq!(opened.load(SeqCst), 1);
    // 사용자가 뒤늦게 고름 → 값은 버려지고 잠금이 풀린다.
    (kept.lock().unwrap().take().unwrap())(Some("/late".into()));
    assert_eq!(
        pick_serial(lock, Duration::from_millis(80), |done| done(Some(
            "/h/out".into()
        )))
        .await,
        Some(PathBuf::from("/h/out"))
    );
    assert_eq!(PICK_TIMEOUT, Duration::from_secs(300));
}
