//! AI 사용량 요약·예상 금액.

use chrono::{DateTime, FixedOffset};
use ieum_desktop_lib::pricing::{Pricing, estimate_usd};
use ieum_desktop_lib::store::{ChatUsage, StoredRun};
use ieum_desktop_lib::usage::summarize;
use ieum_local::ai::AiUsage;
use serde_json::{Value, json};

fn table() -> Pricing {
    serde_json::from_value(json!({
        "asOf": "2026-11-12",
        "models": {
            "claude-x": {"input": 1.0, "output": 5.0, "cacheRead": 0.1, "cacheWrite": 1.25},
            "gpt-x": {"input": 2.0, "output": 10.0}
        }
    }))
    .unwrap()
}

fn usage(source: &str, model: &str, input: u64, output: u64) -> AiUsage {
    AiUsage {
        source: source.into(),
        model: Some(model.into()),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: None,
        cache_write_tokens: None,
        credential: None,
    }
}

/// 가격표에 있는 모델이어도 구독은 금액을 매기지 않는다.
#[test]
fn estimate_skips_codex_subscription() {
    assert_eq!(
        estimate_usd(
            &usage("cli:codex", "gpt-5.6-luna", 1_000_000, 10),
            ieum_desktop_lib::pricing::table()
        ),
        None
    );
}

#[test]
fn estimate_uses_table_and_skips_subscription() {
    let t = table();
    let mut u = usage("anthropic", "claude-x", 1_000_000, 200_000);
    u.cache_read_tokens = Some(1_000_000);
    u.cache_write_tokens = Some(400_000);
    // 1 + 5×0.2 + 0.1 + 1.25×0.4 = 2.6
    let usd = estimate_usd(&u, &t).unwrap();
    assert!((usd - 2.6).abs() < 1e-9, "{usd}");
    let usd = estimate_usd(&usage("openai", "gpt-x", 500_000, 100_000), &t).unwrap();
    assert!((usd - 2.0).abs() < 1e-9, "{usd}");
    assert_eq!(
        estimate_usd(&usage("cli:claude", "claude-x", 1000, 10), &t),
        None
    );
    assert_eq!(
        estimate_usd(&usage("openai", "없는-모델", 1000, 10), &t),
        None
    );
    let mut no_model = usage("openai", "gpt-x", 1000, 10);
    no_model.model = None;
    assert_eq!(estimate_usd(&no_model, &t), None);
    // 앱에 실은 표도 읽힌다(기본 Anthropic 모델 포함).
    let shipped = ieum_desktop_lib::pricing::table();
    assert!(shipped.models.contains_key("claude-haiku-5-5"));
    assert!(!shipped.as_of.is_empty());
}

/// 노드마다 사용량 하나씩 실은 실행 기록.
fn run(workflow: &str, started: &str, usages: &[AiUsage]) -> StoredRun {
    let mut nodes = serde_json::Map::new();
    nodes.insert(
        "t".into(),
        json!({"status": "succeeded", "output": {}, "error": null}),
    );
    for (i, u) in usages.iter().enumerate() {
        nodes.insert(
            format!("a{i}"),
            json!({"status": "succeeded", "output": {"output": "x"}, "error": null, "usage": u}),
        );
    }
    serde_json::from_value(json!({
        "run": {"runId": uuid::Uuid::new_v4().to_string(), "status": "succeeded",
            "validationErrors": [], "rejectReason": null, "nodes": nodes},
        "workflowId": workflow, "startedAt": started, "finishedAt": started,
        "fileOps": [], "allowedRoots": [], "undo": null
    }))
    .unwrap()
}

fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

