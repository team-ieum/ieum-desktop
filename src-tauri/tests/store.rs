//! 앱 저장소. 모든 파일은 임시 폴더 안에서만 만든다.

use std::fs;

use std::collections::BTreeMap;

use ieum_desktop_lib::store::{Language, RunTrigger, Settings, Store, StoredWorkflow};
use ieum_engine::Workflow;
use ieum_local::github::watch::{PrSeen, Seen, WatchEntry};
use serde_json::json;

fn workflow() -> Workflow {
    serde_json::from_value(json!({
        "nodes": [{"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": "/tmp/x"}}],
        "edges": []
    }))
    .unwrap()
}

fn stored(id: &str) -> StoredWorkflow {
    StoredWorkflow {
        id: id.into(),
        name: "정리".into(),
        enabled: true,
        workflow: workflow(),
        updated_at: String::new(),
    }
}

#[test]
fn store_roundtrip_and_atomic_write() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());

    // 빈 id면 새 UUID를 붙이고 시각을 채운다.
    let saved = store.save_workflow(stored("")).unwrap();
    assert!(uuid::Uuid::parse_str(&saved.id).is_ok());
    assert!(!saved.updated_at.is_empty());

    let mut again = saved.clone();
    again.name = "바뀐 이름".into();
    store.save_workflow(again).unwrap();

    let got = store.get_workflow(&saved.id).unwrap();
    assert_eq!(got.name, "바뀐 이름");
    assert_eq!(got.workflow.nodes[0].id, "t");
    assert_eq!(store.list_workflows().unwrap().len(), 1);

    // 원자적 쓰기: 임시 파일이 남지 않는다.
    let names: Vec<String> = fs::read_dir(t.path().join("workflows"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![format!("{}.json", saved.id)]);

    // 설정: 없으면 기본값, 쓰고 읽으면 그대로.
    let s = store.get_settings().unwrap();
    assert_eq!(s.language, Language::System);
    assert!(!s.first_run_done);
    let s = Settings {
        language: Language::En,
        autostart: true,
        first_run_done: true,
        ..Settings::default()
    };
    store.set_settings(&s).unwrap();
    assert_eq!(store.get_settings().unwrap(), s);

    store.delete_workflow(&saved.id).unwrap();
    assert!(store.list_workflows().unwrap().is_empty());
}

#[test]
fn old_settings_get_onboarding_and_notify_defaults() {
    let t = tempfile::tempdir().unwrap();
    fs::write(
        t.path().join("settings.json"),
        r#"{"language":"system","autostart":true,"firstRunDone":true,"serverUrl":"https://green.kiacms.co.kr"}"#,
    )
    .unwrap();
    let s = Store::new(t.path().to_path_buf()).get_settings().unwrap();
    assert!(!s.onboarding_done);
    assert!(s.notify_manual_start && s.notify_manual_end && s.notify_watch_end);
    assert!(s.autostart);
    let d = Settings::default();
    assert!(d.notify_manual_start && d.notify_manual_end && d.notify_watch_end);
    let v = serde_json::to_value(&d).unwrap();
    for k in [
        "onboardingDone",
        "notifyManualStart",
        "notifyManualEnd",
        "notifyWatchEnd",
    ] {
        assert!(v.get(k).is_some(), "{k}");
    }
}

#[test]
fn old_run_without_trigger_reads_none() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let id = "00000000-0000-4000-8000-000000000001";
    fs::create_dir_all(t.path().join("runs")).unwrap();
    let old = json!({
        "run": {"runId": id, "status": "succeeded", "validationErrors": [], "rejectReason": null, "nodes": {}},
        "workflowId": "w", "startedAt": "2026-01-01T00:00:00Z", "finishedAt": "2026-01-01T00:00:01Z",
        "fileOps": [], "allowedRoots": [], "undo": null
    });
    fs::write(t.path().join(format!("runs/{id}.json")), old.to_string()).unwrap();

    assert_eq!(store.list_runs(None).unwrap()[0].trigger, None);
    assert_eq!(store.get_run(id).unwrap().trigger, None);
}

#[test]
fn ids_must_be_uuid() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let store = Store::new(data.clone());
    // 앱 데이터 밖에 미끼 파일
    fs::write(t.path().join("x.json"), "{}").unwrap();

    for bad in ["../x", "x", "../../etc/passwd", "a/b", ""] {
        if !bad.is_empty() {
            assert!(store.save_workflow(stored(bad)).is_err(), "save {bad}");
        }
        assert!(store.get_workflow(bad).is_err(), "get {bad}");
        assert!(store.delete_workflow(bad).is_err(), "delete {bad}");
        assert!(store.get_run(bad).is_err(), "get_run {bad}");
    }
    assert!(t.path().join("x.json").exists());
    assert!(!t.path().join("workflows").exists());
}

#[test]
fn account_suggest_defaults_on() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.notify_account_suggest);
    assert!(Settings::default().notify_account_suggest);
}

