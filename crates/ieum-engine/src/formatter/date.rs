//! 날짜 포맷터 3개 — BE `DateFormats`. 입력은 `condition::date_in`으로 읽는다(`지금`·`오늘` 포함).

use std::str::FromStr;
use std::sync::LazyLock;

use chrono::{
    DateTime, Datelike, FixedOffset, Months, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeDelta, TimeZone, Timelike, Utc,
};
use chrono_tz::Asia::Seoul;
use chrono_tz::Tz;
use regex::Regex;
use serde_json::{Map, Value};

use super::inputs::{Fail, In, MAX_CUSTOM_PATTERN, R, invalid, summary, too_large};
use super::{Entry, out};
use crate::condition::{When, at_zone, date_in, kst, strip};

pub(crate) const OPS: &[Entry] = &[
    ("formatter.date_format", "날짜 표기 바꾸기", format),
    ("formatter.date_add", "날짜 더하기·빼기", add),
    ("formatter.date_compare", "두 날짜 비교", compare),
];

type Out = R<Map<String, Value>>;

/// 식 한 항: 부호 필수 + 숫자 + 단위(붙어도 됨). Java `\s`·`\d`는 ASCII.
static TERM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t\n\x0B\x0C\r]*([+-])([0-9]+)[ \t\n\x0B\x0C\r]*(\p{L}+)[ \t\n\x0B\x0C\r]*")
        .unwrap()
});
const MAX_TERMS: usize = 20;
const MAX_AMOUNT_DIGITS: usize = 6;

#[derive(Clone, Copy)]
enum Unit {
    Years,
    Months,
    Weeks,
    Days,
    Hours,
    Minutes,
    Seconds,
}

const UNITS: &[(&str, Unit)] = &[
    ("year", Unit::Years),
    ("years", Unit::Years),
    ("년", Unit::Years),
    ("month", Unit::Months),
    ("months", Unit::Months),
    ("개월", Unit::Months),
    ("달", Unit::Months),
    ("week", Unit::Weeks),
    ("weeks", Unit::Weeks),
    ("주", Unit::Weeks),
    ("day", Unit::Days),
    ("days", Unit::Days),
    ("일", Unit::Days),
    ("hour", Unit::Hours),
    ("hours", Unit::Hours),
    ("시간", Unit::Hours),
    ("minute", Unit::Minutes),
    ("minutes", Unit::Minutes),
    ("분", Unit::Minutes),
    ("second", Unit::Seconds),
    ("seconds", Unit::Seconds),
    ("초", Unit::Seconds),
];

const PATTERNS: &[(&str, &str)] = &[
    ("korean", "yyyy년 M월 d일 (E)"),
    ("korean_time", "yyyy년 M월 d일 (E) HH:mm"),
    ("date", "yyyy-MM-dd"),
    ("datetime", "yyyy-MM-dd HH:mm"),
];

/// Java `ofPattern`이 아는 글자 중 지원하지 않는 것(나머지 영문자는 Java도 패턴 오류).
const JAVA_LETTERS: &str = "GuDLQqYwWecFKkSAnNVvzOXxZpBg";
const WEEKDAYS: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];

enum Piece {
    Lit(String),
    Field(char, usize),
}

enum Zone {
    Tz(Tz),
    Fixed(FixedOffset),
}

impl Zone {
    fn to_utc(&self, local: NaiveDateTime) -> Option<DateTime<Utc>> {
        match self {
            Zone::Tz(z) => at_zone(z, local),
            Zone::Fixed(o) => at_zone(o, local),
        }
    }

    fn offset(&self, t: DateTime<Utc>) -> FixedOffset {
        match self {
            Zone::Tz(z) => z.offset_from_utc_datetime(&t.naive_utc()).fix(),
            Zone::Fixed(o) => *o,
        }
    }

    /// 그 시간대의 벽시계(초 아래 버림)와 오프셋. chrono 범위를 넘으면 None.
    fn local(&self, t: DateTime<Utc>) -> Option<(NaiveDateTime, FixedOffset)> {
        let off = self.offset(t);
        let local = t.naive_utc().checked_add_offset(off)?.with_nanosecond(0)?;
        Some((local, off))
    }
}

