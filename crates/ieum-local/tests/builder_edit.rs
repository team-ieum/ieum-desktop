//! Builder 편집 동작 — status·events·outline·add·connect·disconnect·remove·rename·pick-folder.

mod builder_support;

use builder_support::MemHost;
use ieum_local::builder::*;
use serde_json::{Value, json};

const W: &str = "11111111-1111-4111-8111-111111111111";

async fn run(b: &Builder, op: Value) -> Reply {
    b.apply(W, Op::from_value(op).unwrap(), None).await
}

fn trig() -> Value {
    json!({"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/h/in"}})
}

/// t → a(local.ai) → m(local.file_move)
fn chain() -> Value {
    json!({
        "nodes": [
            trig(),
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p"}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {}}
        ],
        "edges": [{"source": "t", "target": "a"}, {"source": "a", "target": "m"}]
    })
}

#[tokio::test]
async fn add_links_to_last_node_and_turns_workflow_off() {
    let h = MemHost::with(W, json!({"nodes": [trig()], "edges": []}));
    let b = Builder::new(h.clone()).unwrap();
    let r = run(
        &b,
        json!({"op": "add", "args": {"event": "local.file_trash", "label": "버리기"}}),
    )
    .await;
    assert!(r.ok, "{r:?}");
    let id = r.data["id"].as_str().unwrap().to_string();
    assert!(uuid::Uuid::parse_str(&id).is_ok());
    assert_eq!(r.data["needsAi"], false);
    let d = h.doc(W);
    assert!(!d.enabled); // 첫 편집에서 꺼진다
    let n = d.workflow.nodes.last().unwrap();
    assert_eq!(
        (
            n.r#type.as_deref(),
            n.label.as_deref(),
            n.position.is_none()
        ),
        (Some("ACTION"), Some("버리기"), true)
    );
    assert_eq!(
        (
            d.workflow.edges[0].source.as_str(),
            d.workflow.edges[0].target.as_str()
        ),
        ("t", id.as_str())
    );
    // 채팅 시작 시점 노드는 passed, 새 노드는 untested.
    let s = run(&b, json!({"op": "status"})).await;
    assert_eq!(s.data["nodes"][0]["state"], "passed");
    assert_eq!(s.data["nodes"][1]["state"], "untested");
    assert_eq!(s.data["enabled"], false);
}

#[tokio::test]
async fn first_node_is_the_only_trigger() {
    let h = MemHost::with(W, json!({"nodes": [], "edges": []}));
    let b = Builder::new(h.clone()).unwrap();
    let code = |r: Reply| r.code;
    assert_eq!(
        code(
            run(
                &b,
                json!({"op": "add", "args": {"event": "local.ai", "label": "x"}})
            )
            .await
        ),
        Some(Code::TriggerRule)
    );
    assert_eq!(
        code(
            run(
                &b,
                json!({"op": "add", "args": {"event": "gmail.send", "label": "x"}})
            )
            .await
        ),
        Some(Code::EventUnknown)
    );
    assert!(
        run(
            &b,
            json!({"op": "add", "args": {"event": "local.folder_new_file", "label": "새 파일"}})
        )
        .await
        .ok
    );
    assert!(h.doc(W).workflow.edges.is_empty());
    assert_eq!(
        code(
            run(
                &b,
                json!({"op": "add", "args": {"event": "local.folder_new_file", "label": "또"}})
            )
            .await
        ),
        Some(Code::TriggerRule)
    );
    assert_eq!(h.doc(W).workflow.nodes.len(), 1);
}

#[tokio::test]
async fn branch_edges_need_a_known_branch() {
    let cond = json!({"id": "c", "type": "CONDITION", "event": "branch.if", "config": {"branches": [{"key": "b_11111111", "name": "PDF", "rules": [{"left": "x", "operator": "isNotEmpty"}]}]}});
    let h = MemHost::with(
        W,
        json!({"nodes": [trig(), cond], "edges": [{"source": "t", "target": "c"}]}),
    );
    let b = Builder::new(h.clone()).unwrap();
    let add = |branch: Value, after: &str| json!({"op": "add", "args": {"event": "local.file_trash", "label": "x", "after": after, "branch": branch}});
    assert_eq!(
        run(
            &b,
            json!({"op": "add", "args": {"event": "local.file_trash", "label": "x", "after": "c"}})
        )
        .await
        .code,
        Some(Code::EdgeInvalid)
    );
    assert_eq!(
        run(&b, add(json!("nope"), "c")).await.code,
        Some(Code::EdgeInvalid)
    );
    // 분기가 아닌 노드에 갈래
    assert_eq!(
        run(&b, add(json!("b_11111111"), "t")).await.code,
        Some(Code::EdgeInvalid)
    );
    assert_eq!(h.saves(), 0);
    assert!(run(&b, add(json!("b_11111111"), "c")).await.ok);
    assert!(run(&b, add(json!("else"), "c")).await.ok);
    let kinds: Vec<_> = h
        .doc(W)
        .workflow
        .edges
        .iter()
        .filter(|e| e.source == "c")
        .map(|e| e.condition_type.clone().unwrap())
        .collect();
    assert_eq!(kinds, ["b_11111111", "else"]);
}