#[test]
fn chat_file_default_update_and_deleted_with_workflow() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let id = store.save_workflow(stored("")).unwrap().id;
    assert!(store.get_chat(&id).unwrap().messages.is_empty()); // 없으면 빈 값
    let after = store
        .update_chat(&id, |c| c.state.outline = vec!["새 파일".into()])
        .unwrap();
    assert_eq!(after.state.outline, ["새 파일"]);
    assert!(t.path().join("chats").join(format!("{id}.json")).exists());
    assert_eq!(store.get_chat(&id).unwrap().state.outline, ["새 파일"]);
    assert!(store.get_chat("../x").is_err()); // id는 UUID만
    store.delete_workflow(&id).unwrap();
    assert!(!t.path().join("chats").join(format!("{id}.json")).exists()); // 같이 지운다
    assert!(!t.path().join("chat-usage").exists()); // 쓴 사용량이 없으면 남길 것도 없다
    // 채팅 파일이 없는 워크플로우도 지워진다.
    let id2 = store.save_workflow(stored("")).unwrap().id;
    store.delete_workflow(&id2).unwrap();
}

#[test]
fn chat_write_after_workflow_delete_is_rejected() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let id = store.save_workflow(stored("")).unwrap().id;
    store.delete_workflow(&id).unwrap();
    // 삭제 뒤 도착한 쓰기가 고아 채팅 파일을 남기지 않는다.
    assert!(
        store
            .update_chat(&id, |c| c.state.outline.push("a".into()))
            .is_err()
    );
    assert!(store.save_chat(&id, &Default::default()).is_err());
    assert!(!t.path().join("chats").join(format!("{id}.json")).exists());
}

#[test]
fn all_chat_usage_reads_every_chat_file() {
    use ieum_local::ai::AiUsage;
    use ieum_local::builder::UsageEntry;
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    assert!(store.all_chat_usage().unwrap().is_empty()); // chats 폴더가 없어도 된다
    let id = store.save_workflow(stored("")).unwrap().id;
    let u = AiUsage {
        source: "anthropic".into(),
        model: Some("m".into()),
        input_tokens: 5,
        output_tokens: 2,
        cache_read_tokens: None,
        cache_write_tokens: None,
        credential: Some("k1".into()),
    };
    store
        .update_chat(&id, |c| {
            c.usage.push(UsageEntry {
                at: "2026-11-13T00:00:00.000Z".into(),
                usage: u.clone(),
            })
        })
        .unwrap();
    fs::write(
        t.path()
            .join("chats/33333333-3333-4333-8333-333333333333.json"),
        "깨진 파일",
    )
    .unwrap();
    let all = store.all_chat_usage().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        (
            all[0].workflow_id.as_str(),
            all[0].at.as_str(),
            &all[0].usage
        ),
        (id.as_str(), "2026-11-13T00:00:00.000Z", &u)
    );
    // 워크플로우를 지우면 대화는 지워지고 사용량은 남는다(통계에 계속 합산).
    store.delete_workflow(&id).unwrap();
    assert!(!t.path().join("chats").join(format!("{id}.json")).exists());
    let all = store.all_chat_usage().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        (
            all[0].workflow_id.as_str(),
            all[0].at.as_str(),
            &all[0].usage
        ),
        (id.as_str(), "2026-11-13T00:00:00.000Z", &u)
    );
}

/// 지우다 실패하면 워크플로우가 남아 다시 지울 수 있고, 사용량은 두 번 세지 않는다.
#[test]
fn delete_workflow_failure_keeps_workflow_and_counts_usage_once() {
    use ieum_local::ai::AiUsage;
    use ieum_local::builder::UsageEntry;
    use std::os::unix::fs::PermissionsExt;
    // 막을 폴더: 채팅 파일을 못 지우는 경우, 사용량을 못 옮기는 경우.
    for blocked in ["chats", "chat-usage"] {
        let t = tempfile::tempdir().unwrap();
        let store = Store::new(t.path().to_path_buf());
        let id = store.save_workflow(stored("")).unwrap().id;
        let u = AiUsage {
            source: "anthropic".into(),
            model: None,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: None,
            cache_write_tokens: None,
            credential: None,
        };
        store
            .update_chat(&id, |c| {
                c.usage.push(UsageEntry {
                    at: "2026-11-13T00:00:00.000Z".into(),
                    usage: u.clone(),
                })
            })
            .unwrap();
        let dir = t.path().join(blocked);
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        let r = store.delete_workflow(&id);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(r.is_err(), "{blocked}");
        assert!(store.get_workflow(&id).is_ok(), "{blocked}");
        assert_eq!(store.all_chat_usage().unwrap().len(), 1, "{blocked}");
        store.delete_workflow(&id).unwrap(); // 다시 지울 수 있다
        assert_eq!(store.all_chat_usage().unwrap().len(), 1, "{blocked}");
    }
}

