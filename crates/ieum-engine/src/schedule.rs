//! 스케줄 트리거 계산 — 이음 클라우드(BE `ScheduleCron`·`CronConverter`·`ScheduleTriggerHandler`)와 같은 규칙.
//! 모든 시각은 Asia/Seoul 기준이며 실행 중인 Mac의 로컬 시간대를 읽지 않는다.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use croner::Cron;
use croner::parser::{CronParser, Seconds, Year};
use serde_json::{Map, Value};

pub const SCHEDULE_ZONE: Tz = chrono_tz::Asia::Seoul;
pub const GRACE: Duration = Duration::hours(2);

/// 사용자에게 보일 한국어 이유.
#[derive(Debug, PartialEq)]
pub struct CronError(pub String);

const WEEKDAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

/// BE `ScheduleCron.of`: 칸 event는 Quartz 6필드로, 그 밖(`schedule.every`)은 `config.cron` 원문.
pub fn schedule_cron(event: &str, config: &Map<String, Value>) -> Result<String, CronError> {
    match event {
        "schedule.hourly" => Ok(format!("0 0 * {}", days(config))),
        "schedule.daily" => Ok(format!("0 0 {} {}", hour(config, "매일")?, days(config))),
        "schedule.weekly" => {
            let weekday = field(config, "weekday", "매주", "요일을", |s| {
                WEEKDAYS.contains(&s)
            })?;
            Ok(format!("0 0 {} ? * {weekday}", hour(config, "매주")?))
        }
        "schedule.monthly" => {
            let day = field(config, "day", "매월", "날짜를", |s| {
                s == "L" || in_range(s, 1, 28)
            })?;
            Ok(format!("0 0 {} {day} * ?", hour(config, "매월")?))
        }
        _ => match config.get("cron") {
            Some(Value::String(cron)) if !cron.trim().is_empty() => Ok(cron.clone()),
            _ => Err(CronError("cron 표현식이 비어 있습니다.".into())),
        },
    }
}

/// weekends가 명시적 false(불리언·문자열)일 때만 평일 한정.
fn days(config: &Map<String, Value>) -> &'static str {
    match config.get("weekends") {
        Some(Value::Bool(false)) => "? * MON-FRI",
        Some(Value::String(s)) if s == "false" => "? * MON-FRI",
        _ => "* * ?",
    }
}

fn hour(config: &Map<String, Value>, title: &str) -> Result<String, CronError> {
    field(config, "hour", title, "시각을", |s| in_range(s, 0, 23))
}

/// BE `String.valueOf` + 정규식과 같은 뜻: 숫자는 문자열로 보고, 앞자리 0 없는 십진수만.
fn in_range(s: &str, lo: u32, hi: u32) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s.len() == 1 || !s.starts_with('0'))
        && s.parse::<u32>().is_ok_and(|n| (lo..=hi).contains(&n))
}

fn field(
    config: &Map<String, Value>,
    key: &str,
    title: &str,
    phrase: &str,
    allowed: impl Fn(&str) -> bool,
) -> Result<String, CronError> {
    let text = match config.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    if allowed(&text) {
        Ok(text)
    } else {
        Err(CronError(format!("{title} 노드의 {phrase} 골라 주세요")))
    }
}

/// BE `CronConverter.toQuartz`: 5필드(Unix)는 Quartz로 바꾸고 6·7필드는 그대로. 결과는 계산 가능한지 검증.
pub fn to_quartz(cron: &str) -> Result<String, CronError> {
    if cron.trim().is_empty() {
        return Err(CronError("cron 표현식이 비어 있습니다.".into()));
    }
    let fields: Vec<&str> = cron.split_whitespace().collect();
    let quartz = match fields.len() {
        5 => from_unix(&fields)?,
        6 | 7 => fields.join(" "),
        _ => {
            return Err(CronError(format!(
                "cron 필드 수는 5(Unix) 또는 6/7(Quartz)이어야 합니다: {cron}"
            )));
        }
    };
    Schedule::parse(&quartz)
        .map_err(|_| CronError(format!("올바르지 않은 cron 표현식입니다: {cron}")))?;
    Ok(quartz)
}

/// Quartz는 일·요일 중 하나가 반드시 '?'라 둘 다 지정된 Unix 식(OR 의미)은 옮길 수 없다.
fn from_unix(f: &[&str]) -> Result<String, CronError> {
    let star = |s: &str| {
        if s == "?" {
            "*".to_string()
        } else {
            s.to_string()
        }
    };
    let (mut dom, mut dow) = (star(f[2]), star(f[4]));
    if dow == "*" {
        dow = "?".into();
    } else if dom == "*" {
        dom = "?".into();
        dow = to_quartz_days(&dow)?;
    } else {
        return Err(CronError(format!(
            "일과 요일을 동시에 지정한 cron은 지원하지 않습니다: {}",
            f.join(" ")
        )));
    }
    Ok(["0", f[0], f[1], &dom, f[3], &dow].join(" "))
}

