//! 구독 한도 판정: 기본 계정이 한도에 닿았는지, 어느 계정을 권할지, 다음 조회까지 얼마나 쉴지.
//! 그리고 그 판정을 돌리는 백그라운드 감시 루프.

use std::collections::{BTreeMap, HashMap};
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

/// 제안 키 — 기본 계정 쪽 값만(누구를 권하는지와 무관).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitKey {
    pub company: &'static str,
    pub from_id: String,
    pub kind: &'static str,
    /// 초기화 시각(ms). 못 읽으면 0.
    pub reset_ms: i64,
}

impl Suggestion {
    pub fn key(&self) -> LimitKey {
        LimitKey {
            company: self.company,
            from_id: self.from_id.clone(),
            kind: self.window_kind,
            reset_ms: reset_ms(&self.resets_at).unwrap_or(0),
        }
    }
}

/// RFC 3339 → 밀리초. 못 읽으면 None.
fn reset_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// 한도에 닿은 판정 대상 창 — 여럿이면 가장 늦게 풀리는 것(오프셋이 달라도 시각으로 비교).
fn full_window(p: &PlanUsage) -> Option<&PlanWindow> {
    if p.state != "ok" {
        return None;
    }
    gating_windows(p)
        .filter(|w| w.used_pct >= 100.0)
        .max_by_key(|w| reset_ms(&w.resets_at))
}

/// 기본 계정만으로 만든 제안 키. 한도에 닿은 창이 없으면 None.
pub fn limit_key(company: &'static str, default: &Candidate) -> Option<LimitKey> {
    let w = full_window(&default.plan)?;
    Some(LimitKey {
        company,
        from_id: default.id.clone(),
        kind: w.kind,
        reset_ms: reset_ms(&w.resets_at).unwrap_or(0),
    })
}

