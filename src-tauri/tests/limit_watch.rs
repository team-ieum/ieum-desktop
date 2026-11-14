//! 구독 한도 판정(순수 함수).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ieum_desktop_lib::credentials::{CliAccountMeta, CredentialsView};
use ieum_desktop_lib::limit_watch::{
    Candidate, Delivery, PendingSuggest, Seen, Suggestion, deliver_once, gating, limit_key, needs_others, next_delay,
    route_suggestion,
    suggest, tick,
};
use ieum_desktop_lib::notify::Lang;
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
    assert_eq!((k.company, k.from_id.as_str(), k.kind), ("claude", "me", "five_hour"));
    assert_eq!(k.reset_ms, chrono::DateTime::parse_from_rfc3339(RESET).unwrap().timestamp_millis());
    // 다른 계정 값과 무관하게 기본 계정만으로 같은 키를 만든다.
    assert_eq!(limit_key("claude", &def), Some(k));
    assert_eq!(limit_key("claude", &cand("me", plan("ok", &[("five_hour", 99.0)]))), None);
    let mut seen = Seen::default();
    assert!(seen.first_time("acct", &s));
    assert!(!seen.first_time("acct", &s));
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
    assert!(seen.first_time("acct", &s));
    let later = ieum_desktop_lib::limit_watch::Suggestion {
        resets_at: "2026-11-13T10:00:00+00:00".into(),
        ..s
    };
    assert!(seen.first_time("acct", &later));
}

#[test]
fn suggest_key_ignores_subsecond_jitter() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest("claude", &def, &[cand("b", plan("ok", &[("five_hour", 3.0)]))]).unwrap();
    let mut seen = Seen::default();
    assert!(seen.first_time("acct", &ieum_desktop_lib::limit_watch::Suggestion {
        resets_at: "2026-11-13T05:00:00.123+00:00".into(),
        ..s.clone()
    }));
    // 같은 창, 소수점 초·오프셋 표기만 다름 → 다시 권하지 않는다.
    assert!(!seen.first_time("acct", &ieum_desktop_lib::limit_watch::Suggestion {
        resets_at: "2026-11-13T05:00:00.456Z".into(),
        ..s
    }));
}

#[test]
fn suggest_key_same_window_within_60s() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest("claude", &def, &[cand("b", plan("ok", &[("five_hour", 3.0)]))]).unwrap();
    let at = |r: &str| Suggestion { resets_at: r.into(), ..s.clone() };
    let mut seen = Seen::default();
    assert!(seen.first_time("acct", &at("2026-11-13T04:59:59.990+00:00")));
    // 분 경계를 넘는 20ms 차이 — 같은 창.
    assert!(!seen.first_time("acct", &at("2026-11-13T05:00:00.010+00:00")));
    // 1시간 차이 — 다른 창.
    assert!(seen.first_time("acct", &at("2026-11-13T06:00:00+00:00")));
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

#[test]
fn route_suggestion_respects_toggle() {
    assert_eq!(route_suggestion(false, true), Delivery::Drop);
    assert_eq!(route_suggestion(false, false), Delivery::Drop);
    assert_eq!(route_suggestion(true, true), Delivery::Banner);
    assert_eq!(route_suggestion(true, false), Delivery::System);
}

fn meta(id: &str, is_default: bool) -> CliAccountMeta {
    CliAccountMeta {
        id: id.into(),
        label: format!("{id} 이름"),
        is_default,
        created_at: String::new(),
        email: Some(format!("{id}@x.com")),
    }
}

fn view(claude: Vec<CliAccountMeta>) -> CredentialsView {
    CredentialsView {
        api_keys: Default::default(),
        claude_accounts: claude,
        codex_accounts: vec![],
        keychain_error: None,
    }
}

#[tokio::test]
async fn tick_skips_fetch_when_signed_out() {
    let calls = AtomicUsize::new(0);
    let t = tick(None, Lang::Ko, |_| true, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Some(plan("ok", &[])) }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(t.plans.is_empty() && t.suggestions.is_empty());
    assert_eq!(t.delay, Duration::from_secs(300));
}

#[tokio::test]
async fn tick_fetches_others_only_when_default_full() {
    let fetched = std::sync::Mutex::new(Vec::new());
    let v = view(vec![meta("system", true), meta("b", false)]);
    let t = tick(Some(&v), Lang::Ko, |_| true, |kind, id: String| {
        fetched.lock().unwrap().push(format!("{kind}:{id}"));
        let used = if id == "system" { 100.0 } else { 12.4 };
        async move { Some(plan("ok", &[("five_hour", used)])) }
    })
    .await;
    assert_eq!(*fetched.lock().unwrap(), ["claude:system", "claude:b"]);
    assert_eq!(t.suggestions.len(), 1);
    let s = &t.suggestions[0];
    assert_eq!(
        (s.from_label.as_str(), s.to_id.as_str()),
        ("이 Mac의 로그인", "b")
    );
    assert!(t.plans.contains_key("claude:system") && t.plans.contains_key("claude:b"));
    assert_eq!(t.delay, Duration::from_secs(120));

    // 기본 계정이 여유 있으면 다른 계정은 조회하지 않는다.
    fetched.lock().unwrap().clear();
    let t = tick(Some(&v), Lang::En, |_| true, |kind, id: String| {
        fetched.lock().unwrap().push(format!("{kind}:{id}"));
        async { Some(plan("ok", &[("five_hour", 10.0)])) }
    })
    .await;
    assert_eq!(*fetched.lock().unwrap(), ["claude:system"]);
    assert!(t.suggestions.is_empty());
    assert_eq!(t.delay, Duration::from_secs(300));
}

