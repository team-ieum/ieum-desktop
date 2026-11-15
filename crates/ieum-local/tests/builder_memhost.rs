//! 가짜 Host(`builder_support::MemHost`)가 Host 약속대로 도는지 — Builder 테스트들이 이 위에 선다.

mod builder_support;

use builder_support::MemHost;
use ieum_local::builder::{Builder, Host, SaveError};
use serde_json::json;

const W: &str = "11111111-1111-4111-8111-111111111111";

#[tokio::test]
async fn mem_host_saves_by_updated_at_and_picks_in_order() {
    let h = MemHost::with(
        W,
        json!({"nodes": [{"id": "t", "event": "local.folder_new_file", "config": {"folder": "/h/in"}}], "edges": []}),
    );
    assert!(Builder::new(h.clone()).is_ok());
    let doc = h.load(W).unwrap();
    assert!(doc.enabled && doc.updated_at == "v0");
    // 캔버스가 먼저 저장하면 옛 사본의 저장은 Stale.
    h.put_doc(W, |d| d.name = "캔버스".into());
    assert!(matches!(h.save(W, doc), Err(SaveError::Stale)));
    let saved = h.save(W, h.load(W).unwrap()).unwrap();
    assert_eq!(h.doc(W).updated_at, saved.updated_at);
    assert_eq!(h.saves.load(std::sync::atomic::Ordering::SeqCst), 1);
    // 거부 폴더는 check_folder·save 둘 다 막는다.
    h.bad_folders.lock().unwrap().push("/h/in".into());
    assert!(h.check_folder("/h/in".as_ref()).is_err());
    assert!(matches!(
        h.save(W, h.load(W).unwrap()),
        Err(SaveError::Rejected(_))
    ));
    // 고르기: 준비한 순서대로, 다 쓰면 취소. 기다리는 사이의 캔버스 편집이 먼저 일어난다.
    h.picks.lock().unwrap().push_back(Some("/h/out".into()));
    *h.on_pick.lock().unwrap() = Some(Box::new(|host| host.put_doc(W, |d| d.enabled = false)));
    assert_eq!(h.pick_folder(W, "제목").await, Some("/h/out".into()));
    assert!(!h.doc(W).enabled);
    assert_eq!(h.pick_folder(W, "둘째").await, None);
    assert_eq!(*h.pick_titles.lock().unwrap(), ["제목", "둘째"]);
    h.update_chat(W, &mut |c| c.state.outline.push("a".into()))
        .unwrap();
    assert_eq!(h.load_chat(W).unwrap().state.outline, ["a"]);
}
