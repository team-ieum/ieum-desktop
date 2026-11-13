//! 구독 한도 판정: 기본 계정이 한도에 닿았는지, 어느 계정을 권할지, 다음 조회까지 얼마나 쉴지.
//! 그리고 그 판정을 돌리는 백그라운드 감시 루프.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::active::Active;
use crate::credentials::{CliAccountMeta, CredentialsView};
use crate::notify::{self, Lang};
use crate::plan_usage::{PlanUsage, PlanWindow};
use crate::sub_usage::{AntigravityUsageClient, SubscriptionUsage};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Banner,
    System,
    Drop,
}

/// 꺼짐 → 버림, 앱이 앞 → 앱 안 띠, 뒤 → 시스템 알림.
pub fn route_suggestion(enabled: bool, focused: bool) -> Delivery {
    match (enabled, focused) {
        (false, _) => Delivery::Drop,
        (true, true) => Delivery::Banner,
        (true, false) => Delivery::System,
    }
}

/// 시스템 알림으로 보낸 제안 — 알림을 눌러 화면이 `take_account_suggestion`으로 꺼낸다.
#[derive(Default)]
pub struct PendingSuggest(pub Mutex<Option<Suggestion>>);

/// 감시 한 회차의 결과.
pub struct Tick {
    /// `claude:<id>`·`codex:<id>` → 사용량(조회 못 한 계정은 빠진다).
    pub plans: BTreeMap<String, PlanUsage>,
    pub suggestions: Vec<Suggestion>,
    pub delay: Duration,
}

fn account_label(m: &CliAccountMeta, lang: Lang) -> String {
    match (m.id.as_str(), lang) {
        ("system", Lang::Ko) => "이 Mac의 로그인".into(),
        ("system", Lang::En) => "This Mac's login".into(),
        _ => m.label.clone(),
    }
}

/// 감시 한 회차. `view` = None(로그아웃)이면 조회하지 않는다. `fetch(회사, 계정 id)`는 조용한 조회.
pub async fn tick<F, Fut>(view: Option<&CredentialsView>, lang: Lang, fetch: F) -> Tick
where
    F: Fn(&'static str, String) -> Fut,
    Fut: Future<Output = Option<PlanUsage>>,
{
    let mut t = Tick {
        plans: BTreeMap::new(),
        suggestions: Vec::new(),
        delay: IDLE,
    };
    let Some(view) = view else { return t };
    let mut defaults = Vec::new();
    for (company, list) in [
        ("claude", &view.claude_accounts),
        ("codex", &view.codex_accounts),
    ] {
        let Some(d) = list.iter().find(|m| m.is_default) else {
            continue;
        };
        let Some(plan) = fetch(company, d.id.clone()).await else {
            continue;
        };
        let default = Candidate {
            id: d.id.clone(),
            label: account_label(d, lang),
            plan,
        };
        let mut others = Vec::new();
        if needs_others(&default.plan) {
            for m in list.iter().filter(|m| !m.is_default) {
                if let Some(plan) = fetch(company, m.id.clone()).await {
                    others.push(Candidate {
                        id: m.id.clone(),
                        label: account_label(m, lang),
                        plan,
                    });
                }
            }
        }
        t.suggestions.extend(suggest(company, &default, &others));
        for c in &others {
            t.plans
                .insert(format!("{company}:{}", c.id), c.plan.clone());
        }
        t.plans
            .insert(format!("{company}:{}", default.id), default.plan.clone());
        defaults.push(default.plan);
    }
    t.delay = next_delay(&defaults.iter().collect::<Vec<_>>());
    t
}

/// 로그아웃이거나 여유 있을 때 쉬는 시간.
const IDLE: Duration = Duration::from_secs(300);

/// 백그라운드 감시 루프. 키체인 허용 창은 띄우지 않는다(`get_quiet`).
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut seen = Seen::default();
        loop {
            let delay = run_once(&app, &mut seen).await;
            tokio::time::sleep(delay).await;
        }
    });
}

async fn run_once(app: &AppHandle, seen: &mut Seen) -> Duration {
    let Ok(creds) = app.state::<Active>().credentials().await else {
        return IDLE;
    };
    let creds = Arc::new(creds);
    let c = creds.clone();
    let view = tauri::async_runtime::spawn_blocking(move || c.list())
        .await
        .ok();
    let s = notify::settings(app);
    let lang: Lang = s.language.into();
    let usage = app.state::<SubscriptionUsage>();
    let usage = &*usage;
    let mut t = tick(view.as_ref(), lang, |kind, id| {
        let creds = creds.clone();
        async move {
            let dir = creds.cli_dir(kind, &id).ok()?;
            usage.get_quiet(kind, dir).await.ok().flatten()
        }
    })
    .await;
    if let Some(p) = app.state::<AntigravityUsageClient>().get_quiet().await {
        t.plans.insert("antigravity".into(), p);
    }
    if let Err(e) = app.emit("plans-updated", &t.plans) {
        eprintln!("[ieum] 이벤트를 보내지 못했어요: {e}");
    }
    for sug in t.suggestions {
        if !seen.first_time(&sug) {
            continue;
        }
        match route_suggestion(s.notify_account_suggest, notify::focused(app)) {
            Delivery::Banner => {
                if let Err(e) = app.emit("account-suggest", &sug) {
                    eprintln!("[ieum] 이벤트를 보내지 못했어요: {e}");
                }
            }
            Delivery::System => {
                let notice = notify::suggest_notice(&sug, lang);
                *app.state::<PendingSuggest>()
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(sug);
                notify::deliver(app, &notice);
            }
            Delivery::Drop => {}
        }
    }
    t.delay
}