fn format(i: &In, now: DateTime<Utc>) -> Out {
    let format = i.choice(
        "format",
        Some("korean"),
        &["korean", "korean_time", "date", "datetime", "iso", "custom"],
    )?;
    let pieces = match format {
        "iso" => None,
        "custom" => Some(custom(i)?),
        f => {
            let p = PATTERNS.iter().find(|p| p.0 == f).map_or("", |p| p.1);
            Some(pattern(p)?)
        }
    };
    let from = zone(i, "fromTimezone")?;
    let to = zone(i, "toTimezone")?;
    let w = read(i, "input", "입력값", now, |l| from.to_utc(l))?;
    let instant = if w.date_only {
        from.to_utc(w.day.and_time(NaiveTime::MIN))
    } else {
        Some(w.instant)
    };
    let (local, off) = instant
        .and_then(|t| to.local(t))
        .ok_or_else(|| too_large("날짜를 표기할 수 있는 범위를 넘었어요"))?;
    Ok(out(match pieces {
        None => iso(local, off),
        Some(p) => render(&p, local),
    }))
}

fn add(i: &In, now: DateTime<Utc>) -> Out {
    let terms = terms(&i.required("expression", "더하거나 뺄 기간")?)?;
    let w = read(i, "input", "입력값", now, kst)?;
    let range = || too_large("계산한 날짜가 표기할 수 있는 범위를 넘었어요");
    if w.date_only
        && terms
            .iter()
            .all(|t| matches!(t.1, Unit::Years | Unit::Months | Unit::Weeks | Unit::Days))
    {
        let mut d = w.day;
        for t in &terms {
            d = plus_date(d, *t).ok_or_else(range)?;
        }
        return Ok(out(iso_date(d)));
    }
    let kst_zone = Zone::Tz(Seoul);
    let mut z = w.instant;
    for t in &terms {
        z = plus_zoned(z, *t).ok_or_else(range)?;
    }
    let (local, off) = kst_zone.local(z).ok_or_else(range)?;
    Ok(out(iso(local, off)))
}

fn compare(i: &In, now: DateTime<Utc>) -> Out {
    let a = read(i, "input", "기준 날짜", now, kst)?;
    let b = read(i, "other", "비교할 날짜", now, kst)?;
    let by_day = a.date_only || b.date_only;
    let (days, hours, minutes) = if by_day {
        let d = b.day.signed_duration_since(a.day).num_days();
        (d, d * 24, d * 1440)
    } else {
        // Java Duration은 초를 내림으로 쥐고 나눈다(-59.5초 → -1분).
        let d = b.instant - a.instant;
        let s = d.num_seconds() - i64::from(d.subsec_nanos() < 0);
        (s / 86_400, s / 3600, s / 60)
    };
    let before = if by_day {
        a.day < b.day
    } else {
        a.instant < b.instant
    };
    Ok(Map::from_iter([
        ("days".to_string(), days.into()),
        ("hours".to_string(), hours.into()),
        ("minutes".to_string(), minutes.into()),
        ("isBefore".to_string(), before.into()),
    ]))
}

fn read(
    i: &In,
    key: &'static str,
    title: &str,
    now: DateTime<Utc>,
    zone: impl Fn(NaiveDateTime) -> Option<DateTime<Utc>>,
) -> R<When> {
    let s = i.required(key, title)?;
    date_in(&Value::String(s.clone()), now, zone)
        .ok_or_else(|| invalid(key, format!("'{}'를 날짜로 읽지 못했어요", summary(&s))))
}

/// Java `ZoneId.of` — 지역 이름, `Z`, `±h[h[[:]mm[[:]ss]]]`, `UTC`·`GMT`·`UT`(+오프셋). 비우면 KST.
fn zone(i: &In, key: &'static str) -> R<Zone> {
    let text = i.text(key)?;
    let s = strip(&text);
    if s.is_empty() {
        return Ok(Zone::Tz(Seoul));
    }
    let rest = ["UTC", "GMT", "UT"]
        .iter()
        .find_map(|p| s.strip_prefix(p))
        .unwrap_or(s);
    let zone = if rest.is_empty() {
        FixedOffset::east_opt(0).map(Zone::Fixed)
    } else if s == "Z" || rest.starts_with(['+', '-']) {
        offset(rest).map(Zone::Fixed)
    } else {
        Tz::from_str(s).ok().map(Zone::Tz)
    };
    zone.ok_or_else(|| invalid(key, format!("알 수 없는 시간대예요: {}", summary(s))))
}

