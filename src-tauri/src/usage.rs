//! 실행 기록의 AI 사용량(`NodeRecord.usage`) → 기간 요약. 화면 타입은 `src/api.ts` `UsageSummary`.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Days, Local, TimeZone};
use ieum_local::ai::AiUsage;
use serde::Serialize;

use crate::pricing::{Pricing, estimate_usd};
use crate::store::StoredRun;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceUsage {
    pub source: String,
    pub input: u64,
    pub output: u64,
    /// 그 묶음에 값을 매긴 호출이 하나도 없으면 None.
    pub usd: Option<f64>,
}

/// 공급원 + 키·계정 id 묶음.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialUsage {
    pub source: String,
    pub credential: String,
    pub input: u64,
    pub output: u64,
    pub usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowUsage {
    pub workflow_id: String,
    pub input: u64,
    pub output: u64,
    pub usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    /// 범위 시각대 기준 YYYY-MM-DD.
    pub date: String,
    pub usd: f64,
    pub input: u64,
    pub output: u64,
}

/// 입력 토큰은 캐시 읽기·쓰기까지 포함한 전체 입력.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_usd: f64,
    pub total_input: u64,
    pub total_output: u64,
    /// 값을 못 매긴 호출 수(구독 CLI·표에 없는 모델).
    pub unpriced_calls: u64,
    pub by_source: Vec<SourceUsage>,
    /// 옛 기록(키·계정 id 없음)은 API면 `default`, 구독이면 `system`.
    pub by_credential: Vec<CredentialUsage>,
    /// 예상 금액 큰 순.
    pub by_workflow: Vec<WorkflowUsage>,
    /// 날짜 순, 사용량 있는 날만.
    pub by_day: Vec<DayUsage>,
    pub as_of: String,
}

#[derive(Default)]
struct Acc {
    input: u64,
    output: u64,
    usd: Option<f64>,
}

impl Acc {
    fn add(&mut self, input: u64, output: u64, usd: Option<f64>) {
        self.input += input;
        self.output += output;
        if let Some(x) = usd {
            *self.usd.get_or_insert(0.0) += x;
        }
    }
}

/// `from` ≤ 시작 시각 < `to`인 실행만. 날짜는 `from`의 시각대로 나눈다.
pub fn summarize<Tz: TimeZone>(
    runs: &[StoredRun],
    from: DateTime<Tz>,
    to: DateTime<Tz>,
    table: &Pricing,
) -> UsageSummary {
    let tz = from.timezone();
    let mut s = UsageSummary {
        as_of: table.as_of.clone(),
        ..Default::default()
    };
    let (mut src, mut cred, mut wf, mut day) = (
        BTreeMap::<String, Acc>::new(),
        BTreeMap::<(String, String), Acc>::new(),
        BTreeMap::<String, Acc>::new(),
        BTreeMap::<String, Acc>::new(),
    );
    for r in runs {
        let Ok(at) = DateTime::parse_from_rfc3339(&r.started_at) else {
            continue;
        };
        if at < from || at >= to {
            continue;
        }
        let date = at.with_timezone(&tz).date_naive().to_string();
        let usages = r
            .run
            .nodes
            .values()
            .filter_map(|n| serde_json::from_value::<AiUsage>(n.usage.clone()?).ok());
        for u in usages {
            let usd = estimate_usd(&u, table);
            let input = u.input_tokens
                + u.cache_read_tokens.unwrap_or(0)
                + u.cache_write_tokens.unwrap_or(0);
            let output = u.output_tokens;
            s.unpriced_calls += u64::from(usd.is_none());
            s.total_usd += usd.unwrap_or(0.0);
            s.total_input += input;
            s.total_output += output;
            let id = u.credential.unwrap_or_else(|| {
                let legacy = if u.source.starts_with("cli:") {
                    "system"
                } else {
                    "default"
                };
                legacy.into()
            });
            cred.entry((u.source.clone(), id))
                .or_default()
                .add(input, output, usd);
            src.entry(u.source).or_default().add(input, output, usd);
            wf.entry(r.workflow_id.clone())
                .or_default()
                .add(input, output, usd);
            day.entry(date.clone()).or_default().add(input, output, usd);
        }
    }
    s.by_source = src
        .into_iter()
        .map(|(source, a)| SourceUsage {
            source,
            input: a.input,
            output: a.output,
            usd: a.usd,
        })
        .collect();
    s.by_credential = cred
        .into_iter()
        .map(|((source, credential), a)| CredentialUsage {
            source,
            credential,
            input: a.input,
            output: a.output,
            usd: a.usd,
        })
        .collect();
    s.by_workflow = wf
        .into_iter()
        .map(|(workflow_id, a)| WorkflowUsage {
            workflow_id,
            input: a.input,
            output: a.output,
            usd: a.usd,
        })
        .collect();
    s.by_workflow.sort_by(|a, b| {
        let usd = |w: &WorkflowUsage| w.usd.unwrap_or(0.0);
        usd(b)
            .total_cmp(&usd(a))
            .then((b.input + b.output).cmp(&(a.input + a.output)))
    });
    s.by_day = day
        .into_iter()
        .map(|(date, a)| DayUsage {
            date,
            usd: a.usd.unwrap_or(0.0),
            input: a.input,
            output: a.output,
        })
        .collect();
    s
}