/// Unix 요일 숫자(0·7=일)를 Quartz 숫자(1=일)로. step('/') 뒤 숫자는 그대로 둔다.
fn to_quartz_days(dow: &str) -> Result<String, CronError> {
    if dow.contains('/') && dow.chars().any(|c| c.is_ascii_alphabetic()) {
        return Err(CronError(format!(
            "요일 이름에는 step('/')을 붙일 수 없습니다: {dow}"
        )));
    }
    // Unix 0-7(매일)은 숫자로 옮기면 1-1(일요일만)이 된다.
    let days = match dow.strip_prefix("0-7") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("*{rest}"),
        _ => dow.to_string(),
    };
    let mut out = String::new();
    let mut chars = days.chars().peekable();
    let mut prev = None;
    while let Some(c) = chars.next() {
        if !c.is_ascii_digit() {
            out.push(c);
            prev = Some(c);
            continue;
        }
        let mut num = c.to_string();
        while let Some(d) = chars.next_if(char::is_ascii_digit) {
            num.push(d);
        }
        if prev == Some('/') {
            out.push_str(&num);
        } else {
            let n: u32 = num
                .parse()
                .ok()
                .filter(|n| *n <= 7)
                .ok_or_else(|| CronError(format!("요일은 0~7이어야 합니다: {dow}")))?;
            out.push_str(&(n % 7 + 1).to_string());
        }
        prev = Some(c);
    }
    Ok(out)
}

/// 파싱된 Quartz 식. 계산은 Asia/Seoul 벽시계 기준.
#[derive(Debug, Clone)]
pub struct Schedule(Cron);

impl Schedule {
    /// Quartz 6·7필드(초 필수, 연도 선택, 숫자 요일 1=일). 일·요일 중 정확히 하나가 `?`여야 한다(Quartz 규칙).
    pub fn parse(quartz: &str) -> Result<Schedule, CronError> {
        let bad = || CronError(format!("올바르지 않은 cron 표현식입니다: {quartz}"));
        let fields: Vec<&str> = quartz.split_whitespace().collect();
        if !(6..=7).contains(&fields.len())
            || (fields[3] == "?") == (fields[5] == "?")
            || !quartz
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || " */,-?#".contains(c))
        {
            return Err(bad());
        }
        let cron = CronParser::builder()
            .seconds(Seconds::Required)
            .year(Year::Optional)
            .alternative_weekdays(true)
            .build()
            .parse(quartz)
            .map_err(|_| bad())?;
        Ok(Schedule(cron))
    }

    /// t 초과 첫 차례.
    pub fn next_after(&self, t: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.0
            .find_next_occurrence(&t.with_timezone(&SCHEDULE_ZONE), false)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }

    /// (after, now] 안의 마지막 차례.
    pub fn latest_at_or_before(
        &self,
        after: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        self.0
            .find_previous_occurrence(&now.with_timezone(&SCHEDULE_ZONE), true)
            .ok()
            .map(|d| d.with_timezone(&Utc))
            .filter(|d| *d > after)
    }
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    Nothing,
    Run { slot: DateTime<Utc> },
    Expired { slot: DateTime<Utc> },
    ClockBack,
}

/// 놓친 차례가 여럿이어도 가장 최근 하나만 본다 — 유예 안이면 Run, 밖이면 Expired.
pub fn decide(s: &Schedule, last_slot: DateTime<Utc>, now: DateTime<Utc>) -> Decision {
    if now < last_slot {
        return Decision::ClockBack;
    }
    match s.latest_at_or_before(last_slot, now) {
        None => Decision::Nothing,
        Some(slot) if now - slot <= GRACE => Decision::Run { slot },
        Some(slot) => Decision::Expired { slot },
    }
}

/// Asia/Seoul 로컬 ISO, 오프셋 없음.
pub fn triggered_at(now: DateTime<Utc>) -> String {
    now.with_timezone(&SCHEDULE_ZONE)
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

/// BE `ScheduleTriggerHandler.output`: triggeredAt, `schedule.every`만 cron 원문도.
pub fn schedule_output(
    event: &str,
    config: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("triggeredAt".into(), triggered_at(now).into());
    if event == "schedule.every"
        && let Some(cron) = config.get("cron")
    {
        out.insert("cron".into(), cron.clone());
    }
    out
}
