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
