//! 웹 워크플로우·실행 조회와 실행 생중계 구독. 가짜 HTTP 서버와 가짜 키 저장소만 쓴다.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ieum_desktop_lib::web::client::STREAM_CONNECT;
use ieum_desktop_lib::web::sse::{self, SseLimits};
use ieum_desktop_lib::web::{
    BeClient, SESSION_ACCOUNT, Session, WatchConfig, Watches, WebError, dashboard, executions,
    workflows,
};
use ieum_local::ai::KeyStore;
use ieum_local::ai::keys::KeyError;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const WF: &str = "11111111-1111-4111-8111-111111111111";
const EX: &str = "22222222-2222-4222-8222-222222222222";

#[derive(Default)]
struct FakeKeys(Mutex<HashMap<String, String>>);

impl KeyStore for FakeKeys {
    fn get(&self, account: &str) -> Result<Option<String>, KeyError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, key: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().insert(account.into(), key.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), KeyError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

/// 가짜 서버 응답 — JSON 한 번, 또는 SSE 조각을 (지연 ms, 내용)대로 흘리고 닫기.
enum Resp {
    Json(u16, Value),
    Sse(Vec<(u64, String)>),
}

type Log = Arc<Mutex<Vec<String>>>;

/// 요청 줄(`GET /api/...?q`)만 읽는다.
async fn read_line(sock: &mut TcpStream) -> String {
    let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
    loop {
        let n = sock.read(&mut tmp).await.unwrap();
        buf.extend_from_slice(&tmp[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if text.contains("\r\n\r\n") || n == 0 {
            let line = text.lines().next().unwrap_or("");
            return line.rsplit_once(' ').map_or(line, |(l, _)| l).to_string();
        }
    }
}

async fn serve(route: impl Fn(&str) -> Resp + Send + Sync + 'static) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = l.accept().await.unwrap();
            let (seen, route) = (seen.clone(), route.clone());
            tokio::spawn(async move {
                let line = read_line(&mut sock).await;
                let resp = route(&line);
                seen.lock().unwrap().push(line);
                match resp {
                    Resp::Json(status, body) => {
                        let body = body.to_string();
                        let r = format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = sock.write_all(r.as_bytes()).await;
                    }
                    Resp::Sse(parts) => {
                        let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
                        let _ = sock.write_all(head.as_bytes()).await;
                        for (ms, part) in parts {
                            tokio::time::sleep(Duration::from_millis(ms)).await;
                            if sock.write_all(part.as_bytes()).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    (base, log)
}

fn ok(data: Value) -> Resp {
    Resp::Json(
        200,
        json!({"success": true, "data": data, "message": "OK", "code": "OK"}),
    )
}

fn client(base: &str) -> (Arc<BeClient>, Arc<FakeKeys>) {
    let keys = Arc::new(FakeKeys::default());
    let s = Session {
        email: "a@b.c".into(),
        server_url: base.into(),
        access_token: "acc".into(),
        refresh_token: "ref".into(),
    };
    keys.set(SESSION_ACCOUNT, &serde_json::to_string(&s).unwrap())
        .unwrap();
    (Arc::new(BeClient::new(base, keys.clone()).unwrap()), keys)
}

fn wf_item(id: &str, name: &str) -> Value {
    json!({
        "id": id, "userId": "u", "name": name, "description": null, "active": true,
        "triggerType": "MANUAL", "version": 1, "services": [],
        "nodes": [], "edges": [], "webhookCredentialNames": {},
        "createdAt": "2026-11-10T01:00:00Z", "updatedAt": "2026-11-10T02:00:00Z"
    })
}

fn ev(t: &str) -> String {
    format!(
        "event:{t}\ndata:{}\n\n",
        json!({"type": t, "executionId": EX, "workflowId": WF, "nodeId": "n1"})
    )
}

fn lines(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

/// 받은 이벤트 payload 모음.
fn sink() -> (Arc<Mutex<Vec<Value>>>, executions::Emit) {
    let got: Arc<Mutex<Vec<Value>>> = Arc::default();
    let g = got.clone();
    (got, Arc::new(move |v| g.lock().unwrap().push(v)))
}

fn fast() -> WatchConfig {
    WatchConfig {
        limits: SseLimits {
            idle: Duration::from_millis(300),
            ..SseLimits::default()
        },
        reconnect_delay: Duration::from_millis(50),
    }
}

async fn wait_until(mut f: impl FnMut() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("시간 안에 조건이 맞지 않음");
}

#[tokio::test]
async fn lists_workflows_from_api_response() {
    let a = "aaaaaaaa-0000-4000-8000-000000000001";
    let b = "aaaaaaaa-0000-4000-8000-000000000002";
    let (base, log) = serve(move |line| match line {
        "GET /api/v1/workflows?size=20" => ok(json!({
            "content": [wf_item(a, "첫째")], "size": 1, "hasNext": true, "nextCursor": "c1"
        })),
        "GET /api/v1/workflows?size=20&cursor=c1" => ok(json!({
            "content": [wf_item(b, "둘째")], "size": 1, "hasNext": false
        })),
        _ => Resp::Json(404, json!({})),
    })
    .await;
    let (c, _) = client(&base);
    let list = workflows::list(&c).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, a);
    assert_eq!(list[0].name, "첫째");
    assert!(list[0].active);
    assert_eq!(list[0].updated_at.as_deref(), Some("2026-11-10T02:00:00Z"));
    assert_eq!(list[1].name, "둘째");
    assert!(lines(&log).iter().all(|l| l.starts_with("GET ")));
}

#[tokio::test]
async fn list_reads_services_and_node_count() {
    let mut item = wf_item("aaaaaaaa-0000-4000-8000-000000000001", "카드");
    item["services"] = json!(["GOOGLE_DRIVE", "DISCORD"]);
    item["nodes"] = json!([{"id": "n1"}, {"id": "n2"}, {"id": "n3"}]);
    let mut bare = wf_item("aaaaaaaa-0000-4000-8000-000000000002", "옛 응답");
    bare.as_object_mut().unwrap().remove("services");
    bare.as_object_mut().unwrap().remove("nodes");
    let (base, _) =
        serve(move |_| ok(json!({"content": [item.clone(), bare.clone()], "hasNext": false})))
            .await;
    let (c, _) = client(&base);
    let list = workflows::list(&c).await.unwrap();
    assert_eq!(
        list[0].services.as_deref(),
        Some(&["GOOGLE_DRIVE".to_string(), "DISCORD".to_string()][..])
    );
    assert_eq!(list[0].node_count, Some(3));
    // 화면으로는 노드 배열 대신 개수만 간다.
    let v = serde_json::to_value(&list[0]).unwrap();
    assert_eq!(v["nodeCount"], 3);
    assert!(v.get("nodes").is_none());
    assert_eq!(list[1].services, None);
    assert_eq!(list[1].node_count, None);
}

#[tokio::test]
async fn list_skips_bad_item() {
    let (base, _) = serve(|_| {
        ok(json!({
            "content": [
                wf_item("aaaaaaaa-0000-4000-8000-000000000001", "좋음"),
                {"id": 7, "name": ["깨짐"]},
                wf_item("aaaaaaaa-0000-4000-8000-000000000003", "또 좋음"),
            ],
            "size": 3, "hasNext": false
        }))
    })
    .await;
    let (c, _) = client(&base);
    let list = workflows::list(&c).await.unwrap();
    let names: Vec<_> = list.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["좋음", "또 좋음"]);
}

#[tokio::test]
async fn parses_workflow_keeping_unknown_keys() {
    let (base, _) = serve(|line| {
        if line != format!("GET /api/v1/workflows/{WF}") {
            return Resp::Json(404, json!({}));
        }
        let mut w = wf_item(WF, "분류");
        w["nodes"] = json!([{
            "id": "n1", "event": "github.create_issue", "type": "ACTION", "label": "이슈",
            "position": {"x": 1, "y": 2}, "config": {"repo": "a/b"}, "futureKey": {"k": 1}
        }]);
        w["edges"] = json!([{"id": "e1", "source": "n1", "target": "n2", "weird": true}]);
        w["brandNew"] = json!("유지");
        ok(w)
    })
    .await;
    let (c, _) = client(&base);
    let w = workflows::get(&c, WF).await.unwrap();
    assert_eq!(w.id, WF);
    assert_eq!(w.name, "분류");
    assert_eq!(w.graph.nodes[0].event, "github.create_issue");
    assert_eq!(w.graph.nodes[0].extra["futureKey"], json!({"k": 1}));
    assert_eq!(w.graph.edges[0].extra["weird"], json!(true));
    let out = serde_json::to_value(&w).unwrap();
    assert_eq!(out["brandNew"], "유지");
    assert_eq!(out["nodes"][0]["futureKey"], json!({"k": 1}));

    // 경로에 끼워 넣는 id는 UUID만.
    assert!(workflows::get(&c, "../auth/logout").await.is_err());
}

#[tokio::test]
async fn pages_executions_with_cursor() {
    let (base, log) = serve(|line| {
        let item = |id: &str| {
            json!({
                "id": id, "workflowId": WF, "status": "SUCCESS", "triggerType": "MANUAL",
                "startedAt": "2026-11-10T01:00:00Z", "finishedAt": "2026-11-10T01:00:05Z",
                "createdAt": "2026-11-10T01:00:00Z", "waitingApprovalNodeIds": []
            })
        };
        if line == format!("GET /api/v1/workflows/{WF}/executions?size=20") {
            ok(json!({"content": [item(EX)], "size": 1, "hasNext": true, "nextCursor": "a b&c"}))
        } else if line == format!("GET /api/v1/workflows/{WF}/executions?size=20&cursor=a+b%26c") {
            ok(json!({"content": [item(EX), {"id": 1}], "size": 2, "hasNext": false}))
        } else {
            Resp::Json(404, json!({}))
        }
    })
    .await;
    let (c, _) = client(&base);
    let p1 = executions::list(&c, WF, None).await.unwrap();
    assert_eq!(p1.content.len(), 1);
    assert_eq!(p1.content[0].status, "SUCCESS");
    assert!(p1.has_next);
    assert_eq!(p1.next_cursor.as_deref(), Some("a b&c"));
    let p2 = executions::list(&c, WF, p1.next_cursor).await.unwrap();
    // 깨진 항목은 건너뛴다.
    assert_eq!(p2.content.len(), 1);
    assert!(!p2.has_next);
    assert_eq!(lines(&log).len(), 2);
}

#[tokio::test]
async fn sse_events_forwarded_in_order() {
    let (base, log) = serve(|line| {
        if line.ends_with("/events") {
            Resp::Sse(vec![
                (0, ": keep-alive\n\n".into()),
                (0, ev("NODE_STARTED")),
                (20, ev("NODE_COMPLETED")),
                (20, ev("EXECUTION_COMPLETED")),
                // 끝 이벤트 뒤 것은 넘기지 않는다.
                (20, ev("NODE_STARTED")),
            ])
        } else {
            Resp::Json(404, json!({}))
        }
    })
    .await;
    let (c, _) = client(&base);
    let (got, emit) = sink();
    let w = Watches::new(fast());
    w.watch(c, WF, EX, emit).unwrap();
    wait_until(|| w.active().is_empty()).await;
    let got = got.lock().unwrap().clone();
    let types: Vec<_> = got.iter().map(|v| v["event"]["type"].clone()).collect();
    assert_eq!(
        types,
        [
            json!("NODE_STARTED"),
            json!("NODE_COMPLETED"),
            json!("EXECUTION_COMPLETED")
        ]
    );
    assert!(got.iter().all(|v| v["executionId"] == EX));
    // 끝 이벤트로 닫혔으니 재연결·logs 없음.
    assert_eq!(lines(&log).len(), 1);
}

#[tokio::test]
async fn quiet_node_keeps_reconnecting_until_completed() {
    let n = AtomicUsize::new(0);
    let (base, log) = serve(move |line| {
        if !line.ends_with("/events") {
            return ok(json!([]));
        }
        // 처음 세 번은 유휴 상한(300ms)을 넘게 조용하다.
        if n.fetch_add(1, SeqCst) < 3 {
            Resp::Sse(vec![(0, ev("NODE_STARTED")), (5_000, String::new())])
        } else {
            Resp::Sse(vec![
                (0, ev("NODE_COMPLETED")),
                (0, ev("EXECUTION_COMPLETED")),
            ])
        }
    })
    .await;
    let (c, _) = client(&base);
    let (got, emit) = sink();
    let w = Watches::new(fast());
    w.watch(c, WF, EX, emit).unwrap();
    wait_until(|| w.active().is_empty()).await;
    let l = lines(&log);
    assert_eq!(l.len(), 4, "{l:?}");
    assert!(l.iter().all(|l| l.ends_with("/events")), "{l:?}");
    let got = got.lock().unwrap().clone();
    assert_eq!(got.last().unwrap()["event"]["type"], "EXECUTION_COMPLETED");
}

#[tokio::test]
async fn two_connect_failures_fall_back_to_logs() {
    // 실패 → 성공(끝 없이 닫힘) → 실패 → 실패: 성공이 실패 횟수를 0으로 되돌리므로 네 번 붙는다.
    let n = AtomicUsize::new(0);
    let (base, log) = serve(move |line| {
        if line.ends_with("/events") {
            if n.fetch_add(1, SeqCst) == 1 {
                Resp::Sse(vec![(0, ev("NODE_STARTED"))])
            } else {
                Resp::Json(403, json!({"success": false, "code": "FORBIDDEN"}))
            }
        } else if line.ends_with("/logs") {
            ok(
                json!([{"id": "l1", "nodeId": "n1", "nodeType": "ACTION", "status": "SUCCESS",
                       "createdAt": "2026-11-10T01:00:00Z"}]),
            )
        } else {
            Resp::Json(404, json!({}))
        }
    })
    .await;
    let (c, _) = client(&base);
    let (got, emit) = sink();
    let w = Watches::new(fast());
    w.watch(c, WF, EX, emit).unwrap();
    wait_until(|| w.active().is_empty()).await;
    let l = lines(&log);
    let events = format!("GET /api/v1/workflows/{WF}/executions/{EX}/events");
    let logs = format!("GET /api/v1/workflows/{WF}/executions/{EX}/logs");
    assert_eq!(
        l,
        [events.clone(), events.clone(), events.clone(), events, logs]
    );
    let got = got.lock().unwrap().clone();
    let last = got.last().unwrap();
    assert_eq!(last["executionId"], EX);
    assert_eq!(last["logs"][0]["nodeId"], "n1");
}

#[tokio::test]
async fn sse_idle_timeout() {
    let (base, _) =
        serve(|_| Resp::Sse(vec![(0, ev("NODE_STARTED")), (3000, ev("NODE_COMPLETED"))])).await;
    let (c, _) = client(&base);
    let limits = SseLimits {
        idle: Duration::from_millis(200),
        ..SseLimits::default()
    };
    let started = std::time::Instant::now();
    let resp = c
        .stream(
            &format!("/api/v1/workflows/{WF}/executions/{EX}/events"),
            STREAM_CONNECT,
        )
        .await
        .unwrap();
    let mut seen = Vec::new();
    let r = sse::read_events(resp, &limits, |v| {
        seen.push(v);
        false
    })
    .await;
    assert!(matches!(r, Err(WebError::Network(_))), "{r:?}");
    assert_eq!(seen.len(), 1);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn sse_event_too_large_dropped() {
    let big = format!(
        "data:{}\n\n",
        json!({"type": "NODE_COMPLETED", "pad": "x".repeat(2000)})
    );
    let (base, _) = serve(move |_| {
        Resp::Sse(vec![
            (0, ev("NODE_STARTED")),
            (0, big.clone()),
            (0, "data:{not json\n\n".into()),
            (0, ev("EXECUTION_COMPLETED")),
        ])
    })
    .await;
    let (c, _) = client(&base);
    let limits = SseLimits {
        max_event: 1000,
        ..SseLimits::default()
    };
    let resp = c.stream("/x/events", STREAM_CONNECT).await.unwrap();
    let mut seen = Vec::new();
    let ended = sse::read_events(resp, &limits, |v| {
        let end = v["type"] == "EXECUTION_COMPLETED";
        seen.push(v);
        end
    })
    .await
    .unwrap();
    assert!(ended);
    let types: Vec<_> = seen.iter().map(|v| v["type"].clone()).collect();
    assert_eq!(types, [json!("NODE_STARTED"), json!("EXECUTION_COMPLETED")]);
}

#[tokio::test]
async fn sse_multi_line_data_over_limit_dropped() {
    // 한 줄씩은 상한(1000) 안이지만 합치면 넘는다 — 합친 값은 올바른 JSON이라 크기로만 걸러진다.
    let big = format!(
        "data:{{\"type\":\"NODE_COMPLETED\",\"a\":\"{}\"\ndata:,\"b\":\"{}\"}}\n\n",
        "x".repeat(600),
        "y".repeat(600)
    );
    let (base, _) = serve(move |_| {
        Resp::Sse(vec![
            (
                0,
                "data:{\"type\":\"NODE_STARTED\",\ndata:\"k\":1}\n\n".into(),
            ),
            (0, big.clone()),
            (0, ev("EXECUTION_COMPLETED")),
        ])
    })
    .await;
    let (c, _) = client(&base);
    let limits = SseLimits {
        max_event: 1000,
        ..SseLimits::default()
    };
    let resp = c.stream("/x/events", STREAM_CONNECT).await.unwrap();
    let mut seen = Vec::new();
    sse::read_events(resp, &limits, |v| {
        let end = v["type"] == "EXECUTION_COMPLETED";
        seen.push(v);
        end
    })
    .await
    .unwrap();
    let types: Vec<_> = seen.iter().map(|v| v["type"].clone()).collect();
    assert_eq!(types, [json!("NODE_STARTED"), json!("EXECUTION_COMPLETED")]);
}

#[tokio::test]
async fn stream_times_out_when_server_never_answers() {
    // 연결만 받고 아무것도 보내지 않는 서버.
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            held.push(l.accept().await.unwrap().0);
        }
    });
    let (c, _) = client(&base);
    let started = std::time::Instant::now();
    let r = tokio::time::timeout(
        Duration::from_secs(5),
        c.stream("/x/events", Duration::from_millis(300)),
    )
    .await
    .expect("헤더 대기 상한이 없음");
    assert!(matches!(r, Err(WebError::Network(_))), "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// 이벤트를 계속 흘리는 SSE — 구독 작업이 멈췄는지는 받은 이벤트 수가 더 늘지 않는지로 본다.
fn endless() -> Resp {
    Resp::Sse((0..600).map(|_| (50, ev("NODE_STARTED"))).collect())
}

fn event_count(got: &Arc<Mutex<Vec<Value>>>) -> usize {
    got.lock()
        .unwrap()
        .iter()
        .filter(|v| v.get("event").is_some())
        .count()
}

async fn assert_stopped(got: &[&Arc<Mutex<Vec<Value>>>]) {
    // abort는 다음 await에서 걸리므로 이미 받은 조각만큼은 더 올 수 있다.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before: Vec<_> = got.iter().map(|g| event_count(g)).collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after: Vec<_> = got.iter().map(|g| event_count(g)).collect();
    assert_eq!(before, after);
}

#[tokio::test]
async fn dropped_watches_stop() {
    let (base, _) = serve(|_| endless()).await;
    let (c, _) = client(&base);
    let w = Watches::new(fast());
    let id = |n: u32| format!("22222222-2222-4222-8222-{n:012}");
    let (a, ea) = sink();
    let (b, eb) = sink();
    w.watch(c.clone(), WF, &id(1), ea).unwrap();
    w.watch(c.clone(), WF, &id(2), eb).unwrap();
    wait_until(|| event_count(&a) > 0 && event_count(&b) > 0).await;
    w.unwatch(&id(2));
    // 3~7번을 더하면 가장 오래된 1번이 밀려난다.
    for n in 3..=7 {
        w.watch(c.clone(), WF, &id(n), sink().1).unwrap();
    }
    assert!(!w.active().contains(&id(1)));
    assert_stopped(&[&a, &b]).await;
    // 밀려난 구독은 화면에 알리고, 직접 해제한 구독은 알리지 않는다.
    assert!(
        a.lock()
            .unwrap()
            .contains(&json!({"executionId": id(1), "stopped": true}))
    );
    assert!(b.lock().unwrap().iter().all(|v| v.get("stopped").is_none()));
}

#[tokio::test]
async fn clear_stops_all_watches() {
    let (base, _) = serve(|_| endless()).await;
    let (c, _) = client(&base);
    let w = Watches::new(fast());
    let id = |n: u32| format!("22222222-2222-4222-8222-{n:012}");
    let (a, ea) = sink();
    let (b, eb) = sink();
    w.watch(c.clone(), WF, &id(1), ea).unwrap();
    w.watch(c, WF, &id(2), eb).unwrap();
    wait_until(|| event_count(&a) > 0 && event_count(&b) > 0).await;
    w.clear();
    assert!(w.active().is_empty());
    assert_stopped(&[&a, &b]).await;
}

#[tokio::test]
async fn watch_dedup_and_cap() {
    // 연결을 오래 붙잡는 SSE.
    let (base, log) =
        serve(|_| Resp::Sse(vec![(0, ": hi\n\n".into()), (60_000, String::new())])).await;
    let (c, _) = client(&base);
    let cfg = WatchConfig {
        limits: SseLimits::default(),
        reconnect_delay: Duration::from_millis(50),
    };
    let w = Watches::new(cfg);
    let id = |n: u32| format!("22222222-2222-4222-8222-{n:012}");
    w.watch(c.clone(), WF, &id(1), sink().1).unwrap();
    w.watch(c.clone(), WF, &id(1), sink().1).unwrap();
    assert_eq!(w.active(), [id(1)]);
    for n in 2..=6 {
        w.watch(c.clone(), WF, &id(n), sink().1).unwrap();
    }
    // 최대 5개 — 가장 오래된 1번이 빠진다.
    assert_eq!(w.active(), (2..=6).map(id).collect::<Vec<_>>());
    w.unwatch(&id(4));
    assert_eq!(w.active(), [id(2), id(3), id(5), id(6)]);
    // 남은 2·3·5·6번이 연결한다(1·4번은 연결 전에 해제됐을 수 있다).
    wait_until(|| lines(&log).len() >= 4).await;
    // 같은 실행 두 번 구독해도 연결은 하나.
    let first = format!("GET /api/v1/workflows/{WF}/executions/{}/events", id(1));
    assert!(lines(&log).iter().filter(|l| **l == first).count() <= 1);
    assert!(w.watch(c, WF, "nope", sink().1).is_err());
}

#[tokio::test]
async fn oauth_401_keeps_session() {
    let (base, log) = serve(|line| {
        if line.starts_with("GET /api/v1/workflows/") {
            Resp::Json(
                401,
                json!({"success": false, "code": "CREDENTIAL_EXPIRED", "message": "OAuth 토큰이 만료되었습니다."}),
            )
        } else {
            Resp::Json(400, json!({}))
        }
    })
    .await;
    let (c, keys) = client(&base);
    let r = workflows::get(&c, WF).await;
    assert_eq!(r.unwrap_err(), WebError::ExternalExpired);
    assert!(keys.get(SESSION_ACCOUNT).unwrap().is_some());
    // 갱신도 하지 않는다.
    assert!(lines(&log).iter().all(|l| !l.contains("/auth/")));
}

#[tokio::test]
async fn dashboard_summary_parses_and_ignores_unknown() {
    // BE `WorkflowDashboardSummaryResponse` — hourlyCounts 키는 Map<Integer,Long>라 "0"~"23".
    let (base, log) = serve(|_| {
        ok(json!({
            "metrics": {"todayRuns": 12, "percentageChange": 12.5, "averageDurationSeconds": 3.2,
                        "successRate": 97.8, "newField": 1},
            "hourlyCounts": {"0": 0, "9": 3, "23": 1},
            "workflowStats": {"total": 5, "active": 3, "inactive": 2, "errored": 1, "running": 0},
            "extra": "x"
        }))
    })
    .await;
    let (c, _) = client(&base);
    let s = dashboard::summary(&c).await.unwrap();
    assert_eq!(s.metrics.today_runs, 12);
    assert_eq!(s.metrics.percentage_change, 12.5);
    assert_eq!(s.metrics.success_rate, 97.8);
    assert_eq!(s.hourly_counts["9"], 3);
    assert_eq!(s.workflow_stats.total, 5);
    assert_eq!(s.workflow_stats.errored, 1);
    assert_eq!(lines(&log), vec!["GET /api/v1/workflows/dashboard/summary"]);
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["metrics"]["averageDurationSeconds"], 3.2);
    assert_eq!(v["workflowStats"]["running"], 0);
}

#[tokio::test]
async fn dashboard_executions_uses_cursor() {
    let (base, log) = serve(|line| {
        let item = if line.contains("/errors") {
            json!({"executionId": EX, "workflowId": WF, "workflowName": "w", "failedNodeId": "n1",
                   "failedNodeType": "AI", "errorMessage": "boom", "startedAt": "2026-11-10T01:00:00",
                   "finishedAt": null})
        } else {
            json!({"id": EX, "workflowId": WF, "workflowName": "w", "status": "SUCCESS",
                   "durationSeconds": 1.5, "triggerType": "MANUAL",
                   "startedAt": "2026-11-10T01:00:00", "finishedAt": null, "more": 1})
        };
        ok(json!({"content": [item], "hasNext": false, "nextCursor": null}))
    })
    .await;
    let (c, _) = client(&base);
    let p = dashboard::executions(&c, Some("abc".into())).await.unwrap();
    assert_eq!(p.content[0].workflow_name, "w");
    assert_eq!(p.content[0].duration_seconds, Some(1.5));
    assert_eq!(p.content[0].trigger_type.as_deref(), Some("MANUAL"));
    let e = dashboard::errors(&c, None).await.unwrap();
    assert_eq!(e.content[0].execution_id, EX);
    assert_eq!(e.content[0].error_message.as_deref(), Some("boom"));
    assert_eq!(
        lines(&log),
        vec![
            "GET /api/v1/workflows/dashboard/executions?size=20&cursor=abc",
            "GET /api/v1/workflows/dashboard/errors?size=20",
        ]
    );
    let v = serde_json::to_value(&p.content[0]).unwrap();
    assert_eq!(v["workflowId"], WF);
    assert!(v.get("more").is_none());
}