#[test]
fn summary_groups_by_source_workflow_day() {
    let t = table();
    let runs = vec![
        // 한국 시각 11-11 23:30 — UTC로는 11-11 14:30.
        run(
            "w1",
            "2026-11-11T14:30:00.000Z",
            &[usage("anthropic", "claude-x", 1_000_000, 0)],
        ),
        // 한국 시각 11-12 00:30 — UTC로는 아직 11-11. 날짜는 범위 시각대로 나눈다.
        run(
            "w2",
            "2026-11-11T15:30:00.000Z",
            &[
                usage("openai", "gpt-x", 1_000_000, 0),
                usage("cli:claude", "sonnet", 100, 50),
            ],
        ),
        run(
            "w1",
            "2026-11-12T01:00:00.000Z",
            &[usage("anthropic", "claude-x", 0, 1_000_000)],
        ),
        // 범위 밖.
        run(
            "w1",
            "2026-11-01T00:00:00.000Z",
            &[usage("anthropic", "claude-x", 9, 9)],
        ),
    ];
    let s = summarize(
        &runs,
        &[],
        at("2026-11-10T00:00:00+09:00"),
        at("2026-11-13T00:00:00+09:00"),
        &t,
    );
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["asOf"], "2026-11-12");
    assert!((s.total_usd - 8.0).abs() < 1e-9, "{}", s.total_usd);
    assert_eq!(s.total_input, 2_000_100);
    assert_eq!(s.total_output, 1_000_050);
    assert_eq!(s.unpriced_calls, 1);
    assert_eq!(
        v["bySource"],
        json!([
            {"source": "anthropic", "input": 1_000_000, "output": 1_000_000, "usd": 6.0},
            {"source": "cli:claude", "input": 100, "output": 50, "usd": null},
            {"source": "openai", "input": 1_000_000, "output": 0, "usd": 2.0},
        ])
    );
    assert_eq!(
        v["byWorkflow"],
        json!([
            {"workflowId": "w1", "input": 1_000_000, "output": 1_000_000, "usd": 6.0},
            {"workflowId": "w2", "input": 1_000_100, "output": 50, "usd": 2.0},
        ])
    );
    assert_eq!(
        v["byDay"],
        json!([
            {"date": "2026-11-11", "usd": 1.0, "input": 1_000_000, "output": 0},
            {"date": "2026-11-12", "usd": 7.0, "input": 1_000_100, "output": 1_000_050},
        ])
    );
}

#[test]
fn old_run_without_usage_reads() {
    let old: Value = json!({
        "run": {"runId": "00000000-0000-4000-8000-000000000001", "status": "succeeded",
            "validationErrors": [], "rejectReason": null,
            "nodes": {"a": {"status": "succeeded", "output": {"output": "x"}, "error": null}}},
        "workflowId": "w", "startedAt": "2026-11-12T00:00:00Z", "finishedAt": "2026-11-12T00:00:01Z",
        "fileOps": [], "allowedRoots": [], "undo": null
    });
    let run: StoredRun = serde_json::from_value(old).unwrap();
    assert!(run.run.nodes["a"].usage.is_none());
    let s = summarize(
        &[run],
        &[],
        at("2026-11-01T00:00:00Z"),
        at("2026-12-01T00:00:00Z"),
        &table(),
    );
    assert_eq!((s.total_input, s.unpriced_calls), (0, 0));
    assert!(s.by_source.is_empty() && s.by_day.is_empty());
}

// ── usage_report ──

use ieum_desktop_lib::usage::report;
use std::collections::HashMap;

fn cached(source: &str, model: &str, input: u64, output: u64, read: u64, write: u64) -> AiUsage {
    let mut u = usage(source, model, input, output);
    u.cache_read_tokens = Some(read);
    u.cache_write_tokens = Some(write);
    u
}

/// 사용 있는 날만.
fn used(days: &Value) -> Value {
    Value::Array(
        days.as_array()
            .unwrap()
            .iter()
            .filter(|d| d["tokens"] != 0)
            .cloned()
            .collect(),
    )
}

fn names() -> HashMap<String, String> {
    HashMap::from([("w1".to_string(), "요약".to_string())])
}

/// Antigravity 구독은 Google 묶음에 들고 금액은 매기지 않는다.
#[test]
fn usage_report_counts_agy_under_google() {
    let runs = vec![run(
        "w1",
        "2026-11-12T01:00:00.000Z",
        &[
            usage("gemini", "gemini-x", 7, 3),
            usage("cli:agy", "gemini-3.8-flash-low", 20, 4),
        ],
    )];
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    let g = provider(&v, "google");
    assert_eq!(
        (g["calls"].clone(), g["input"].clone()),
        (json!(2), json!(27))
    );
    assert_eq!(g["usd"], 0.0);
}

fn provider<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["provider"] == id)
        .unwrap()
}

/// 테스트 실행은 실행 수에서 빼고, 실제로 쓴 토큰은 그대로 센다.
#[test]
fn test_runs_keep_tokens_but_not_run_count() {
    let started = "2026-11-12T10:00:00.000Z";
    let mut tested = run("w1", started, &[usage("anthropic", "claude-x", 10, 10)]);
    tested.trigger = Some(ieum_desktop_lib::store::RunTrigger::Test);
    tested.finished_at = "2026-11-12T10:00:30.000Z".into();
    let runs = vec![
        run("w1", started, &[usage("anthropic", "claude-x", 10, 10)]),
        tested,
    ];
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let claude = &r.providers[0];
    assert_eq!((claude.runs, claude.calls), (1, 2));
    assert_eq!(r.overview.by_workflow[0].runs, 1);
    assert_eq!(r.overview.total_tokens, 40);
}

