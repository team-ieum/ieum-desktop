//! 채팅 빌더의 `pick-folder`가 띄우는 폴더 고르기 창 — 앱을 앞으로 가져오고, 한 번에 하나, 5분 뒤 취소로 답한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::builder_host::PickFolder;

pub const PICK_TIMEOUT: Duration = Duration::from_secs(300);
pub type Done = Box<dyn FnOnce(Option<PathBuf>) + Send>;

/// 한 번에 창 하나. `open(done)`이 창을 띄우고 사용자가 닫으면 `done`을 부른다.
/// `limit`(잠금 기다림 포함) 안에 답이 없으면 None. 창은 사용자가 닫을 때까지 잠금을 쥔다.
pub async fn pick_serial(
    lock: Arc<tokio::sync::Mutex<()>>,
    limit: Duration,
    open: impl FnOnce(Done) + Send,
) -> Option<PathBuf> {
    // ponytail: 시간이 지나도 시스템 창은 닫지 못한다(플러그인에 닫기 API 없음). 뒤늦게 고른 값은
    // 버려지고, 그 창이 닫힐 때까지 다음 고르기는 줄을 선다. 닫기 API가 생기면 여기서 닫는다.
    tokio::time::timeout(limit, async {
        let guard = lock.lock_owned().await;
        let (tx, rx) = oneshot::channel();
        open(Box::new(move |p| {
            let _ = tx.send(p);
            drop(guard);
        }));
        rx.await.ok().flatten()
    })
    .await
    .ok()
    .flatten()
}

/// 실제 시스템 창. 창 제목은 Builder가 노드 이름·칸 제목으로 만든 것 — 무엇에 쓸 폴더인지 보인다.
pub fn system_pick(app: AppHandle) -> PickFolder {
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    PickFolder(Arc::new(move |_workflow_id, title| {
        let (app, lock) = (app.clone(), lock.clone());
        Box::pin(pick_serial(lock, PICK_TIMEOUT, move |done| {
            crate::tray::show_main(&app);
            let mut dialog = app.dialog().file().set_title(title);
            if let Some(w) = app.get_webview_window("main") {
                dialog = dialog.set_parent(&w);
            }
            dialog.pick_folder(move |p| done(p.and_then(|f| f.into_path().ok())));
        }))
    }))
}
