//! 키별 월 한도 저장(`key-limits.json`). 모든 파일은 임시 폴더 안에서만 만든다.

use std::fs;

use chrono::{DateTime, Local, TimeZone};
use ieum_desktop_lib::key_limits::{LimitGate, after_use, level, money, month_spend, used_keys};
use ieum_desktop_lib::pricing;
use ieum_desktop_lib::store::{ChatUsage, KeyLimitsFile, Notified, Store, StoredRun};
use ieum_desktop_lib::usage::{range_start, summarize};
use ieum_local::ai::{AiUsage, KeyGate};
use serde_json::json;

const K: &str = "anthropic:3fa9c1d2";

fn setup() -> (tempfile::TempDir, Store) {
    let t = tempfile::tempdir().unwrap();
    let s = Store::new(t.path().to_path_buf());
    (t, s)
}

#[test]
fn limits_roundtrip() {
    let (t, s) = setup();
    s.set_key_limit(K, Some(10.0)).unwrap();
    assert_eq!(s.key_limits().limits[K], 10.0);
    let disk = fs::read_to_string(t.path().join("key-limits.json")).unwrap();
    assert!(disk.contains("\"limits\""));
}

#[test]
fn rejects_bad_amounts() {
    let (t, s) = setup();
    for v in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(s.set_key_limit(K, Some(v)).is_err(), "{v}");
    }
    assert!(!t.path().join("key-limits.json").exists());
}

#[test]
fn none_removes_limit() {
    let (_t, s) = setup();
    s.set_key_limit(K, Some(10.0)).unwrap();
    s.set_key_limit(K, None).unwrap();
    assert!(!s.key_limits().limits.contains_key(K));
}

#[test]
fn set_limit_clears_notified() {
    let (_t, s) = setup();
    let n = Notified {
        month: "2026-11".into(),
        level: 100,
    };
    s.set_notified(K, n.clone()).unwrap();
    assert_eq!(s.key_limits().notified[K], n);
    s.set_key_limit(K, Some(20.0)).unwrap();
    assert!(!s.key_limits().notified.contains_key(K));
}

#[test]
fn broken_file_starts_empty() {
    let (t, s) = setup();
    fs::write(t.path().join("key-limits.json"), "\"깨진\"").unwrap();
    assert_eq!(s.key_limits(), KeyLimitsFile::default());
    s.set_key_limit(K, Some(5.0)).unwrap();
    assert_eq!(s.key_limits().limits[K], 5.0);
}

#[test]
fn remove_key_limit_clears_both() {
    let (t, s) = setup();
    s.set_key_limit(K, Some(10.0)).unwrap();
    s.set_notified(
        K,
        Notified {
            month: "2026-11".into(),
            level: 80,
        },
    )
    .unwrap();
    s.remove_key_limit(K).unwrap();
    assert_eq!(s.key_limits(), KeyLimitsFile::default());
    // 지울 게 없으면 파일을 쓰지 않는다.
    let (t2, s2) = setup();
    s2.remove_key_limit(K).unwrap();
    assert!(!t2.path().join("key-limits.json").exists());
    drop(t);
}

// ── 이번 달 지출·게이트·알림 단계 ──

/// 가격표의 실제 모델 — 입력 100만 토큰 $2, 출력 100만 토큰 $10.
const MODEL: &str = "claude-sonnet-5-5";

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap()
}

/// 실행 환경 시각대와 무관하게 로컬 시각을 RFC3339로.
fn local(m: u32, d: u32, h: u32) -> String {
    Local
        .with_ymd_and_hms(2026, m, d, h, 0, 0)
        .unwrap()
        .to_rfc3339()
}

fn usage(source: &str, credential: Option<&str>, input: u64, output: u64) -> AiUsage {
    AiUsage {
        source: source.into(),
        model: Some(MODEL.into()),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: None,
        cache_write_tokens: None,
        credential: credential.map(Into::into),
    }
}