#[test]
fn dropped_suggestion_not_remembered() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest(
        "claude",
        &def,
        &[cand("b", plan("ok", &[("five_hour", 3.0)]))],
    )
    .unwrap();
    let mut seen = Seen::default();
    let pending = PendingSuggest::default();
    // 알림 꺼짐 → 버리고 기억하지 않는다.
    assert_eq!(deliver_once(&mut seen, &pending, "acct", &s, false, false), Delivery::Drop);
    assert!(pending.take(Some("acct")).is_empty());
    // 켜면 같은 제안을 한 번 보내고, 그 뒤로는 다시 안 보낸다.
    assert_eq!(deliver_once(&mut seen, &pending, "acct", &s, true, false), Delivery::System);
    assert_eq!(deliver_once(&mut seen, &pending, "acct", &s, true, true), Delivery::Drop);
}

#[test]
fn banner_suggestion_also_pending() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest(
        "claude",
        &def,
        &[cand("b", plan("ok", &[("five_hour", 3.0)]))],
    )
    .unwrap();
    let mut seen = Seen::default();
    let pending = PendingSuggest::default();
    // 앱이 앞이어도 대기 칸에 넣는다 — 화면 리스너가 붙기 전에 emit돼도 꺼내 갈 수 있게.
    assert_eq!(deliver_once(&mut seen, &pending, "acct", &s, true, true), Delivery::Banner);
    assert_eq!(pending.take(Some("acct")), [s]);
}

#[tokio::test]
async fn tick_skips_others_when_already_seen_or_disabled() {
    let v = view(vec![meta("me", true), meta("b", false)]);
    let fetched = std::sync::Mutex::new(Vec::new());
    let fetch = |_, id: String| {
        fetched.lock().unwrap().push(id.clone());
        let used = if id == "me" { 100.0 } else { 3.0 };
        async move { Some(plan("ok", &[("five_hour", used)])) }
    };
    // 이미 보인 제안 키(기본 계정 plan만으로 만든 키) — 다른 계정을 묻지 않는다.
    let mut seen = Seen::default();
    let s = suggest("claude", &cand("me", plan("ok", &[("five_hour", 100.0)])), &[cand("x", plan("ok", &[]))]).unwrap();
    assert!(seen.first_time("acct", &s));
    let t = tick(Some(&v), Lang::Ko, |k| !seen.contains("acct", k), fetch).await;
    assert_eq!(*fetched.lock().unwrap(), ["me"]);
    assert!(t.suggestions.is_empty());
    // 알림이 꺼져 있어도 묻지 않는다.
    fetched.lock().unwrap().clear();
    let enabled = false;
    tick(Some(&v), Lang::Ko, |_| enabled, fetch).await;
    assert_eq!(*fetched.lock().unwrap(), ["me"]);
}

#[test]
fn pending_keeps_one_per_company() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let others = [cand("b", plan("ok", &[("five_hour", 3.0)]))];
    let c = suggest("claude", &def, &others).unwrap();
    let x = suggest("codex", &def, &others).unwrap();
    let pending = PendingSuggest::default();
    let mut seen = Seen::default();
    deliver_once(&mut seen, &pending, "acct", &c, true, false);
    deliver_once(&mut seen, &pending, "acct", &x, true, false);
    assert_eq!(pending.take(Some("acct")), [c, x]);
    assert!(pending.take(Some("acct")).is_empty());
}

#[test]
fn pending_dropped_after_account_change() {
    let def = cand("me", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest("claude", &def, &[cand("b", plan("ok", &[("five_hour", 3.0)]))]).unwrap();
    let pending = PendingSuggest::default();
    deliver_once(&mut Seen::default(), &pending, "old", &s, true, false);
    // 이음 계정이 바뀌었거나(다른 키) 로그아웃(None) — 버린다.
    assert!(pending.take(Some("new")).is_empty());
    deliver_once(&mut Seen::default(), &pending, "old", &s, true, false);
    assert!(pending.take(None).is_empty());
    assert!(pending.take(Some("old")).is_empty());
}

#[test]
fn seen_is_per_ieum_account() {
    let def = cand("system", plan("ok", &[("five_hour", 100.0)]));
    let s = suggest("claude", &def, &[cand("b", plan("ok", &[("five_hour", 3.0)]))]).unwrap();
    let mut seen = Seen::default();
    let pending = PendingSuggest::default();
    assert_eq!(deliver_once(&mut seen, &pending, "old", &s, true, true), Delivery::Banner);
    // 다른 이음 계정에선 같은 기본 계정(이 Mac의 로그인)·같은 창이어도 처음 보는 제안.
    assert!(!seen.contains("new", &s.key()));
    assert_eq!(deliver_once(&mut seen, &pending, "new", &s, true, true), Delivery::Banner);
    assert_eq!(deliver_once(&mut seen, &pending, "old", &s, true, true), Delivery::Drop);
}