#[tokio::test]
async fn ai_node_connection_is_filled_by_app_not_by_caller() {
    let h = MemHost::with(W, json!({"nodes": [trig()], "edges": []}));
    let b = Builder::new(h.clone()).unwrap();
    let add = Op::Add {
        event: "local.ai".into(),
        label: "분류".into(),
        after: None,
        branch: None,
    };
    // 터미널: 연결된 것이 없으면 비워 두고 알린다.
    let r = b.apply(W, add.clone(), None).await;
    assert_eq!(r.data["needsAi"], true);
    assert!(!h.doc(W).workflow.nodes[1].config.contains_key("provider"));
    // 터미널: 앱 기본 연결.
    h.set_ai(Some(AiDefault {
        provider: "cli:claude".into(),
        credential: None,
        model: None,
        effort: None,
    }));
    let r = b.apply(W, add.clone(), None).await;
    assert_eq!(r.data["needsAi"], false);
    assert_eq!(h.doc(W).workflow.nodes[2].config["provider"], "cli:claude");
    // 채팅 패널: 그 채팅의 키·모델이 앱 기본보다 먼저.
    let chat = AiDefault {
        provider: "api:anthropic".into(),
        credential: Some("k1".into()),
        model: Some("claude-haiku-5-5".into()),
        effort: Some("high".into()),
    };
    b.apply(W, add.clone(), Some(chat)).await;
    let c = &h.doc(W).workflow.nodes[3].config;
    assert_eq!(
        (&c["provider"], &c["credential"], &c["model"], &c["effort"]),
        (
            &json!("api:anthropic"),
            &json!("k1"),
            &json!("claude-haiku-5-5"),
            &json!("high")
        )
    );
    // 터미널(앱 기본)은 수준을 채우지 않는다.
    assert!(!h.doc(W).workflow.nodes[2].config.contains_key("effort"));
    // OpenAI는 기본 모델이 없다.
    let openai = AiDefault {
        provider: "api:openai".into(),
        credential: None,
        model: None,
        effort: None,
    };
    let r = b.apply(W, add, Some(openai)).await;
    assert_eq!(r.data["needsAi"], true);
}

#[tokio::test]
async fn connect_disconnect_remove_rename_rules() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    let c = |from: &str, to: &str| json!({"op": "connect", "args": {"from": from, "to": to}});
    for (from, to, code) in [
        ("a", "a", Code::EdgeInvalid),
        ("a", "t", Code::EdgeInvalid),
        ("m", "a", Code::EdgeInvalid),
        ("a", "zz", Code::NodeNotFound),
    ] {
        assert_eq!(run(&b, c(from, to)).await.code, Some(code), "{from}->{to}");
    }
    assert!(run(&b, c("t", "a")).await.ok); // 이미 있는 연결 = 그대로
    assert_eq!((h.saves(), h.doc(W).workflow.edges.len()), (0, 2));
    assert!(run(&b, c("t", "m")).await.ok);
    assert_eq!(h.doc(W).workflow.edges.len(), 3);
    let cut = json!({"op": "disconnect", "args": {"from": "t", "to": "m"}});
    assert!(run(&b, cut.clone()).await.ok);
    assert_eq!(run(&b, cut).await.code, Some(Code::EdgeInvalid));
    assert_eq!(
        run(&b, json!({"op": "remove", "args": {"id": "t"}}))
            .await
            .code,
        Some(Code::TriggerRule)
    );
    assert_eq!(
        run(
            &b,
            json!({"op": "rename", "args": {"id": "a", "label": "  "}})
        )
        .await
        .code,
        Some(Code::BadRequest)
    );
    assert!(
        run(
            &b,
            json!({"op": "rename", "args": {"id": "a", "label": "분류"}})
        )
        .await
        .ok
    );
    h.update_chat(W, &mut |ch| {
        ch.state.tests.insert(
            "a".into(),
            TestEntry {
                hash: "x".into(),
                status: TestStatus::Success,
            },
        );
    })
    .unwrap();
    assert!(
        run(&b, json!({"op": "remove", "args": {"id": "a"}}))
            .await
            .ok
    );
    let d = h.doc(W);
    assert_eq!(
        d.workflow
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["t", "m"]
    );
    assert!(d.workflow.edges.is_empty());
    assert!(!h.load_chat(W).unwrap().state.tests.contains_key("a"));
}

#[tokio::test]
async fn outline_and_events() {
    let h = MemHost::with(W, json!({"nodes": [trig()], "edges": []}));
    let b = Builder::new(h.clone()).unwrap();
    assert!(
        run(
            &b,
            json!({"op": "outline", "args": {"steps": ["새 파일", "분류"]}})
        )
        .await
        .ok
    );
    assert!(h.doc(W).enabled && h.saves() == 0); // 이름을 안 바꾸면 저장본은 그대로
    assert!(
        run(
            &b,
            json!({"op": "outline", "args": {"name": "영수증 정리", "steps": ["새 파일", "분류"]}})
        )
        .await
        .ok
    );
    assert_eq!(
        (h.doc(W).name.as_str(), h.doc(W).enabled),
        ("영수증 정리", false)
    );
    assert_eq!(
        run(&b, json!({"op": "status"})).await.data["outline"],
        json!(["새 파일", "분류"])
    );
    let e = run(&b, json!({"op": "events"})).await;
    assert_eq!(e.data["events"].as_array().unwrap().len(), 6);
    assert_eq!(
        e.data["events"][0],
        json!({"event": "local.folder_new_file", "title": "폴더에 새 파일", "description": "고른 폴더에 새 파일이 생기면 시작해요.", "nodeType": "TRIGGER"})
    );
}

