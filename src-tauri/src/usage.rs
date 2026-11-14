//! 실행 기록의 AI 사용량(`NodeRecord.usage`) → 기간 요약. 화면 타입은 `src/api.ts` `UsageSummary`.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Days, Local, TimeZone};
use ieum_local::ai::{AiUsage, agy_split};
use serde::Serialize;

use crate::pricing::{Pricing, estimate_usd};
use crate::store::{ChatUsage, StoredRun};

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

fn run_usages(r: &StoredRun) -> Vec<AiUsage> {
    r.run
        .nodes
        .values()
        .filter_map(|n| serde_json::from_value(n.usage.clone()?).ok())
        .collect()
}

/// 집계 한 묶음: 시각, 워크플로우, 사용량들, 실행이면 그 번호와 기록(채팅 호출은 None — 실행 건수·시간에 안 든다).
type Item<'a> = (
    &'a str,
    &'a str,
    Vec<AiUsage>,
    Option<(usize, &'a StoredRun)>,
);

/// 실행 기록과 채팅 패널 호출을 같은 누적 코드에 태울 한 줄.
fn items<'a>(runs: &'a [StoredRun], chats: &'a [ChatUsage]) -> impl Iterator<Item = Item<'a>> {
    let runs = runs.iter().enumerate().map(|(i, r)| {
        (
            r.started_at.as_str(),
            r.workflow_id.as_str(),
            run_usages(r),
            Some((i, r)),
        )
    });
    let chats = chats.iter().map(|c| {
        (
            c.at.as_str(),
            c.workflow_id.as_str(),
            vec![c.usage.clone()],
            None,
        )
    });
    runs.chain(chats)
}

