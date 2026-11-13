//! 구독 한도 판정(순수 함수).

use std::time::Duration;

use ieum_desktop_lib::limit_watch::{Candidate, Seen, gating, needs_others, next_delay, suggest};
use ieum_desktop_lib::plan_usage::{PlanUsage, PlanWindow};

const RESET: &str = "2026-11-13T05:00:00+00:00";

fn plan(state: &'static str, windows: &[(&'static str, f64)]) -> PlanUsage {
    PlanUsage {
        state,
        windows: windows
            .iter()
            .map(|&(kind, used_pct)| PlanWindow {
                kind,
                used_pct,
                resets_at: RESET.into(),
                label: None,
            })
            .collect(),
        checked_at: RESET.into(),
        account_email: None,
    }
}

fn cand(id: &str, plan: PlanUsage) -> Candidate {
    Candidate {
        id: id.into(),
        label: format!("{id} 이름"),
        plan,
    }
}

#[test]
fn gating_excludes_model_windows() {
    assert!(!gating("seven_day_opus"));
    assert!(!gating("seven_day_model"));
    for k in ["five_hour", "seven_day", "session", "weekly", "monthly"] {
        assert!(gating(k), "{k}");
    }
}

#[test]
fn delay_is_2min_at_80() {
    let two = Duration::from_secs(120);
    let five = Duration::from_secs(300);
    assert_eq!(next_delay(&[&plan("ok", &[("five_hour", 80.0)])]), two);
    assert_eq!(next_delay(&[&plan("ok", &[("five_hour", 79.9)])]), five);
    assert_eq!(
        next_delay(&[&plan("ok", &[("seven_day_model", 95.0)])]),
        five
    );
    assert_eq!(next_delay(&[]), five);
}

#[test]
fn suggests_lowest_other() {
    let def = cand(
        "me",
        plan("ok", &[("five_hour", 100.0), ("seven_day", 50.0)]),
    );
    assert!(needs_others(&def.plan));
    let others = [
        cand("a", plan("ok", &[("seven_day", 40.0)])),
        cand("b", plan("ok", &[("five_hour", 12.0)])),
        cand("c", plan("ok", &[("five_hour", 100.0)])),
    ];
    let s = suggest("claude", &def, &others).expect("제안");
    assert_eq!(s.company, "claude");
    assert_eq!(s.from_id, "me");
    assert_eq!(s.from_label, "me 이름");
    assert_eq!(s.to_id, "b");
    assert_eq!(s.to_label, "b 이름");
    assert_eq!(s.to_used_pct, 12.0);
    assert_eq!(s.window_kind, "five_hour");
    assert_eq!(s.resets_at, RESET);
}

#[test]
fn suggest_picks_latest_reset_among_full_windows() {
    let mut p = plan("ok", &[("five_hour", 100.0), ("seven_day", 100.0)]);
    p.windows[1].resets_at = "2026-11-16T05:00:00+00:00".into();
    let s = suggest(
        "claude",
        &cand("me", p),
        &[cand("b", plan("ok", &[("five_hour", 1.0)]))],
    )
    .unwrap();
    assert_eq!(s.window_kind, "seven_day");
    assert_eq!(s.resets_at, "2026-11-16T05:00:00+00:00");
}

#[test]
fn no_suggest_when_only_model_window_full() {
    let def = cand(
        "me",
        plan("ok", &[("seven_day_model", 100.0), ("five_hour", 10.0)]),
    );
    assert!(!needs_others(&def.plan));
    let others = [cand("b", plan("ok", &[("five_hour", 1.0)]))];
    assert_eq!(suggest("claude", &def, &others), None);
}

#[test]
fn no_suggest_without_room() {
    let def = cand("me", plan("ok", &[("weekly", 100.0)]));
    let others = [
        cand("a", plan("ok", &[("session", 100.0)])),
        cand("b", plan("unavailable", &[])),
    ];
    assert_eq!(suggest("codex", &def, &others), None);
    let bad = cand("me", plan("unavailable", &[("weekly", 100.0)]));
    assert_eq!(suggest("codex", &bad, &[cand("c", plan("ok", &[]))]), None);
}

#[test]
fn suggest_not_repeated_for_same_key() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest(
        "claude",
        &def,
        &[cand("b", plan("ok", &[("five_hour", 3.0)]))],
    )
    .unwrap();
    let k = s.key();
    assert_eq!((k.0.as_str(), k.1.as_str(), k.2.as_str()), ("claude", "me", "five_hour"));
    // 초기화 시각은 분 단위(epoch 분).
    assert_eq!(k.3, (chrono::DateTime::parse_from_rfc3339(RESET).unwrap().timestamp() / 60).to_string());
    let mut seen = Seen::default();
    assert!(seen.first_time(&s));
    assert!(!seen.first_time(&s));
}

#[test]
fn suggest_again_after_reset() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest(
        "claude",
        &def,
        &[cand("b", plan("ok", &[("five_hour", 3.0)]))],
    )
    .unwrap();
    let mut seen = Seen::default();
    assert!(seen.first_time(&s));
    let later = ieum_desktop_lib::limit_watch::Suggestion {
        resets_at: "2026-11-13T10:00:00+00:00".into(),
        ..s
    };
    assert!(seen.first_time(&later));
}

#[test]
fn suggest_key_ignores_subsecond_jitter() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest("claude", &def, &[cand("b", plan("ok", &[("five_hour", 3.0)]))]).unwrap();
    let mut seen = Seen::default();
    assert!(seen.first_time(&ieum_desktop_lib::limit_watch::Suggestion {
        resets_at: "2026-11-13T05:00:00.123+00:00".into(),
        ..s.clone()
    }));
    // 같은 창, 소수점 초·오프셋 표기만 다름 → 다시 권하지 않는다.
    assert!(!seen.first_time(&ieum_desktop_lib::limit_watch::Suggestion {
        resets_at: "2026-11-13T05:00:00.456Z".into(),
        ..s
    }));
}

#[test]
fn suggest_skips_self_and_ignores_model_windows_of_others() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let others = [
        // 기본 계정 자신이 목록에 섞여 와도 권하지 않는다.
        cand("me", plan("ok", &[("five_hour", 0.0)])),
        // 모델 전용 창이 100이어도 후보, 최댓값에서도 뺀다.
        cand("b", plan("ok", &[("five_hour", 12.4), ("seven_day_model", 100.0)])),
    ];
    let s = suggest("claude", &def, &others).unwrap();
    assert_eq!(s.to_id, "b");
    assert_eq!(s.to_used_pct, 12.4);
    let v = serde_json::to_value(&s).unwrap();
    for k in ["company", "fromId", "fromLabel", "toId", "toLabel", "toUsedPct", "windowKind", "resetsAt"] {
        assert!(v.get(k).is_some(), "{k}");
    }
}