/// 앞선 삭제가 마지막 단계에서 실패해 남은 사용량 사본이 있으면, 다시 지울 때 덮지 않고 이어 붙인다.
#[test]
fn delete_workflow_appends_to_a_leftover_usage_copy() {
    use ieum_local::ai::AiUsage;
    use ieum_local::builder::UsageEntry;
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let id = store.save_workflow(stored("")).unwrap().id;
    let entry = |at: &str| UsageEntry {
        at: at.into(),
        usage: AiUsage {
            source: "anthropic".into(),
            model: None,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: None,
            cache_write_tokens: None,
            credential: None,
        },
    };
    // 남은 사본(첫 삭제 때 옮겨 둔 것).
    let kept = t.path().join("chat-usage");
    fs::create_dir_all(&kept).unwrap();
    fs::write(
        kept.join(format!("{id}.json")),
        serde_json::to_string(&vec![entry("2026-11-13T00:00:00.000Z")]).unwrap(),
    )
    .unwrap();
    // 그 뒤 채팅을 더 했다.
    store
        .update_chat(&id, |c| c.usage.push(entry("2026-11-14T00:00:00.000Z")))
        .unwrap();
    assert_eq!(store.all_chat_usage().unwrap().len(), 2);
    store.delete_workflow(&id).unwrap();
    assert_eq!(store.all_chat_usage().unwrap().len(), 2);
}

/// 저장본의 HTTP 본문 줄이기 — 대리쌍을 가르지 않고, HTTP가 아닌 노드·작은 본문은 그대로.
#[test]
fn clip_http_bodies_keeps_surrogate_pairs() {
    use ieum_desktop_lib::store::clip_http_bodies;
    use ieum_engine::RunResult;

    let wf: Workflow = serde_json::from_value(json!({
        "nodes": [
            {"id": "h", "type": "HTTP", "event": "http.request", "config": {}},
            {"id": "s", "type": "HTTP", "event": "http.request", "config": {}},
            {"id": "f", "type": "TRANSFORM", "event": "formatter.text_trim", "config": {}}
        ],
        "edges": []
    }))
    .unwrap();
    // 99,999번째 단위 뒤에 이모지(2단위)가 걸친다.
    let big = format!("{}😀{}", "a".repeat(99_999), "b".repeat(10));
    let rec = |body: serde_json::Value| json!({"status": "succeeded", "output": {"statusCode": 200, "body": body}, "error": null});
    let mut run: RunResult = serde_json::from_value(json!({
        "runId": "r", "status": "succeeded", "validationErrors": [], "rejectReason": null,
        "nodes": {"h": rec(json!(big)), "s": rec(json!({"a": 1})), "f": rec(json!(big))}
    }))
    .unwrap();
    clip_http_bodies(&mut run, &wf);
    let h = &run.nodes["h"];
    assert_eq!(
        h.output.as_ref().unwrap()["body"],
        json!(format!("{}…(잘림, 전체 100011자)", "a".repeat(99_999)))
    );
    assert!(h.output_clipped);
    assert_eq!(
        run.nodes["s"].output.as_ref().unwrap()["body"],
        json!({"a": 1})
    );
    assert!(!run.nodes["s"].output_clipped);
    assert_eq!(run.nodes["f"].output.as_ref().unwrap()["body"], json!(big));
}

fn watch_entries() -> (WatchEntry, WatchEntry) {
    let at = chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
        .unwrap()
        .to_utc();
    let issue = WatchEntry {
        owner: "o".into(),
        repo: "r".into(),
        config_hash: "h1".into(),
        last_checked_at: at,
        seen: Seen::Issue { issue_max: 7 },
    };
    let pr = WatchEntry {
        config_hash: "h2".into(),
        seen: Seen::Pr {
            pr_max: 9,
            open: BTreeMap::from([(
                9,
                PrSeen {
                    head_sha: "abc".into(),
                    draft: true,
                },
            )]),
            open_complete: true,
        },
        ..issue.clone()
    };
    (issue, pr)
}

#[test]
fn github_watch_roundtrip_and_remove() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    assert!(store.get_github_watch().unwrap().is_empty());
    let (issue, pr) = watch_entries();
    store.set_github_watch("a", &issue).unwrap();
    store.set_github_watch("b", &pr).unwrap();
    let got = store.get_github_watch().unwrap();
    assert_eq!(got.get("a"), Some(&issue));
    assert_eq!(got.get("b"), Some(&pr));
    store.remove_github_watch("a").unwrap();
    store.remove_github_watch("없음").unwrap();
    let got = store.get_github_watch().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got.get("b"), Some(&pr));
    assert_eq!(
        store.created_issues_path(),
        t.path().join("github_created.json")
    );
}

#[test]
fn github_watch_corrupt_is_empty() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    fs::write(t.path().join("github_watch.json"), "{깨짐").unwrap();
    assert!(store.get_github_watch().unwrap().is_empty());
    let (issue, _) = watch_entries();
    store.set_github_watch("a", &issue).unwrap();
    assert_eq!(store.get_github_watch().unwrap().len(), 1);
}

#[test]
fn run_trigger_github_serializes() {
    assert_eq!(
        serde_json::to_string(&RunTrigger::Github).unwrap(),
        "\"github\""
    );
}