/// `from` ≤ 시작 시각 < `to`인 실행과 채팅 호출만. 날짜는 `from`의 시각대로 나눈다.
pub fn summarize<Tz: TimeZone>(
    runs: &[StoredRun],
    chats: &[ChatUsage],
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
    for (started, workflow_id, usages, _) in items(runs, chats) {
        let Ok(at) = DateTime::parse_from_rfc3339(started) else {
            continue;
        };
        if at < from || at >= to {
            continue;
        }
        let date = at.with_timezone(&tz).date_naive().to_string();
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
            wf.entry(workflow_id.to_string())
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayUse {
    pub date: String,
    pub tokens: u64,
    /// 구독 호출은 빠진 예상 금액.
    pub usd: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    /// `claude`·`openai`·`google`
    pub provider: &'static str,
    pub model: String,
    pub tokens: u64,
    pub calls: u64,
    pub usd: f64,
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
    pub usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total_tokens: u64,
    pub usd: f64,
    pub unpriced_calls: u64,
    pub calls: u64,
    /// 오늘 포함 30일, 오래된 날부터(빈 날 0).
    pub by_day: Vec<DayUse>,
    /// 토큰 많은 순.
    pub by_model: Vec<ModelUsage>,
    /// 토큰 많은 순.
    pub by_workflow: Vec<WorkflowTokens>,
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
    pub top_model: Option<String>,
    /// 오늘 포함 30일, 오래된 날부터(빈 날 0).
    pub by_day: Vec<DayUse>,
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
    /// 최근 30일, 세 회사 합.
    pub overview: Overview,
    /// 늘 claude·openai·google 순.
    pub providers: Vec<ProviderReport>,
}

const PROVIDERS: [&str; 3] = ["claude", "openai", "google"];
const DAYS: u64 = 30;
const RECENT: usize = 10;

fn provider_of(source: &str) -> Option<usize> {
    match source {
        "cli:claude" | "anthropic" => Some(0),
        "openai" | "cli:codex" => Some(1),
        "gemini" | "cli:agy" => Some(2),
        _ => None,
    }
}

/// 날·모델·워크플로우별 묶음 — 개요와 회사별이 같이 쓴다.
#[derive(Default)]
struct Groups {
    tokens: Tokens,
    usd: f64,
    unpriced: u64,
    calls: u64,
    runs: std::collections::BTreeSet<usize>,
    /// (토큰, 금액)
    day: BTreeMap<String, (u64, f64)>,
    /// (회사, 모델) → (토큰, 호출, 금액)
    model: BTreeMap<(usize, String), (u64, u64, f64)>,
    /// (토큰, 호출, 금액, 실행 번호들)
    wf: BTreeMap<String, (u64, u64, f64, std::collections::BTreeSet<usize>)>,
}

impl Groups {
    /// `run` = 실행 번호. 노드 테스트·채팅 호출은 None.
    fn add(
        &mut self,
        u: &AiUsage,
        p: usize,
        usd: Option<f64>,
        day: &str,
        run: Option<usize>,
        wf: &str,
    ) {
        let mut t = Tokens::default();
        t.add(u);
        let (n, cost) = (t.total(), usd.unwrap_or(0.0));
        self.tokens.add(u);
        self.usd += cost;
        self.unpriced += u64::from(usd.is_none());
        self.calls += 1;
        self.runs.extend(run);
        let d = self.day.entry(day.to_string()).or_default();
        d.0 += n;
        d.1 += cost;
        let mut model = u.model.clone().unwrap_or_default();
        // agy는 추론 수준이 id에 붙는다(`-high`) — 수준과 상관없이 같은 모델로 묶는다.
        if u.source == "cli:agy" {
            model = agy_split(&model).0.to_string();
        }
        let m = self.model.entry((p, model)).or_default();
        m.0 += n;
        m.1 += 1;
        m.2 += cost;
        let w = self.wf.entry(wf.to_string()).or_default();
        w.0 += n;
        w.1 += 1;
        w.2 += cost;
        w.3.extend(run);
    }

    /// `from`부터 30일, 빈 날은 0.
    fn by_day(&self, from: chrono::NaiveDate) -> Vec<DayUse> {
        (0..DAYS)
            .map(|d| {
                let date = (from + Days::new(d)).to_string();
                let (tokens, usd) = self.day.get(&date).copied().unwrap_or_default();
                DayUse { date, tokens, usd }
            })
            .collect()
    }

    fn by_model(&self) -> Vec<ModelUsage> {
        let mut v: Vec<ModelUsage> = self
            .model
            .iter()
            .map(|((p, model), &(tokens, calls, usd))| ModelUsage {
                provider: PROVIDERS[*p],
                model: model.clone(),
                tokens,
                calls,
                usd,
            })
            .collect();
        v.sort_by_key(|m| std::cmp::Reverse(m.tokens));
        v
    }

    fn by_workflow(
        &self,
        names: &std::collections::HashMap<String, String>,
    ) -> Vec<WorkflowTokens> {
        let mut v: Vec<WorkflowTokens> = self
            .wf
            .iter()
            .map(|(id, (tokens, calls, usd, runs))| WorkflowTokens {
                workflow_id: id.clone(),
                name: names.get(id).cloned(),
                tokens: *tokens,
                calls: *calls,
                runs: runs.len() as u64,
                usd: *usd,
            })
            .collect();
        v.sort_by_key(|w| std::cmp::Reverse(w.tokens));
        v
    }
}

/// 통계 및 사용량 화면 한 번에. 추적 시작만 전체 기간, 나머지는 `now`의 시각대로 오늘 포함 최근 30일.
/// `names` = 워크플로우 id → 이름.
pub fn report<Tz: TimeZone>(
    runs: &[StoredRun],
    chats: &[ChatUsage],
    names: &std::collections::HashMap<String, String>,
    now: DateTime<Tz>,
    table: &Pricing,
) -> UsageReport
where
    Tz::Offset: std::fmt::Display,
{
    let tz = now.timezone();
    let from = now.date_naive() - Days::new(DAYS - 1);
    let mut oldest: Option<(DateTime<chrono::FixedOffset>, &str)> = None;
    let mut all = Groups::default();
    let mut prov: [Groups; 3] = Default::default();
    let mut recent: [Vec<(DateTime<chrono::FixedOffset>, RecentCall)>; 3] = Default::default();

    for (started, workflow_id, usages, run) in items(runs, chats) {
        let Ok(at) = DateTime::parse_from_rfc3339(started) else {
            continue;
        };
        if oldest.is_none_or(|(o, _)| at < o) {
            oldest = Some((at, started));
        }
        let date = at.with_timezone(&tz).date_naive();
        if at > now || date < from {
            continue;
        }
        // 노드 테스트와 채팅 호출은 실행 수에서 뺀다. 토큰·금액은 실제로 쓴 것이라 아래에서 그대로 센다.
        let run = run
            .filter(|(_, r)| r.trigger != Some(crate::store::RunTrigger::Test))
            .map(|(i, _)| i);
        let key = date.to_string();
        for u in usages {
            let Some(p) = provider_of(&u.source) else {
                continue;
            };
            let usd = estimate_usd(&u, table);
            all.add(&u, p, usd, &key, run, workflow_id);
            prov[p].add(&u, p, usd, &key, run, workflow_id);
            let credential = u.credential.clone().unwrap_or_else(|| {
                if u.source.starts_with("cli:") {
                    "system"
                } else {
                    "default"
                }
                .into()
            });
            recent[p].push((
                at,
                RecentCall {
                    at: started.to_string(),
                    workflow_id: workflow_id.to_string(),
                    workflow_name: names.get(workflow_id).cloned(),
                    model: u.model,
                    credential,
                    input: u.input_tokens,
                    output: u.output_tokens,
                },
            ));
        }
    }

    let overview = Overview {
        total_tokens: all.tokens.total(),
        usd: all.usd,
        unpriced_calls: all.unpriced,
        calls: all.calls,
        by_day: all.by_day(from),
        by_model: all.by_model(),
        by_workflow: all.by_workflow(names),
    };

    let providers = prov
        .into_iter()
        .zip(recent)
        .zip(PROVIDERS)
        .map(|((a, mut recent), provider)| {
            let by_model = a.by_model();
            // 같은 실행 안의 호출은 노드 순서 그대로(안정 정렬).
            recent.sort_by_key(|c| std::cmp::Reverse(c.0));
            ProviderReport {
                provider,
                usd: a.usd,
                unpriced_calls: a.unpriced,
                calls: a.calls,
                runs: a.runs.len() as u64,
                top_model: by_model.first().map(|m| m.model.clone()),
                by_day: a.by_day(from),
                by_workflow: a.by_workflow(names),
                by_model,
                tokens: a.tokens,
                recent: recent.into_iter().take(RECENT).map(|(_, c)| c).collect(),
            }
        })
        .collect();

    UsageReport {
        updated_at: now.to_rfc3339(),
        tracking_since: oldest.map(|(_, s)| s.to_string()),
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
