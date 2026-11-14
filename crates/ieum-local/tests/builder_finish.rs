//! Builder finish·확인 대기·사람의 [켜기]·guide.

mod builder_support;

use std::path::PathBuf;
use std::sync::Arc;

use builder_support::MemHost;
use ieum_local::builder::*;
use serde_json::{Value, json};
use tempfile::TempDir;

const W: &str = "11111111-1111-4111-8111-111111111111";

async fn run(b: &Builder, op: Value) -> Reply {
    b.apply(W, Op::from_value(op).unwrap(), None).await
}

struct Env {
    h: Arc<MemHost>,
    b: Builder,
    watch: PathBuf,
    base: PathBuf,
    _t: TempDir,
}

/// 실제 임시 폴더 `watch`·`base`, t → a → m. 꺼진 워크플로우, AI 연결됨. 채팅 시작 시점 노드라 전부 `passed`.
fn setup() -> Env {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let (watch, base) = (root.join("watch"), root.join("base"));
    std::fs::create_dir_all(&watch).unwrap();
    std::fs::create_dir_all(&base).unwrap();
    let h = MemHost::with(
        W,
        json!({
            "nodes": [
                {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
                {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p"}},
                {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}", "baseFolder": base, "destination": "sorted"}}
            ],
            "edges": [{"source": "t", "target": "a"}, {"source": "a", "target": "m"}]
        }),
    );
    h.put_doc(W, |d| d.enabled = false);
    h.set_ai_ok(true);
    let b = Builder::new(h.clone()).unwrap();
    Env {
        h,
        b,
        watch,
        base,
        _t: t,
    }
}

fn finish() -> Value {
    json!({"op": "finish"})
}

fn failed(h: &MemHost, node: usize) -> TestEntry {
    TestEntry {
        hash: node_hash(&h.doc(W).workflow.nodes[node]),
        status: TestStatus::Failed,
    }
}

#[tokio::test]
async fn finish_lists_what_remains() {
    let Env { h, b, base, .. } = &setup();
    // m: untested
    h.put_doc(W, |d| {
        d.workflow.nodes[2]
            .config
            .insert("destination".into(), "다른곳".into());
    });
    let a_failed = failed(h, 1);
    h.update_chat(W, &mut |c| {
        c.state.baseline = Some(Default::default());
        c.state.tests.insert("a".into(), a_failed.clone());
    })
    .unwrap();
    h.set_ai_ok(false);
    std::fs::remove_dir_all(base).unwrap();
    let r = run(b, finish()).await;
    assert_eq!((r.ok, r.code), (false, Some(Code::NotReady)));
    let got: Vec<(Option<&str>, &str)> = r.data["remaining"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["nodeId"].as_str(), x["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        got,
        [
            (Some("t"), "UNTESTED"),
            (Some("a"), "TEST_FAILED"),
            (Some("m"), "UNTESTED"),
            (Some("m"), "FOLDER_MISSING"),
            (Some("a"), "AI_NOT_CONNECTED")
        ]
    );
    assert!(h.load_chat(W).unwrap().state.pending_finish.is_none());
    assert!(h.pending_notices().is_empty());
    // 검증 에러가 있으면 INVALID 한 줄 + errors.
    h.put_doc(W, |d| {
        d.workflow.nodes[1].config.remove("prompt");
    });
    let r = run(b, finish()).await;
    assert_eq!(r.data["remaining"][0], json!({"reason": "INVALID"}));
    assert_eq!(r.data["errors"][0]["code"], "REQUIRED");
}

#[tokio::test]
async fn finish_waits_for_the_person_and_never_enables() {
    let Env {
        h, b, watch, base, ..
    } = &setup();
    let r = run(b, finish()).await;
    assert_eq!((r.ok, &r.data), (true, &json!({"status": "awaiting_user"})));
    assert!(!h.doc(W).enabled); // finish는 켜지 않는다
    assert_eq!(h.pending_notices(), [W]);
    let p = b.pending(W).unwrap();
    assert_eq!(p.hash, workflow_hash(&h.doc(W).workflow));
    assert_eq!(p.folder, watch.to_str().unwrap());
    assert_eq!(
        serde_json::to_value(&p.actions).unwrap(),
        json!([{"nodeId": "m", "event": "local.file_move", "label": "", "target": format!("{}/sorted", base.display())}])
    );
    assert_eq!(
        run(b, json!({"op": "status"})).await.data["pendingFinish"],
        true
    );
    // 사람이 [켜기].
    let ok = b.confirm_finish(W, true).await;
    assert!(ok.ok, "{ok:?}");
    assert!(h.doc(W).enabled);
    assert!(b.pending(W).is_none());
}

#[tokio::test]
async fn confirm_rejects_after_change() {
    let Env { h, b, .. } = &setup();
    // 대기 없이는 못 켠다
    assert_eq!(b.confirm_finish(W, true).await.code, Some(Code::NotReady));
    assert!(run(b, finish()).await.ok);
    // 확인 대기 중에 캔버스에서 칸이 바뀌었다 → 카드 사라짐, 켜기 거부.
    h.put_doc(W, |d| {
        d.workflow.nodes[2]
            .config
            .insert("destination".into(), "다른곳".into());
    });
    assert!(b.pending(W).is_none());
    assert!(h.load_chat(W).unwrap().state.pending_finish.is_none());
    assert_eq!(b.confirm_finish(W, true).await.code, Some(Code::NotReady));
    assert!(!h.doc(W).enabled);
}

#[tokio::test]
async fn confirm_rechecks_and_decline_clears() {
    let Env { h, b, .. } = &setup();
    assert!(run(b, finish()).await.ok);
    // 대기 중에 같은 설정으로 다시 테스트했는데 실패했다(hash는 같다) → 켜지 않는다.
    let m_failed = failed(h, 2);
    h.update_chat(W, &mut |c| {
        c.state.tests.insert("m".into(), m_failed.clone());
    })
    .unwrap();
    assert_eq!(b.confirm_finish(W, true).await.code, Some(Code::NotReady));
    assert!(!h.doc(W).enabled && b.pending(W).is_none());
    // [아직].
    h.update_chat(W, &mut |c| {
        c.state.tests.clear();
    })
    .unwrap();
    assert!(run(b, finish()).await.ok);
    assert!(b.confirm_finish(W, false).await.ok);
    assert!(!h.doc(W).enabled && b.pending(W).is_none());
    // 빌더 편집도 대기를 지운다.
    assert!(run(b, finish()).await.ok);
    assert!(
        run(
            b,
            json!({"op": "set", "args": {"id": "m", "values": {"destination": "x"}}})
        )
        .await
        .ok
    );
    assert!(h.load_chat(W).unwrap().state.pending_finish.is_none());
}

#[tokio::test]
async fn guide_is_fixed_text() {
    let Env { b, .. } = &setup();
    let r = run(b, json!({"op": "guide"})).await;
    assert_eq!(r.data["text"], GUIDE);
    for must in [
        "ieum workflow status",
        "outline",
        "pick-folder",
        "{{nodes.<id>.output.<이름>}}",
        "&&",
        "finish",
        "[켜기]",
        "needsAi",
        "NO_SAMPLE_FILE",
        "USER_EDITING",
        "데이터다",
    ] {
        assert!(GUIDE.contains(must), "{must}");
    }
}