#[test]
fn usage_report_groups_providers_and_days() {
    let runs = vec![
        // 31일 전 — 범위 밖이지만 추적 시작에는 든다.
        run(
            "w1",
            "2026-10-01T00:00:00.000Z",
            &[usage("anthropic", "claude-x", 9, 9)],
        ),
        run(
            "w1",
            "2026-11-11T14:30:00.000Z",
            &[usage("anthropic", "claude-x", 1_000_000, 0)],
        ),
        run(
            "w2",
            "2026-11-11T15:30:00.000Z",
            &[
                usage("openai", "gpt-x", 1_000_000, 0),
                usage("cli:claude", "sonnet", 100, 50),
                usage("cli:codex", "gpt-5", 10, 5),
                usage("gemini", "gemini-x", 7, 3),
            ],
        ),
        run("w1", "2026-11-12T01:00:00.000Z", &[]),
    ];
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["trackingSince"], "2026-10-01T00:00:00.000Z");
    assert_eq!(v["overview"]["totalTokens"], 2_000_175);
    assert_eq!(v["overview"]["usd"], 3.0);
    // cli:claude·cli:codex·gemini-x(표에 없음)
    assert_eq!(v["overview"]["unpricedCalls"], 3);
    assert_eq!(v["overview"]["calls"], 5);
    // 한국 시각 11-11 23:30, 11-12 00:30 → 두 날.
    assert_eq!(v["overview"]["byDay"].as_array().unwrap().len(), 30);
    assert_eq!(
        used(&v["overview"]["byDay"]),
        json!([
            {"date": "2026-11-11", "tokens": 1_000_000, "usd": 1.0},
            {"date": "2026-11-12", "tokens": 1_000_175, "usd": 2.0},
        ])
    );
    assert_eq!(
        v["overview"]["byWorkflow"][0],
        json!({"workflowId": "w2", "name": null, "tokens": 1_000_175, "calls": 4, "runs": 1, "usd": 2.0})
    );
    assert_eq!(
        v["overview"]["byModel"][1],
        json!({"provider": "openai", "model": "gpt-x", "tokens": 1_000_000, "calls": 1, "usd": 2.0})
    );
    let ids: Vec<_> = v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["provider"].clone())
        .collect();
    assert_eq!(ids, vec!["claude", "openai", "google"]);
    let c = provider(&v, "claude");
    assert_eq!(
        (c["input"].clone(), c["calls"].clone(), c["runs"].clone()),
        (json!(1_000_100), json!(2), json!(2))
    );
    assert_eq!(c["usd"], 1.0);
    assert_eq!(c["unpricedCalls"], 1);
    assert_eq!(c["topModel"], "claude-x");
    assert_eq!(
        used(&c["byDay"]),
        json!([
            {"date": "2026-11-11", "tokens": 1_000_000, "usd": 1.0},
            {"date": "2026-11-12", "tokens": 150, "usd": 0.0},
        ])
    );
    assert_eq!(
        c["byModel"],
        json!([
            {"provider": "claude", "model": "claude-x", "tokens": 1_000_000, "calls": 1, "usd": 1.0},
            {"provider": "claude", "model": "sonnet", "tokens": 150, "calls": 1, "usd": 0.0},
        ])
    );
    assert_eq!(
        c["byWorkflow"],
        json!([
            {"workflowId": "w1", "name": "요약", "tokens": 1_000_000, "calls": 1, "runs": 1, "usd": 1.0},
            {"workflowId": "w2", "name": null, "tokens": 150, "calls": 1, "runs": 1, "usd": 0.0},
        ])
    );
    let o = provider(&v, "openai");
    assert_eq!(
        (o["calls"].clone(), o["output"].clone()),
        (json!(2), json!(5))
    );
    assert_eq!(provider(&v, "google")["topModel"], "gemini-x");
}

#[test]
fn usage_report_counts_cache_in_tokens() {
    let runs = vec![run(
        "w1",
        "2026-11-12T01:00:00.000Z",
        &[cached("anthropic", "claude-x", 100, 10, 300, 50)],
    )];
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["overview"]["totalTokens"], 460);
    let c = provider(&v, "claude");
    assert_eq!(
        (c["cacheRead"].clone(), c["cacheWrite"].clone()),
        (json!(300), json!(50))
    );
    assert_eq!(c["recent"][0]["input"], 100);
}