/// 기본 계정이 한도에 닿았고 여유 있는 다른 계정이 있으면, 판정 대상 최댓값이 가장 낮은 계정을 권한다.
pub fn suggest(
    company: &'static str,
    default: &Candidate,
    others: &[Candidate],
) -> Option<Suggestion> {
    let full = full_window(&default.plan)?;
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

/// 이미 보인(또는 "나중에"로 넘긴) 제안: (이음 계정 키, 회사, 기본 계정, 창) → 마지막 초기화 시각(ms).
/// 이음 계정마다 따로 기억한다(`system` 같은 계정 id는 이음 계정이 달라도 같다). 메모리에만 둔다.
#[derive(Default)]
pub struct Seen(HashMap<(String, &'static str, String, &'static str), i64>);

/// 서버가 조회마다 초기화 시각을 조금씩 다르게 줘도(분 경계를 넘어도) 이 안이면 같은 창.
const SAME_WINDOW_MS: i64 = 60_000;

impl Seen {
    pub fn contains(&self, account: &str, k: &LimitKey) -> bool {
        self.0
            .get(&(account.to_string(), k.company, k.from_id.clone(), k.kind))
            .is_some_and(|&r| (r - k.reset_ms).abs() <= SAME_WINDOW_MS)
    }

    /// 처음 보는 제안이면 기억하고 true.
    pub fn first_time(&mut self, account: &str, s: &Suggestion) -> bool {
        let k = s.key();
        if self.contains(account, &k) {
            return false;
        }
        self.0.insert(
            (account.to_string(), k.company, k.from_id, k.kind),
            k.reset_ms,
        );
        true
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

/// 보낸 제안(띠·시스템 알림 둘 다) — 화면이 `take_account_suggestion`으로 꺼낸다.
/// 띠 이벤트가 화면 리스너보다 먼저 와도, 마운트·포커스·알림 클릭 때 여기서 다시 가져간다.
/// 회사별로 하나씩, 그때의 이음 계정 키와 함께 둔다.
#[derive(Default)]
pub struct PendingSuggest(Mutex<BTreeMap<&'static str, (String, Suggestion)>>);

impl PendingSuggest {
    pub fn put(&self, account: &str, s: Suggestion) {
        self.lock().insert(s.company, (account.to_string(), s));
    }

    /// 전부 꺼내고 비운다. `account`(지금 이음 계정 키)와 다른 계정 때 쌓인 것은 버린다.
    pub fn take(&self, account: Option<&str>) -> Vec<Suggestion> {
        std::mem::take(&mut *self.lock())
            .into_values()
            .filter(|(a, _)| Some(a.as_str()) == account)
            .map(|(_, s)| s)
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<&'static str, (String, Suggestion)>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

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
/// 기본 계정이 한도에 닿아도 `want(제안 키)`가 false면(이미 보임·알림 꺼짐) 다른 계정은 묻지 않는다.
pub async fn tick<W, F, Fut>(view: Option<&CredentialsView>, lang: Lang, want: W, fetch: F) -> Tick
where
    W: Fn(&LimitKey) -> bool,
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
        if limit_key(company, &default).is_some_and(|k| want(&k)) {
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
        let seen = Arc::new(Mutex::new(Seen::default()));
        loop {
            // 회차마다 따로 돌려 패닉이 나도 루프는 산다.
            let once = tauri::async_runtime::spawn(run_once(app.clone(), seen.clone()));
            let delay = once.await.unwrap_or_else(|e| {
                eprintln!("[ieum] 구독 한도 감시 회차가 실패했어요: {e}");
                IDLE
            });
            tokio::time::sleep(delay).await;
        }
    });
}

/// 처음 보는 제안이면 갈 곳을 정하고, 실제로 보낼 때만(`Drop` 아님) 기억한다
/// — 알림을 끈 동안 버린 제안은 켠 뒤 다시 권한다. 보내는 제안은 대기 칸에도 넣는다.
pub fn deliver_once(
    seen: &mut Seen,
    pending: &PendingSuggest,
    account: &str,
    s: &Suggestion,
    enabled: bool,
    focused: bool,
) -> Delivery {
    if seen.contains(account, &s.key()) {
        return Delivery::Drop;
    }
    let d = route_suggestion(enabled, focused);
    if d != Delivery::Drop {
        seen.first_time(account, s);
        pending.put(account, s.clone());
    }
    d
}

async fn run_once(app: AppHandle, seen: Arc<Mutex<Seen>>) -> Duration {
    let app = &app;
    let active = app.state::<Active>();
    let Some(account) = active.account_key().await else {
        return IDLE;
    };
    let Ok(creds) = active.credentials().await else {
        return IDLE;
    };
    let creds = Arc::new(creds);
    let c = creds.clone();
    // 파일만 읽는다 — `list`는 옛 API 키를 찾느라 키체인을 읽어 허용 창이 뜰 수 있다.
    let view = tauri::async_runtime::spawn_blocking(move || c.subscription_accounts())
        .await
        .ok()
        .and_then(Result::ok);
    let s = notify::settings(app);
    let lang: Lang = s.language.into();
    let usage = app.state::<SubscriptionUsage>();
    let usage = &*usage;
    let want = |k: &LimitKey| {
        s.notify_account_suggest
            && !seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&account, k)
    };
    let mut t = tick(view.as_ref(), lang, want, |kind, id| {
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
        let focused = notify::focused(app);
        let d = deliver_once(
            &mut seen.lock().unwrap_or_else(|e| e.into_inner()),
            &app.state::<PendingSuggest>(),
            &account,
            &sug,
            s.notify_account_suggest,
            focused,
        );
        match d {
            Delivery::Banner => {
                if let Err(e) = app.emit("account-suggest", &sug) {
                    eprintln!("[ieum] 이벤트를 보내지 못했어요: {e}");
                }
            }
            Delivery::System => {
                notify::deliver(app, &notify::suggest_notice(&sug, lang));
            }
            Delivery::Drop => {}
        }
    }
    t.delay
}