// ── 통계 및 사용량 화면(`usage_report`). 화면 타입은 `src/api.ts` `UsageReport`. ──

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub runs: u64,
    pub run_seconds: u64,
    pub ai_calls: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    fn add(&mut self, u: &AiUsage) {
        self.input += u.input_tokens;
        self.output += u.output_tokens;
        self.cache_read += u.cache_read_tokens.unwrap_or(0);
        self.cache_write += u.cache_write_tokens.unwrap_or(0);
    }
    fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
    /// 캐시 읽기 ÷ (입력 + 캐시 읽기). 없으면 0.
    fn cache_share(&self) -> f64 {
        ratio(self.cache_read, self.input + self.cache_read)
    }
}

fn ratio(a: u64, b: u64) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatDay {
    pub date: String,
    pub tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total_tokens: u64,
    pub usd: f64,
    pub unpriced_calls: u64,
    pub active_days: u64,
    pub cache_share: f64,
    /// 최근 42일, 오래된 날부터(빈 날 0).
    pub heat: Vec<HeatDay>,
    pub composition: Tokens,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDay {
    pub date: String,
    #[serde(flatten)]
    pub tokens: Tokens,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub tokens: u64,
    pub calls: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTokens {
    pub workflow_id: String,
    /// 지운 워크플로우면 None.
    pub name: Option<String>,
    pub tokens: u64,
    pub calls: u64,
    pub runs: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentCall {
    /// 실행 시작 시각(RFC 3339).
    pub at: String,
    pub workflow_id: String,
    pub workflow_name: Option<String>,
    pub model: Option<String>,
    /// 키·계정 id. 옛 기록은 API `default`, 구독 `system`.
    pub credential: String,
    pub input: u64,
    pub output: u64,
    /// 캐시 읽기.
    pub cache: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderReport {
    /// `claude`·`openai`·`google`
    pub provider: &'static str,
    #[serde(flatten)]
    pub tokens: Tokens,
    pub usd: f64,
    pub unpriced_calls: u64,
    pub calls: u64,
    pub runs: u64,
    pub cache_reuse: f64,
    /// 캐시 읽기가 0인 호출 비율.
    pub no_cache_call_share: f64,
    pub top_model: Option<String>,
    /// 사용 있는 날만, 날짜 순.
    pub by_day: Vec<TokenDay>,
    /// 토큰 많은 순.
    pub by_model: Vec<ModelUsage>,
    /// 토큰 많은 순.
    pub by_workflow: Vec<WorkflowTokens>,
    /// 최근 10건, 최신부터.
    pub recent: Vec<RecentCall>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub updated_at: String,
    /// 가장 오래된 실행의 시작 시각. 기록이 없으면 None.
    pub tracking_since: Option<String>,
    /// 전체 기간.
    pub totals: Totals,
    /// 최근 30일.
    pub overview: Overview,
    /// 늘 claude·openai·google 순.
    pub providers: Vec<ProviderReport>,
}

const PROVIDERS: [&str; 3] = ["claude", "openai", "google"];
const DAYS: u64 = 30;
const HEAT_DAYS: u64 = 42;
const RECENT: usize = 10;

fn provider_of(source: &str) -> Option<usize> {
    match source {
        "cli:claude" | "anthropic" => Some(0),
        "openai" | "cli:codex" => Some(1),
        "gemini" | "cli:agy" => Some(2),
        _ => None,
    }
}

#[derive(Default)]
struct ProvAcc {
    tokens: Tokens,
    usd: f64,
    unpriced: u64,
    calls: u64,
    no_cache_calls: u64,
    runs: std::collections::BTreeSet<usize>,
    day: BTreeMap<String, Tokens>,
    model: BTreeMap<String, (u64, u64)>,
    /// (토큰, 호출, 실행 번호들)
    wf: BTreeMap<String, (u64, u64, std::collections::BTreeSet<usize>)>,
    recent: Vec<(DateTime<chrono::FixedOffset>, RecentCall)>,
}

/// 통계 및 사용량 화면 한 번에. 합계는 전체 기간, 나머지는 `now`의 시각대로 오늘 포함 최근 30일
/// (히트맵은 42일). `names` = 워크플로우 id → 이름.
pub fn report<Tz: TimeZone>(
    runs: &[StoredRun],
    names: &std::collections::HashMap<String, String>,
    now: DateTime<Tz>,
    table: &Pricing,
) -> UsageReport
where
    Tz::Offset: std::fmt::Display,
{
    let tz = now.timezone();
    let today = now.date_naive();
    let from = today - Days::new(DAYS - 1);
    let heat_from = today - Days::new(HEAT_DAYS - 1);
    let mut totals = Totals::default();
    let mut oldest: Option<(DateTime<chrono::FixedOffset>, &str)> = None;
    let mut overview = Overview::default();
    let mut heat = BTreeMap::<String, u64>::new();
    let mut active = std::collections::BTreeSet::<String>::new();
    let mut prov: [ProvAcc; 3] = Default::default();

    for (i, r) in runs.iter().enumerate() {
        let Ok(at) = DateTime::parse_from_rfc3339(&r.started_at) else {
            continue;
        };
        totals.runs += 1;
        if let Ok(end) = DateTime::parse_from_rfc3339(&r.finished_at) {
            totals.run_seconds += u64::try_from((end - at).num_seconds()).unwrap_or(0);
        }
        if oldest.is_none_or(|(o, _)| at < o) {
            oldest = Some((at, &r.started_at));
        }
        let usages: Vec<AiUsage> = r
            .run
            .nodes
            .values()
            .filter_map(|n| serde_json::from_value(n.usage.clone()?).ok())
            .collect();
        totals.ai_calls += usages.len() as u64;
        let date = at.with_timezone(&tz).date_naive();
        if at > now || date < heat_from {
            continue;
        }
        let key = date.to_string();
        for u in usages {
            let Some(p) = provider_of(&u.source) else {
                continue;
            };
            let mut t = Tokens::default();
            t.add(&u);
            *heat.entry(key.clone()).or_default() += t.total();
            if date < from {
                continue;
            }
            active.insert(key.clone());
            let usd = estimate_usd(&u, table);
            overview.usd += usd.unwrap_or(0.0);
            overview.unpriced_calls += u64::from(usd.is_none());
            overview.composition.add(&u);
            let a = &mut prov[p];
            a.tokens.add(&u);
            a.usd += usd.unwrap_or(0.0);
            a.unpriced += u64::from(usd.is_none());
            a.calls += 1;
            a.no_cache_calls += u64::from(u.cache_read_tokens.unwrap_or(0) == 0);
            a.runs.insert(i);
            a.day.entry(key.clone()).or_default().add(&u);
            let model = u.model.clone().unwrap_or_default();
            let m = a.model.entry(model).or_default();
            m.0 += t.total();
            m.1 += 1;
            let w = a.wf.entry(r.workflow_id.clone()).or_default();
            w.0 += t.total();
            w.1 += 1;
            w.2.insert(i);
            let credential = u.credential.clone().unwrap_or_else(|| {
                if u.source.starts_with("cli:") {
                    "system"
                } else {
                    "default"
                }
                .into()
            });
            a.recent.push((
                at,
                RecentCall {
                    at: r.started_at.clone(),
                    workflow_id: r.workflow_id.clone(),
                    workflow_name: names.get(&r.workflow_id).cloned(),
                    model: u.model,
                    credential,
                    input: t.input,
                    output: t.output,
                    cache: t.cache_read,
                },
            ));
        }
    }

    overview.total_tokens = overview.composition.total();
    overview.cache_share = overview.composition.cache_share();
    overview.active_days = active.len() as u64;
    overview.heat = (0..HEAT_DAYS)
        .map(|d| {
            let date = (heat_from + Days::new(d)).to_string();
            let tokens = heat.get(&date).copied().unwrap_or(0);
            HeatDay { date, tokens }
        })
        .collect();

    let providers = prov
        .into_iter()
        .zip(PROVIDERS)
        .map(|(mut a, provider)| {
            let mut by_model: Vec<ModelUsage> = a
                .model
                .into_iter()
                .map(|(model, (tokens, calls))| ModelUsage {
                    model,
                    tokens,
                    calls,
                })
                .collect();
            by_model.sort_by_key(|m| std::cmp::Reverse(m.tokens));
            let mut by_workflow: Vec<WorkflowTokens> =
                a.wf.into_iter()
                    .map(|(workflow_id, (tokens, calls, runs))| WorkflowTokens {
                        name: names.get(&workflow_id).cloned(),
                        workflow_id,
                        tokens,
                        calls,
                        runs: runs.len() as u64,
                    })
                    .collect();
            by_workflow.sort_by_key(|w| std::cmp::Reverse(w.tokens));
            // 같은 실행 안의 호출은 노드 순서 그대로(안정 정렬).
            a.recent.sort_by_key(|c| std::cmp::Reverse(c.0));
            ProviderReport {
                provider,
                cache_reuse: a.tokens.cache_share(),
                no_cache_call_share: ratio(a.no_cache_calls, a.calls),
                tokens: a.tokens,
                usd: a.usd,
                unpriced_calls: a.unpriced,
                calls: a.calls,
                runs: a.runs.len() as u64,
                top_model: by_model.first().map(|m| m.model.clone()),
                by_day: a
                    .day
                    .into_iter()
                    .map(|(date, tokens)| TokenDay { date, tokens })
                    .collect(),
                by_model,
                by_workflow,
                recent: a.recent.into_iter().take(RECENT).map(|(_, c)| c).collect(),
            }
        })
        .collect();

    UsageReport {
        updated_at: now.to_rfc3339(),
        tracking_since: oldest.map(|(_, s)| s.to_string()),
        totals,
        overview,
        providers,
    }
}

/// `today`·`week`(월요일부터)·`month`(1일부터)의 로컬 시작 시각.
pub fn range_start(range: &str, now: DateTime<Local>) -> Result<DateTime<Local>, String> {
    let today = now.date_naive();
    let day = match range {
        "today" => today,
        "week" => today - Days::new(today.weekday().num_days_from_monday().into()),
        "month" => today.with_day(1).unwrap_or(today),
        _ => return Err(format!("알 수 없는 기간이에요: {range:?}")),
    };
    // 자정이 없는 날(DST)이면 지금부터 — 빈 요약이 된다.
    Ok(day
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(Local).earliest())
        .unwrap_or(now))
}
