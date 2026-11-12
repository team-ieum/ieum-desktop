//! AI 사용량 요약·예상 금액.

use chrono::{DateTime, FixedOffset};
use ieum_desktop_lib::pricing::{Pricing, estimate_usd};
use ieum_desktop_lib::store::StoredRun;
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
    }
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
        at("2026-11-01T00:00:00Z"),
        at("2026-12-01T00:00:00Z"),
        &table(),
    );
    assert_eq!((s.total_input, s.unpriced_calls), (0, 0));
    assert!(s.by_source.is_empty() && s.by_day.is_empty());
}