/// Java `ZoneOffset.of`.
fn offset(s: &str) -> Option<FixedOffset> {
    if s == "Z" {
        return FixedOffset::east_opt(0);
    }
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let b = &s.as_bytes()[1..];
    let num = |at: usize| -> Option<i32> {
        let d = b.get(at..at + 2)?;
        d.iter()
            .all(u8::is_ascii_digit)
            .then(|| i32::from(d[0] - b'0') * 10 + i32::from(d[1] - b'0'))
    };
    let colon = |at: usize| b.get(at) == Some(&b':');
    let (h, m, sec) = match b.len() {
        1 if b[0].is_ascii_digit() => (i32::from(b[0] - b'0'), 0, 0),
        2 => (num(0)?, 0, 0),
        4 => (num(0)?, num(2)?, 0),
        5 if colon(2) => (num(0)?, num(3)?, 0),
        6 => (num(0)?, num(2)?, num(4)?),
        8 if colon(2) && colon(5) => (num(0)?, num(3)?, num(6)?),
        _ => return None,
    };
    let total = h * 3600 + m * 60 + sec;
    if h > 18 || m > 59 || sec > 59 || total > 18 * 3600 {
        return None;
    }
    FixedOffset::east_opt(sign * total)
}

fn custom(i: &In) -> R<Vec<Piece>> {
    let p = i.required("customPattern", "직접 정한 모양")?;
    if p.encode_utf16().count() > MAX_CUSTOM_PATTERN {
        return Err(too_large("'직접 정한 모양' 칸은 100자까지예요"));
    }
    pattern(&p)
}

fn bad_pattern() -> Fail {
    invalid(
        "customPattern",
        "'직접 정한 모양'이 올바른 날짜 패턴이 아니에요",
    )
}

/// Java `DateTimeFormatter.ofPattern` 중 `y M d E H h m s a`·따옴표 글자·그 밖 글자만.
fn pattern(p: &str) -> R<Vec<Piece>> {
    let cs: Vec<char> = p.chars().collect();
    let mut pieces = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_alphabetic() {
            let start = i;
            while cs.get(i) == Some(&c) {
                i += 1;
            }
            let n = i - start;
            let max = match c {
                'y' => 19,
                'M' if (3..=5).contains(&n) => return Err(unsupported(c)),
                'M' | 'd' | 'H' | 'h' | 'm' | 's' => 2,
                'E' => 5,
                'a' => 1,
                _ if JAVA_LETTERS.contains(c) => return Err(unsupported(c)),
                _ => return Err(bad_pattern()),
            };
            if n > max {
                return Err(bad_pattern());
            }
            pieces.push(Piece::Field(c, n));
        } else if c == '\'' {
            i += 1;
            let mut lit = String::new();
            loop {
                match (cs.get(i), cs.get(i + 1)) {
                    (None, _) => return Err(bad_pattern()),
                    (Some('\''), Some('\'')) => {
                        lit.push('\'');
                        i += 2;
                    }
                    (Some('\''), _) => {
                        i += 1;
                        break;
                    }
                    (Some(&ch), _) => {
                        lit.push(ch);
                        i += 1;
                    }
                }
            }
            // 바깥의 `''`는 따옴표 한 글자.
            pieces.push(Piece::Lit(if lit.is_empty() { "'".into() } else { lit }));
        } else if matches!(c, '[' | ']') {
            return Err(unsupported(c));
        } else if matches!(c, '{' | '}' | '#') {
            return Err(bad_pattern());
        } else {
            pieces.push(Piece::Lit(c.to_string()));
            i += 1;
        }
    }
    Ok(pieces)
}

fn unsupported(c: char) -> Fail {
    invalid(
        "customPattern",
        format!("지원하지 않는 날짜 패턴 문자예요: {c}"),
    )
}

fn render(pieces: &[Piece], t: NaiveDateTime) -> String {
    let mut s = String::new();
    for p in pieces {
        match p {
            Piece::Lit(l) => s.push_str(l),
            Piece::Field(c, n) => {
                let n = *n;
                let num = |v: u32| format!("{v:0n$}");
                s += &match c {
                    'y' => {
                        // YEAR_OF_ERA: yy는 끝 두 자리, 4자 이상은 넘치면 `+`.
                        let yoe = if t.year() > 0 {
                            i64::from(t.year())
                        } else {
                            1 - i64::from(t.year())
                        };
                        let d = yoe.to_string();
                        match n {
                            2 => format!("{:02}", yoe % 100),
                            _ if n >= 4 && d.len() > n => format!("+{d}"),
                            _ => format!("{yoe:0n$}"),
                        }
                    }
                    'M' => num(t.month()),
                    'd' => num(t.day()),
                    'H' => num(t.hour()),
                    'h' => num(t.hour12().1),
                    'm' => num(t.minute()),
                    's' => num(t.second()),
                    'E' => {
                        let w = WEEKDAYS[t.weekday().num_days_from_monday() as usize];
                        if n == 4 {
                            format!("{w}요일")
                        } else {
                            w.into()
                        }
                    }
                    _ => (if t.hour() < 12 { "오전" } else { "오후" }).into(),
                };
            }
        }
    }
    s
}

