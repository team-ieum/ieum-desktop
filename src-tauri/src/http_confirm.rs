//! HTTP 노드가 낀 빌더 테스트의 사람 확인 대기 — 워크플로우마다 하나, 5분 뒤 취소로 답한다.
//! 계정(감독)마다 하나를 두어, 로그아웃·계정 전환 때 남은 대기가 다음 계정의 답을 받지 않는다.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use ieum_local::builder::HttpConfirm;
use tokio::sync::oneshot;

pub const HTTP_CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Waiting {
    /// 워크플로우 id → (차례 번호, 답 보낼 곳).
    by_id: HashMap<String, (u64, oneshot::Sender<HttpConfirm>)>,
    next: u64,
    /// 감독이 멈췄다 — 새 대기도 바로 취소.
    closed: bool,
}

#[derive(Default)]
pub struct HttpConfirmGate(Mutex<Waiting>);

impl HttpConfirmGate {
    fn waiting(&self) -> std::sync::MutexGuard<'_, Waiting> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 답 또는 300초 뒤 Cancelled. 같은 워크플로우의 앞선 대기는 Cancelled로 끝낸다.
    /// 대기는 부르는 즉시 등록된다 — 화면 이벤트를 그 뒤에 보내면 답을 놓치지 않는다.
    pub fn ask(&self, workflow_id: &str) -> impl Future<Output = HttpConfirm> + Send + '_ {
        self.ask_within(workflow_id, HTTP_CONFIRM_TIMEOUT)
    }

    pub fn ask_within(
        &self,
        workflow_id: &str,
        limit: Duration,
    ) -> impl Future<Output = HttpConfirm> + Send + '_ {
        let (tx, rx) = oneshot::channel();
        let seq = {
            let mut w = self.waiting();
            if w.closed {
                0 // tx가 버려져 바로 Cancelled
            } else {
                w.next += 1;
                let seq = w.next;
                // 앞선 대기의 보낼 곳이 버려지면 그쪽은 Cancelled로 깬다.
                w.by_id.insert(workflow_id.to_string(), (seq, tx));
                seq
            }
        };
        let id = workflow_id.to_string();
        async move {
            let got = tokio::time::timeout(limit, rx).await;
            // 시간 초과면 내 자리만 치운다 — 그사이 들어온 새 대기는 둔다.
            let mut w = self.waiting();
            if w.by_id.get(&id).is_some_and(|(n, _)| *n == seq) {
                w.by_id.remove(&id);
            }
            match got {
                Ok(Ok(choice)) => choice,
                _ => HttpConfirm::Cancelled,
            }
        }
    }

    /// 대기 중이 아니면 false.
    pub fn answer(&self, workflow_id: &str, choice: HttpConfirm) -> bool {
        match self.waiting().by_id.remove(workflow_id) {
            Some((_, tx)) => tx.send(choice).is_ok(),
            None => false,
        }
    }

    /// 감독을 멈출 때 — 남은 대기는 모두 Cancelled, 이후 ask도 바로 Cancelled.
    pub fn close(&self) {
        let mut w = self.waiting();
        w.closed = true;
        w.by_id.clear();
    }
}