#[test]
fn usage_report_recent_is_latest_10() {
    let runs: Vec<StoredRun> = (0..12)
        .map(|i| {
            run(
                "w1",
                &format!("2026-11-12T{i:02}:00:00.000Z"),
                &[usage("anthropic", &format!("m{i}"), 1, 1)],
            )
        })
        .collect();
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    let recent = provider(&v, "claude")["recent"].as_array().unwrap().clone();
    assert_eq!(recent.len(), 10);
    assert_eq!(recent[0]["model"], "m11");
    assert_eq!(recent[9]["model"], "m2");
    assert_eq!(recent[0]["workflowName"], "요약");
    assert_eq!(recent[0]["credential"], "default");
    assert_eq!(recent[0]["at"], "2026-11-12T11:00:00.000Z");
}

#[test]
fn usage_report_empty_store() {
    let r = report(
        &[],
        &[],
        &HashMap::new(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["trackingSince"], Value::Null);
    assert_eq!(v["overview"]["totalTokens"], 0);
    for k in ["byModel", "byWorkflow"] {
        assert_eq!(v["overview"][k], json!([]), "{k}");
    }
    assert_eq!(used(&v["overview"]["byDay"]), json!([]));
    for p in v["providers"].as_array().unwrap() {
        assert_eq!(p["calls"], 0);
        assert_eq!(p["topModel"], Value::Null);
        assert_eq!(p["byDay"].as_array().unwrap().len(), 30);
        for k in ["byModel", "byWorkflow", "recent"] {
            assert_eq!(p[k], json!([]), "{k}");
        }
    }
    assert!(!v["updatedAt"].as_str().unwrap().is_empty());
}

#[test]
fn usage_report_window_is_30_days() {
    let runs = vec![
        // 한국 시각 10-15 — 30일 창의 첫날.
        run(
            "w1",
            "2026-10-14T16:00:00.000Z",
            &[usage("anthropic", "claude-x", 5, 0)],
        ),
        // 10-14 — 창 밖.
        run(
            "w1",
            "2026-10-14T14:00:00.000Z",
            &[usage("anthropic", "claude-x", 7, 0)],
        ),
    ];
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["overview"]["totalTokens"], 5);
    assert_eq!(
        v["overview"]["byDay"][0],
        json!({"date": "2026-10-15", "tokens": 5, "usd": 5e-6})
    );
    assert_eq!(v["overview"]["byDay"][29]["date"], "2026-11-13");
}