fn run(started: &str, usages: &[AiUsage]) -> StoredRun {
    let mut nodes = serde_json::Map::new();
    for (i, u) in usages.iter().enumerate() {
        nodes.insert(
            format!("a{i}"),
            json!({"status": "succeeded", "output": {"output": "x"}, "error": null, "usage": u}),
        );
    }
    serde_json::from_value(json!({
        "run": {"runId": uuid::Uuid::new_v4().to_string(), "status": "succeeded",
            "validationErrors": [], "rejectReason": null, "nodes": nodes},
        "workflowId": "w1", "startedAt": started, "finishedAt": started,
        "fileOps": [], "allowedRoots": [], "undo": null
    }))
    .unwrap()
}

fn chat(at: &str, u: AiUsage) -> ChatUsage {
    ChatUsage {
        workflow_id: "w1".into(),
        at: at.into(),
        usage: u,
    }
}

/// 이번 달 실행 하나로 `id` 키에 입력 토큰을 쌓는다(100만 토큰 = $2).
fn spend(s: &Store, id: Option<&str>, input: u64) {
    s.save_run(&run(&local(11, 10, 9), &[usage("anthropic", id, input, 0)]))
        .unwrap();
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn levels(n: &[ieum_desktop_lib::key_limits::KeyLimitNotice]) -> Vec<u8> {
    n.iter().map(|n| n.level).collect()
}

fn used(id: &str) -> Vec<(String, String)> {
    vec![("anthropic".into(), id.into())]
}

#[test]
fn spend_matches_dashboard() {
    let runs = vec![run(
        &local(11, 3, 10),
        &[
            usage("anthropic", Some("k1"), 100_000, 0),
            usage("cli:claude", Some("system"), 9, 9),
        ],
    )];
    let chats = vec![chat(
        &local(11, 17, 8),
        usage("anthropic", Some("k1"), 0, 10_000),
    )];
    let m = month_spend(&runs, &chats, now());
    // 입력 10만 × $2 + 출력 1만 × $10 = 0.2 + 0.1
    assert!(close(m["anthropic:k1"], 0.3), "{m:?}");
    let dash = summarize(
        &runs,
        &chats,
        range_start("month", now()).unwrap(),
        now(),
        pricing::table(),
    );
    let c = dash
        .by_credential
        .iter()
        .find(|c| c.source == "anthropic" && c.credential == "k1")
        .unwrap();
    assert_eq!(m["anthropic:k1"], c.usd.unwrap());
    assert!(m.keys().all(|k| !k.starts_with("cli:")), "{m:?}");
}

#[test]
fn legacy_usage_counts_for_default_key() {
    let runs = vec![run(
        &local(11, 5, 10),
        &[usage("anthropic", None, 500_000, 0)],
    )];
    let m = month_spend(&runs, &[], now());
    assert!(close(m["anthropic:default"], 1.0), "{m:?}");
}

#[test]
fn last_month_spend_excluded() {
    let runs = vec![
        run(
            &local(10, 31, 23),
            &[usage("anthropic", Some("k1"), 1_000_000, 0)],
        ),
        run(
            &local(11, 1, 0),
            &[usage("anthropic", Some("k1"), 100_000, 0)],
        ),
    ];
    let m = month_spend(&runs, &[], now());
    assert!(close(m["anthropic:k1"], 0.2), "{m:?}");
}

#[test]
fn level_steps() {
    assert_eq!(level(7.9, 10.0), 0);
    assert_eq!(level(8.0, 10.0), 80);
    assert_eq!(level(10.0, 10.0), 100);
    assert_eq!(level(12.0, 10.0), 100);
}

#[test]
fn money_format() {
    assert_eq!(money(10.0), "$10");
    assert_eq!(money(2.1), "$2.10");
    assert_eq!(money(0.4), "$0.40");
}

#[test]
fn after_use_notifies_80_then_100_once() {
    let (t, s) = setup();
    // 이름표는 credentials.json에서 읽는다.
    fs::write(
        t.path().join("credentials.json"),
        json!({"apiKeys": {"anthropic": [
            {"id": "k1", "label": "업무", "isDefault": true, "createdAt": "", "last4": "abcd"}
        ]}})
        .to_string(),
    )
    .unwrap();
    s.set_key_limit("anthropic:k1", Some(1.0)).unwrap();
    spend(&s, Some("k1"), 425_000); // $0.85
    let n = after_use(&s, &used("k1"), now());
    assert_eq!(levels(&n), [80]);
    assert_eq!(
        (n[0].source.as_str(), n[0].label.as_str()),
        ("anthropic", "업무")
    );
    assert!(close(n[0].spend, 0.85) && n[0].limit == 1.0, "{n:?}");
    assert!(after_use(&s, &used("k1"), now()).is_empty());
    spend(&s, Some("k1"), 175_000); // $1.20
    assert_eq!(levels(&after_use(&s, &used("k1"), now())), [100]);
    assert!(after_use(&s, &used("k1"), now()).is_empty());
}

#[test]
fn jump_straight_to_100() {
    let (_t, s) = setup();
    s.set_key_limit("anthropic:k1", Some(1.0)).unwrap();
    spend(&s, Some("k1"), 750_000); // $1.50
    assert_eq!(levels(&after_use(&s, &used("k1"), now())), [100]);
}

#[test]
fn new_month_notifies_again() {
    let (_t, s) = setup();
    s.set_key_limit("anthropic:k1", Some(1.0)).unwrap();
    s.set_notified(
        "anthropic:k1",
        Notified {
            month: "2026-10".into(),
            level: 100,
        },
    )
    .unwrap();
    spend(&s, Some("k1"), 600_000);
    assert_eq!(levels(&after_use(&s, &used("k1"), now())), [100]);
    assert_eq!(s.key_limits().notified["anthropic:k1"].month, "2026-11");
}

#[test]
fn remembers_after_restart() {
    let (t, s) = setup();
    s.set_key_limit("anthropic:k1", Some(1.0)).unwrap();
    spend(&s, Some("k1"), 425_000);
    assert_eq!(levels(&after_use(&s, &used("k1"), now())), [80]);
    let again = Store::new(t.path().to_path_buf());
    assert!(after_use(&again, &used("k1"), now()).is_empty());
}

fn gate(s: &Store) -> LimitGate {
    LimitGate {
        store: s.clone(),
        now,
    }
}

#[tokio::test]
async fn unlimited_key_reads_nothing() {
    let (t, s) = setup();
    // 기록 폴더 자리에 파일 — 읽으면 실패한다.
    fs::write(t.path().join("runs"), "").unwrap();
    assert!(s.all_runs().is_err());
    assert_eq!(gate(&s).check("anthropic", "k1").await, Ok(()));
    assert!(after_use(&s, &used("k1"), now()).is_empty());
    // 한도가 있으면 못 읽을 때 막는다.
    s.set_key_limit("anthropic:k1", Some(1.0)).unwrap();
    let e = gate(&s).check("anthropic", "k1").await.unwrap_err();
    assert_eq!(e, "이번 달 사용량을 읽지 못해 한도를 확인할 수 없어요.");
    assert!(after_use(&s, &used("k1"), now()).is_empty());
}

fn over_default_limit() -> (tempfile::TempDir, Store) {
    let (t, s) = setup();
    s.set_key_limit("anthropic:default", Some(1.0)).unwrap();
    spend(&s, None, 600_000); // $1.20, 옛 기록(id 없음)
    (t, s)
}

#[tokio::test]
async fn gate_blocks_over_limit() {
    let (_t, s) = over_default_limit();
    assert_eq!(
        gate(&s).check("anthropic", "default").await,
        Err("Claude 기본 키가 이번 달 한도($1)를 넘었어요.".into())
    );
    // 다른 키는 그대로.
    assert_eq!(gate(&s).check("openai", "default").await, Ok(()));
}

#[tokio::test]
async fn raised_limit_unblocks() {
    let (_t, s) = over_default_limit();
    s.set_key_limit("anthropic:default", Some(5.0)).unwrap();
    assert_eq!(gate(&s).check("anthropic", "default").await, Ok(()));
}

#[test]
fn used_keys_filters_cli_and_dedups() {
    let keys = used_keys([
        usage("anthropic", Some("k1"), 1, 1),
        usage("cli:claude", Some("system"), 1, 1),
        usage("anthropic", Some("k1"), 1, 1),
        usage("openai", None, 1, 1),
        usage("anthropic", None, 1, 1),
    ]);
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        keys,
        [
            pair("anthropic", "k1"),
            pair("openai", "default"),
            pair("anthropic", "default"),
        ]
    );
}