#[tokio::test]
async fn pick_folder_reloads_doc_after_wait() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    let pick = json!({"op": "pick-folder", "args": {"id": "m", "field": "baseFolder"}});
    assert_eq!(
        run(
            &b,
            json!({"op": "pick-folder", "args": {"id": "a", "field": "prompt"}})
        )
        .await
        .code,
        Some(Code::NotFolderField)
    );
    h.push_pick(None);
    assert_eq!(run(&b, pick.clone()).await.code, Some(Code::PickCancelled));
    h.bad_folder("/etc");
    h.push_pick(Some("/etc".into()));
    assert_eq!(
        run(&b, pick.clone()).await.code,
        Some(Code::FolderNotAllowed)
    );
    assert_eq!(h.saves(), 0);
    // 고르기를 기다리는 사이 사용자가 캔버스에서 다른 노드를 고쳤다 → 그 수정이 살아 있어야 한다.
    h.push_pick(Some("/h/out".into()));
    h.on_pick(|host| {
        host.put_doc(W, |d| {
            d.workflow.nodes[1].label = Some("캔버스에서 바꾼 이름".into())
        })
    });
    let r = run(&b, pick).await;
    assert_eq!(r.data["path"], "/h/out");
    // 창 제목이 어느 칸의 폴더인지 말한다
    assert!(h.pick_titles().last().unwrap().contains("기준 폴더"));
    let d = h.doc(W);
    assert_eq!(
        d.workflow.nodes[1].label.as_deref(),
        Some("캔버스에서 바꾼 이름")
    );
    assert_eq!(d.workflow.nodes[2].config["baseFolder"], "/h/out");
}

#[tokio::test]
async fn user_editing_rejects_changes_but_not_reads() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    h.set_editing(W, true); // 사람이 캔버스에서 고치는 중(미저장)
    for op in [
        json!({"op": "outline", "args": {"name": "x"}}),
        json!({"op": "add", "args": {"event": "local.file_trash", "label": "x"}}),
        json!({"op": "set", "args": {"id": "a", "values": {"prompt": "x"}}}),
        json!({"op": "connect", "args": {"from": "t", "to": "m"}}),
        json!({"op": "disconnect", "args": {"from": "t", "to": "a"}}),
        json!({"op": "remove", "args": {"id": "m"}}),
        json!({"op": "rename", "args": {"id": "a", "label": "x"}}),
        json!({"op": "pick-folder", "args": {"id": "m", "field": "baseFolder"}}),
        json!({"op": "test", "args": {"id": "a"}}),
        json!({"op": "finish"}),
    ] {
        let r = run(&b, op.clone()).await;
        assert_eq!(
            (r.code, r.exit_code()),
            (Some(Code::UserEditing), 2),
            "{op}"
        );
    }
    assert_eq!(h.saves(), 0);
    assert!(h.doc(W).enabled); // 끄지도 않는다
    for op in ["status", "events"] {
        assert!(run(&b, json!({"op": op})).await.ok, "{op}"); // 읽기는 된다
    }
    h.set_editing(W, false); // 저장·버림 뒤에는 된다
    let rename = json!({"op": "rename", "args": {"id": "a", "label": "x"}});
    assert!(run(&b, rename).await.ok);
    // 폴더 창을 기다리는 사이 사람이 고치기 시작했다 → 고른 값도 저장하지 않는다.
    let saves = h.saves();
    h.push_pick(Some("/h/out".into()));
    h.on_pick(|host| host.set_editing(W, true));
    assert_eq!(
        run(
            &b,
            json!({"op": "pick-folder", "args": {"id": "m", "field": "baseFolder"}})
        )
        .await
        .code,
        Some(Code::UserEditing)
    );
    assert_eq!(h.saves(), saves);
}

#[tokio::test]
async fn save_failures_map_to_codes_without_leaking_text() {
    let h = MemHost::with(W, chain());
    let b = Builder::new(h.clone()).unwrap();
    let rename = json!({"op": "rename", "args": {"id": "a", "label": "x"}});
    h.fail_next_save(SaveError::Stale);
    assert_eq!(run(&b, rename.clone()).await.code, Some(Code::Stale));
    h.fail_next_save(SaveError::Rejected(
        "홈 폴더 아래 폴더만 쓸 수 있어요: /etc/비밀".into(),
    ));
    let r = run(&b, rename).await;
    assert_eq!(r.code, Some(Code::SaveRejected));
    assert!(!r.to_line().contains("비밀"));
}