/// Java `LocalDate.toString`·ISO 연도: 4자리, 9999 넘으면 `+`, 음수는 `-0001`.
fn iso_date(d: NaiveDate) -> String {
    let y = d.year();
    let year = if y > 9999 {
        format!("+{y}")
    } else if y < 0 {
        format!("-{:04}", -y)
    } else {
        format!("{y:04}")
    };
    format!("{year}-{:02}-{:02}", d.month(), d.day())
}

/// Java `ISO_OFFSET_DATE_TIME`(초까지): 오프셋 0은 `Z`, 초 단위 오프셋은 `+HH:MM:ss`.
fn iso(t: NaiveDateTime, off: FixedOffset) -> String {
    let total = off.local_minus_utc();
    let zone = if total == 0 {
        "Z".to_string()
    } else {
        let a = total.abs();
        let sign = if total < 0 { '-' } else { '+' };
        let sec = if a % 60 == 0 {
            String::new()
        } else {
            format!(":{:02}", a % 60)
        };
        format!("{sign}{:02}:{:02}{sec}", a / 3600, a / 60 % 60)
    };
    format!(
        "{}T{:02}:{:02}:{:02}{zone}",
        iso_date(t.date()),
        t.hour(),
        t.minute(),
        t.second()
    )
}

/// `+1 month -2 days`·`+3일 -2시간`. 부호 없는 항·모르는 단위·남는 글자는 실패.
fn terms(expr: &str) -> R<Vec<(i64, Unit)>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < expr.len() {
        let m = TERM.captures(&expr[pos..]).ok_or_else(|| {
            invalid(
                "expression",
                "기간 식을 읽지 못했어요(예: +1 month -2 days)",
            )
        })?;
        if out.len() == MAX_TERMS {
            return Err(too_large(format!("기간 식은 {MAX_TERMS}항까지예요")));
        }
        if m[2].len() > MAX_AMOUNT_DIGITS {
            return Err(too_large("기간 숫자가 너무 커요(6자리까지)"));
        }
        let name = m[3].to_lowercase();
        let unit = UNITS
            .iter()
            .find(|u| u.0 == name)
            .map(|u| u.1)
            .ok_or_else(|| {
                invalid(
                    "expression",
                    format!("알 수 없는 단위예요: {}", summary(&m[3])),
                )
            })?;
        let n: i64 = m[2].parse().unwrap_or(0);
        out.push((if &m[1] == "-" { -n } else { n }, unit));
        pos += m[0].len();
    }
    Ok(out)
}

/// Java `LocalDate.plus` — 달·해는 말일로 맞춘다.
fn plus_date(d: NaiveDate, (n, unit): (i64, Unit)) -> Option<NaiveDate> {
    let months = |k: i64| {
        let m = Months::new(u32::try_from(k.unsigned_abs()).ok()?);
        if k < 0 {
            d.checked_sub_months(m)
        } else {
            d.checked_add_months(m)
        }
    };
    match unit {
        Unit::Years => months(n * 12),
        Unit::Months => months(n),
        Unit::Weeks => d.checked_add_signed(TimeDelta::try_days(n * 7)?),
        Unit::Days => d.checked_add_signed(TimeDelta::try_days(n)?),
        _ => None,
    }
}

/// Java `ZonedDateTime.plus`(KST) — 날짜 단위는 벽시계에 더해 이전 오프셋을 우선, 시각 단위는 순간에 더한다.
fn plus_zoned(t: DateTime<Utc>, (n, unit): (i64, Unit)) -> Option<DateTime<Utc>> {
    let delta = match unit {
        Unit::Hours => TimeDelta::try_hours(n),
        Unit::Minutes => TimeDelta::try_minutes(n),
        Unit::Seconds => TimeDelta::try_seconds(n),
        _ => None,
    };
    if let Some(delta) = delta {
        return t.checked_add_signed(delta);
    }
    let prev = Seoul.offset_from_utc_datetime(&t.naive_utc()).fix();
    let local = t.naive_utc().checked_add_offset(prev)?;
    let date = plus_date(local.date(), (n, unit))?;
    let next = date.and_time(local.time());
    match Seoul.from_local_datetime(&next) {
        chrono::LocalResult::Ambiguous(_, b) if b.offset().fix() == prev => Some(b.to_utc()),
        _ => kst(next),
    }
}