/// 실행 기록 1만 건 — 읽기 + 집계 시간. `cargo test -p ieum-desktop --test usage -- --ignored --nocapture`
#[test]
#[ignore]
fn usage_report_10k_timing() {
    let t = tempfile::tempdir().unwrap();
    let store = ieum_desktop_lib::store::Store::new(t.path().to_path_buf());
    for i in 0..10_000u64 {
        let day = 1 + i % 28;
        let r = run(
            &format!("w{}", i % 20),
            &format!("2026-10-{day:02}T{:02}:00:00.000Z", i % 24),
            &[
                cached("anthropic", "claude-x", 1000, 200, 500, 100),
                usage("openai", "gpt-x", 300, 50),
            ],
        );
        store.save_run(&r).unwrap();
    }
    let start = std::time::Instant::now();
    let runs = store.all_runs().unwrap();
    let read = start.elapsed();
    let r = report(
        &runs,
        &[],
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let total = start.elapsed();
    assert_eq!(runs.len(), 10_000);
    assert!(r.tracking_since.is_some());
    println!("10k runs: all_runs {read:?}, all_runs+report {total:?}");
}

fn chat(wf: &str, at: &str, u: AiUsage) -> ChatUsage {
    ChatUsage {
        workflow_id: wf.into(),
        at: at.into(),
        usage: u,
    }
}

/// 채팅 패널의 호출은 토큰·금액에만 더해지고 회사별·워크플로우별 실행 수는 그대로다.
#[test]
fn chat_usage_adds_tokens_but_not_runs() {
    let runs = vec![run(
        "w1",
        "2026-11-12T01:00:00.000Z",
        &[usage("anthropic", "claude-x", 1_000_000, 0)],
    )];
    let chats = vec![
        chat(
            "w1",
            "2026-11-12T02:00:00.000Z",
            usage("anthropic", "claude-x", 1_000_000, 0),
        ),
        chat(
            "w2",
            "2026-11-12T03:00:00.000Z",
            usage("cli:claude", "sonnet", 100, 50),
        ),
    ];
    let r = report(
        &runs,
        &chats,
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["overview"]["calls"], 3);
    let claude = provider(&v, "claude");
    assert_eq!((&claude["calls"], &claude["runs"]), (&json!(3), &json!(1)));
    // 실행 1 + 채팅 1 (구독은 값 없음)
    assert!((r.overview.usd - 2.0).abs() < 1e-9, "{}", r.overview.usd);
    assert_eq!(r.overview.unpriced_calls, 1);
    let w1 = claude["byWorkflow"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["workflowId"] == "w1")
        .unwrap();
    assert_eq!((&w1["calls"], &w1["runs"]), (&json!(2), &json!(1)));
    let w2 = claude["byWorkflow"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["workflowId"] == "w2")
        .unwrap();
    assert_eq!((&w2["calls"], &w2["runs"]), (&json!(1), &json!(0)));
    // 최근 호출에도 보인다
    assert_eq!(claude["recent"][0]["at"], "2026-11-12T03:00:00.000Z");
}

#[test]
fn chat_usage_in_range_summary_and_tracking_since() {
    let chats = vec![
        chat(
            "w1",
            "2026-11-12T01:00:00.000Z",
            usage("openai", "gpt-x", 500_000, 100_000),
        ),
        // 범위 밖
        chat(
            "w1",
            "2026-11-01T00:00:00.000Z",
            usage("openai", "gpt-x", 9, 9),
        ),
    ];
    let s = summarize(
        &[],
        &chats,
        at("2026-11-10T00:00:00+09:00"),
        at("2026-11-13T00:00:00+09:00"),
        &table(),
    );
    assert!((s.total_usd - 2.0).abs() < 1e-9, "{}", s.total_usd);
    assert_eq!((s.total_input, s.total_output), (500_000, 100_000));
    assert_eq!(s.by_workflow[0].workflow_id, "w1");
    assert_eq!(s.by_day.len(), 1);
    let r = report(
        &[],
        &chats,
        &names(),
        at("2026-11-13T09:00:00+09:00"),
        &table(),
    );
    assert_eq!(
        r.tracking_since.as_deref(),
        Some("2026-11-01T00:00:00.000Z")
    );
    assert_eq!((r.providers[1].runs, r.providers[1].calls), (0, 2));
}

/// 최종 리뷰 I-3: 워크플로우를 지워도 그 채팅의 토큰·예상 금액은 합계·보고서에 남는다(실행 기록의 비용과 같게).
#[test]
fn chat_usage_survives_workflow_delete() {
    use ieum_desktop_lib::store::{Store, StoredWorkflow};
    use ieum_local::builder::{ChatMessage, Role, UsageEntry};
    let t = tempfile::tempdir().unwrap();
    let store = Store::new(t.path().to_path_buf());
    let id = store
        .save_workflow(StoredWorkflow {
            id: String::new(),
            name: "초안".into(),
            enabled: false,
            workflow: serde_json::from_value(json!({"nodes": [], "edges": []})).unwrap(),
            updated_at: String::new(),
        })
        .unwrap()
        .id;
    store
        .update_chat(&id, |c| {
            c.messages.push(ChatMessage {
                role: Role::User,
                text: "비밀 대화".into(),
                at: "2026-11-12T01:00:00.000Z".into(),
            });
            for (at, u) in [
                (
                    "2026-11-12T01:00:00.000Z",
                    usage("anthropic", "claude-x", 1_000_000, 0),
                ),
                (
                    "2026-11-12T02:00:00.000Z",
                    usage("openai", "gpt-x", 500_000, 100_000),
                ),
            ] {
                c.usage.push(UsageEntry {
                    at: at.into(),
                    usage: u,
                });
            }
        })
        .unwrap();
    let totals = |store: &Store| {
        let chats = store.all_chat_usage().unwrap();
        let s = summarize(
            &[],
            &chats,
            at("2026-11-10T00:00:00+09:00"),
            at("2026-11-13T09:00:00+09:00"),
            &table(),
        );
        let r = report(
            &[],
            &chats,
            &HashMap::new(),
            at("2026-11-13T09:00:00+09:00"),
            &table(),
        );
        let r = serde_json::to_value(&r).unwrap();
        (serde_json::to_value(&s).unwrap(), r["overview"].clone())
    };
    let before = totals(&store);
    assert_eq!(before.1["calls"], 2);
    store.delete_workflow(&id).unwrap();
    assert_eq!(totals(&store), before);
    // 대화는 남지 않는다.
    assert!(!t.path().join("chats").join(format!("{id}.json")).exists());
    let kept = fs_text(t.path());
    assert!(!kept.contains("비밀 대화"), "{kept}");
}

/// 폴더 아래 모든 파일의 글.
fn fs_text(dir: &std::path::Path) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.push_str(&fs_text(&p));
        } else {
            out.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
        }
    }
    out
}
