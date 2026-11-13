//! 구독 한도 판정: 기본 계정이 한도에 닿았는지, 어느 계정을 권할지, 다음 조회까지 얼마나 쉴지.

use std::collections::HashSet;
use std::time::Duration;

use serde::Serialize;

use crate::plan_usage::{PlanUsage, PlanWindow};

/// 한도 판정 대상 창. 모델 전용 창(`seven_day_opus`·`seven_day_model`)은 표시만 한다.
pub fn gating(kind: &str) -> bool {
    matches!(
        kind,
        "five_hour" | "seven_day" | "session" | "weekly" | "monthly"
    )
}

fn gating_windows(p: &PlanUsage) -> impl Iterator<Item = &PlanWindow> {
    p.windows.iter().filter(|w| gating(w.kind))
}

fn max_used(p: &PlanUsage) -> f64 {
    gating_windows(p).map(|w| w.used_pct).fold(0.0, f64::max)
}

/// 판정 대상 창이 하나라도 80% 이상인 기본 계정이 있으면 2분, 아니면 5분.
pub fn next_delay(defaults: &[&PlanUsage]) -> Duration {
    let hot = defaults.iter().any(|p| max_used(p) >= 80.0);
    Duration::from_secs(if hot { 120 } else { 300 })
}

/// 판정 대상 창 중 한도(100%)에 닿은 것이 있는가 — 다른 계정을 조회할지.
pub fn needs_others(default: &PlanUsage) -> bool {
    gating_windows(default).any(|w| w.used_pct >= 100.0)
}

pub struct Candidate {
    pub id: String,
    pub label: String,
    pub plan: PlanUsage,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// `claude` | `codex`
    pub company: &'static str,
    pub from_id: String,
    pub from_label: String,
    pub to_id: String,
    pub to_label: String,
    pub to_used_pct: f64,
    pub window_kind: &'static str,
    pub resets_at: String,
}

impl Suggestion {
    pub fn key(&self) -> (String, String, String, String) {
        (
            self.company.to_string(),
            self.from_id.clone(),
            self.window_kind.to_string(),
            // 분 단위로 — 서버가 조회마다 소수점 초를 다르게 줘도 같은 창으로 본다.
            reset_ms(&self.resets_at).map_or_else(|| self.resets_at.clone(), |ms| (ms / 60_000).to_string()),
        )
    }
}

/// RFC 3339 → 밀리초. 못 읽으면 None.
fn reset_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp_millis())
}

/// 기본 계정이 한도에 닿았고 여유 있는 다른 계정이 있으면, 판정 대상 최댓값이 가장 낮은 계정을 권한다.
pub fn suggest(
    company: &'static str,
    default: &Candidate,
    others: &[Candidate],
) -> Option<Suggestion> {
    if default.plan.state != "ok" {
        return None;
    }
    // 한도에 닿은 창이 여럿이면 가장 늦게 풀리는 것(오프셋이 달라도 시각으로 비교).
    let full = gating_windows(&default.plan)
        .filter(|w| w.used_pct >= 100.0)
        .max_by_key(|w| reset_ms(&w.resets_at))?;
    let (to, to_used_pct) = others
        .iter()
        .filter(|c| c.id != default.id)
        .filter(|c| c.plan.state == "ok" && gating_windows(&c.plan).all(|w| w.used_pct < 100.0))
        .map(|c| (c, max_used(&c.plan)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    Some(Suggestion {
        company,
        from_id: default.id.clone(),
        from_label: default.label.clone(),
        to_id: to.id.clone(),
        to_label: to.label.clone(),
        to_used_pct,
        window_kind: full.kind,
        resets_at: full.resets_at.clone(),
    })
}

/// 이미 보인(또는 "나중에"로 넘긴) 제안 키. 메모리에만 둔다.
#[derive(Default)]
pub struct Seen(HashSet<(String, String, String, String)>);

impl Seen {
    /// 처음 보는 제안이면 기억하고 true.
    pub fn first_time(&mut self, s: &Suggestion) -> bool {
        self.0.insert(s.key())
    }
}
