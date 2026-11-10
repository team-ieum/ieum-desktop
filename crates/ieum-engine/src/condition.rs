//! `branch.if` 실행기 — BE `ConditionNodeExecutor`·`ConditionValues`를 옮겼다.
//! 다른 점: BE의 detail(갈래·조건별 판단 근거)은 실을 이벤트가 없어 만들지 않는다. 경고는 `evaluate`가 돌려준다.
//! 값의 글자 표기는 Java `String.valueOf`(숫자 `1.0E10`, 목록 `[a, b]`, 객체 `{k=v}`)를 따른다.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use chrono::{
    DateTime, Datelike, Days, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeDelta,
    TimeZone, Utc,
};
use chrono_tz::Asia::Seoul;
use regex::Regex;
use serde_json::{Map, Number, Value};

use crate::run::{NodeCtx, NodeError, NodeRunner, Runners};

const MAX_BRANCHES: usize = 10;
const MAX_RULES: usize = 10;
const RESERVED_KEYS: [&str; 3] = ["else", "true", "false"];
const OPERATORS: [&str; 20] = [
    "equals",
    "notEquals",
    "contains",
    "notContains",
    "startsWith",
    "endsWith",
    "in",
    "notIn",
    "greaterThan",
    "lessThan",
    "greaterThanOrEqual",
    "lessThanOrEqual",
    "numberEquals",
    "before",
    "after",
    "sameDay",
    "isTrue",
    "isFalse",
    "isEmpty",
    "isNotEmpty",
];
const OTHERWISE_NAME: &str = "그 외";
static NULL: Value = Value::Null;

/// 유닉스 시각: 숫자만 10자리(초)·13자리(밀리초). 0으로 시작하면 아님.
static UNIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[1-9][0-9]{9}|[1-9][0-9]{12})$").unwrap());
/// `2026년 11월 7일`(Java `\d`·`\s`는 ASCII).
static KOREAN_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]{4})년[ \t\n\x0B\x0C\r]*([0-9]{1,2})월[ \t\n\x0B\x0C\r]*([0-9]{1,2})일$")
        .unwrap()
});

/// 시계를 주입할 수 있다 — `None`이면 시스템 시계.
#[derive(Default)]
pub struct ConditionRunner {
    now: Option<DateTime<Utc>>,
}

impl ConditionRunner {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self { now: Some(now) }
    }
}

#[async_trait]
impl NodeRunner for ConditionRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        evaluate(&ctx.config, self.now.unwrap_or_else(Utc::now)).map(|e| e.output)
    }
}

/// `Runners`(타입 별칭)에 `with_builtin()`을 붙이는 확장.
pub trait BuiltinRunners {
    fn with_builtin() -> Self;
}

impl BuiltinRunners for Runners {
    fn with_builtin() -> Self {
        let cond: Arc<dyn NodeRunner> = Arc::new(ConditionRunner::default());
        Runners::from([("branch.if".to_string(), cond)])
    }
}

#[derive(Debug)]
pub struct Evaluation {
    pub output: Map<String, Value>,
    /// 판단 불가로 거짓 처리한 조건마다 한 줄(BE detail.warnings와 같은 문구).
    pub warnings: Vec<String>,
}

struct Rule<'a> {
    left: &'a Value,
    op: &'a str,
    right: &'a Value,
}

struct Branch<'a> {
    key: &'a str,
    name: &'a str,
    any: bool,
    rules: Vec<Rule<'a>>,
}

struct Setup<'a> {
    branches: Vec<Branch<'a>>,
    pick_all: bool,
    stop: bool,
    case_sensitive: bool,
    legacy: bool,
}

fn fail<T>(message: String) -> Result<T, NodeError> {
    Err(NodeError { message })
}

/// 설정 오류는 `Err`(노드 실패), 값을 못 읽은 조건은 거짓 + 경고.
pub fn evaluate(input: &Map<String, Value>, now: DateTime<Utc>) -> Result<Evaluation, NodeError> {
    let setup = parse(input)?;
    let mut warnings = Vec::new();
    let mut picked: Vec<(&str, &str)> = Vec::new(); // (key, name)
    for b in &setup.branches {
        let mut passed = Vec::new();
        for (i, rule) in b.rules.iter().enumerate() {
            let at = format!("갈래 '{}' {}번째 조건", b.name, i + 1);
            passed.push(check(rule, setup.case_sensitive, now, &at, &mut warnings));
        }
        let matched = if b.any {
            passed.contains(&true)
        } else {
            !passed.contains(&false)
        };
        if matched {
            picked.push((b.key, b.name));
        }
    }
    let matched = !picked.is_empty();
    if matched && !setup.pick_all {
        picked.truncate(1);
    } else if !matched && !setup.stop {
        picked.push((if setup.legacy { "false" } else { "else" }, OTHERWISE_NAME));
    }

    let mut output = Map::new();
    output.insert(
        "branch".into(),
        picked.first().map_or(Value::Null, |p| p.1.into()),
    );
    output.insert("branches".into(), picked.iter().map(|p| p.1).collect());
    output.insert("matched".into(), matched.into());
    output.insert("result".into(), matched.into());
    output.insert("_branchKeys".into(), picked.iter().map(|p| p.0).collect());
    Ok(Evaluation { output, warnings })
}

// ──────────────────────────────────────────────────────────────────────
// 설정 해석
// ──────────────────────────────────────────────────────────────────────

fn get<'a>(map: &'a Map<String, Value>, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&NULL)
}

fn parse(input: &Map<String, Value>) -> Result<Setup<'_>, NodeError> {
    let case_sensitive = match get(input, "caseSensitive") {
        Value::Bool(b) => *b,
        Value::String(s) => s.eq_ignore_ascii_case("true"),
        _ => false,
    };
    let list = match get(input, "branches") {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::Array(l) if l.is_empty() => None,
        Value::Array(l) => Some(l),
        _ => return fail("branches는 목록이어야 합니다.".into()),
    };
    let Some(list) = list else {
        let side = |k: &str, alt: &str| get(input, if input.contains_key(k) { k } else { alt });
        let rule = Rule {
            left: side("left", "leftValue"),
            op: operator(get(input, "operator"))?,
            right: side("right", "rightValue"),
        };
        let branches = vec![Branch {
            key: "true",
            name: "참",
            any: false,
            rules: vec![rule],
        }];
        return Ok(Setup {
            branches,
            pick_all: false,
            stop: false,
            case_sensitive,
            legacy: true,
        });
    };
    if list.len() > MAX_BRANCHES {
        return fail(format!("갈래는 최대 {MAX_BRANCHES}개입니다."));
    }
    let mut keys = HashSet::new();
    let mut branches = Vec::new();
    for item in list {
        let Value::Object(map) = item else {
            return fail("갈래는 객체여야 합니다.".into());
        };
        branches.push(branch(map, &mut keys)?);
    }
    let pick_all = choice(get(input, "pick"), "first", "pick", &["first", "all"])? == "all";
    let stop = choice(
        get(input, "otherwise"),
        "branch",
        "otherwise",
        &["branch", "stop"],
    )? == "stop";
    Ok(Setup {
        branches,
        pick_all,
        stop,
        case_sensitive,
        legacy: false,
    })
}

fn branch<'a>(
    map: &'a Map<String, Value>,
    keys: &mut HashSet<&'a str>,
) -> Result<Branch<'a>, NodeError> {
    let key = match get(map, "key") {
        Value::Null => "",
        Value::String(s) => s.as_str(),
        other => {
            return fail(format!(
                "갈래 key는 문자열이어야 합니다: {}",
                java_str(other)
            ));
        }
    };
    if key.is_empty() {
        return fail("갈래 key가 비어 있습니다.".into());
    }
    if RESERVED_KEYS.contains(&key) {
        return fail(format!("갈래 key로 예약어를 쓸 수 없습니다: {key}"));
    }
    if !keys.insert(key) {
        return fail(format!("갈래 key가 중복됩니다: {key}"));
    }
    let name = match get(map, "name") {
        Value::String(s) if !s.is_empty() => s.as_str(),
        _ => key,
    };
    let any = choice(get(map, "match"), "all", "match", &["all", "any"])? == "any";
    let raw = match get(map, "rules") {
        Value::Array(r) if !r.is_empty() => r,
        _ => return fail(format!("갈래 '{name}'에 조건 목록이 없습니다.")),
    };
    if raw.len() > MAX_RULES {
        return fail(format!("갈래 '{name}'의 조건은 최대 {MAX_RULES}개입니다."));
    }
    let mut rules = Vec::new();
    for r in raw {
        let Value::Object(rule) = r else {
            return fail(format!("갈래 '{name}'의 조건은 객체여야 합니다."));
        };
        rules.push(Rule {
            left: get(rule, "left"),
            op: operator(get(rule, "operator"))?,
            right: get(rule, "right"),
        });
    }
    Ok(Branch {
        key,
        name,
        any,
        rules,
    })
}

fn operator(value: &Value) -> Result<&str, NodeError> {
    match value {
        Value::String(op) if OPERATORS.contains(&op.as_str()) => Ok(op),
        other => fail(format!("지원하지 않는 연산자: {}", java_str(other))),
    }
}

/// null·`""`면 기본값, 아니면 허용 값 중 하나여야 한다.
fn choice<'a>(
    value: &Value,
    fallback: &'a str,
    field: &str,
    allowed: &[&'a str],
) -> Result<&'a str, NodeError> {
    match value {
        Value::Null => Ok(fallback),
        Value::String(s) if s.is_empty() => Ok(fallback),
        Value::String(s) if allowed.contains(&s.as_str()) => {
            Ok(allowed.iter().find(|a| **a == s.as_str()).unwrap())
        }
        other => fail(format!("지원하지 않는 {field} 값: {}", java_str(other))),
    }
}

// ──────────────────────────────────────────────────────────────────────
// 조건 평가 — 판단 불가는 거짓 + 경고
// ──────────────────────────────────────────────────────────────────────

fn check(rule: &Rule, cs: bool, now: DateTime<Utc>, at: &str, warnings: &mut Vec<String>) -> bool {
    let (l, r, op) = (rule.left, rule.right, rule.op);
    let tr = text(r, cs);
    let empty_right = |warnings: &mut Vec<String>| {
        warnings.push(format!("{at}: 기준값이 비어 있어 거짓으로 봤어요."));
        false
    };
    match op {
        "equals" => text(l, cs) == tr,
        "notEquals" => text(l, cs) != tr,
        "contains" | "notContains" | "startsWith" | "endsWith" => {
            if tr.is_empty() {
                return empty_right(warnings);
            }
            let tl = text(l, cs);
            match op {
                "startsWith" => tl.starts_with(&tr),
                "endsWith" => tl.ends_with(&tr),
                _ => {
                    let has = json_list(l, cs)
                        .map_or_else(|| tl.contains(&tr), |items| items.contains(&tr));
                    (op == "contains") == has
                }
            }
        }
        "in" | "notIn" => {
            let items = list_items(r, cs);
            if items.is_empty() {
                return empty_right(warnings);
            }
            (op == "in") == items.contains(&text(l, cs))
        }
        "greaterThan" | "lessThan" | "greaterThanOrEqual" | "lessThanOrEqual" | "numberEquals" => {
            let a = number(l);
            let b = a.as_ref().and_then(|_| number(r));
            let (Some(a), Some(b)) = (a.as_ref(), b) else {
                let bad = if a.is_none() { l } else { r };
                warnings.push(format!(
                    "{at}: '{}'을(를) 숫자로 읽지 못해 거짓으로 봤어요.",
                    shown(bad)
                ));
                return false;
            };
            let c = a.compare(&b);
            match op {
                "greaterThan" => c.is_gt(),
                "lessThan" => c.is_lt(),
                "greaterThanOrEqual" => c.is_ge(),
                "lessThanOrEqual" => c.is_le(),
                _ => c.is_eq(),
            }
        }
        "before" | "after" | "sameDay" => {
            let a = date(l, now);
            let b = a.as_ref().and_then(|_| date(r, now));
            let (Some(a), Some(b)) = (a.as_ref(), b) else {
                let bad = if a.is_none() { l } else { r };
                warnings.push(format!(
                    "{at}: '{}'을(를) 날짜로 읽지 못해 거짓으로 봤어요.",
                    shown(bad)
                ));
                return false;
            };
            match op {
                "before" => a.compare(&b).is_lt(),
                "after" => a.compare(&b).is_gt(),
                _ => a.day == b.day,
            }
        }
        "isTrue" => bool_of(l) == Some(true),
        "isFalse" => bool_of(l) == Some(false),
        "isEmpty" => is_empty(l),
        _ => !is_empty(l), // isNotEmpty — 연산자는 parse에서 걸렀다
    }
}

/// 경고에 넣는 값 — 200자(UTF-16) 넘으면 앞 200자 + "…", 서로게이트 쌍은 가르지 않는다.
fn shown(value: &Value) -> String {
    let s = java_str(value);
    if s.encode_utf16().count() <= 200 {
        return s;
    }
    let mut n = 0;
    let cut: String = s
        .chars()
        .take_while(|c| {
            n += c.len_utf16();
            n <= 200
        })
        .collect();
    cut + "…"
}

// ──────────────────────────────────────────────────────────────────────
// 값 읽기 — BE `ConditionValues`
// ──────────────────────────────────────────────────────────────────────

/// Java `String.valueOf`.
fn java_str(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => java_num(n),
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(java_str).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(m) => {
            let items: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}={}", java_str(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Java `Double.toString` — 1e-3 이상 1e7 미만은 소수, 그 밖은 `1.0E10` 꼴.
fn java_num(n: &Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f != 0.0 && !(1e-3..1e7).contains(&f.abs()) => {
            let s = format!("{f:e}");
            let (m, e) = s.split_once('e').unwrap();
            if m.contains('.') {
                format!("{m}E{e}")
            } else {
                format!("{m}.0E{e}")
            }
        }
        _ => n.to_string(),
    }
}

/// Java `String.strip` — `Character.isWhitespace`(NBSP류 제외, U+001C–U+001F 포함).
fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(c, '\u{1C}'..='\u{1F}')
            || (c.is_whitespace() && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}' | '\u{85}'))
    })
}

fn text(value: &Value, cs: bool) -> String {
    if value.is_null() {
        return String::new();
    }
    let s = strip(&java_str(value)).to_string();
    if cs { s } else { s.to_lowercase() }
}

fn json_list(value: &Value, cs: bool) -> Option<Vec<String>> {
    let parsed;
    let items = match value {
        Value::Array(list) => list,
        Value::String(s) if strip(s).starts_with('[') => {
            parsed = serde_json::from_str::<Vec<Value>>(strip(s)).ok()?;
            &parsed
        }
        _ => return None,
    };
    Some(
        items
            .iter()
            .map(|i| text(i, cs))
            .filter(|i| !i.is_empty())
            .collect(),
    )
}

fn list_items(value: &Value, cs: bool) -> Vec<String> {
    json_list(value, cs).unwrap_or_else(|| {
        text(value, cs)
            .split(',')
            .map(|i| strip(i).to_string())
            .filter(|i| !i.is_empty())
            .collect()
    })
}

/// `BigDecimal` 값 비교용 — 0.d₁d₂… × 10^pos, 앞뒤 0 없는 숫자열(빈 = 0).
struct Decimal {
    neg: bool,
    digits: Vec<u8>,
    pos: i64,
}

impl Decimal {
    /// `BigDecimal.compareTo` — 표기(scale)는 보지 않는다.
    fn compare(&self, other: &Self) -> Ordering {
        let sign = |d: &Decimal| {
            if d.digits.is_empty() {
                0
            } else if d.neg {
                -1
            } else {
                1
            }
        };
        let (a, b) = (sign(self), sign(other));
        if a != b || a == 0 {
            return a.cmp(&b);
        }
        let mag = self
            .pos
            .cmp(&other.pos)
            .then_with(|| self.digits.cmp(&other.digits));
        if a < 0 { mag.reverse() } else { mag }
    }
}

fn number(value: &Value) -> Option<Decimal> {
    let s = match value {
        Value::Number(n) => java_num(n),
        // Java `(?U)[\s,원%]` 제거 — `\s` = 유니코드 White_Space
        Value::String(s) => s
            .chars()
            .filter(|&c| !(c.is_whitespace() || matches!(c, ',' | '원' | '%')))
            .collect(),
        _ => return None,
    };
    // BigDecimal 거대 입력 차단(Jackson maxNumberLength와 같은 1000)
    if s.is_empty() || s.encode_utf16().count() > 1000 {
        return None;
    }
    decimal(&s)
}

/// `new BigDecimal(String)` 문법: 부호? (숫자+ ('.' 숫자*)? | '.' 숫자+) ([eE] 부호? 숫자+)?
fn decimal(s: &str) -> Option<Decimal> {
    let (neg, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let end = body.find(['e', 'E']).unwrap_or(body.len());
    let (mantissa, exp) = (&body[..end], body.get(end + 1..));
    // i32 parse = 부호? ASCII 숫자+ (BigDecimal 지수도 int)
    let exp: i64 = exp.map_or(Some(0), |e| e.parse::<i32>().ok())?.into();
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.len() + frac.len() == 0 || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    // BigDecimal scale은 int 범위
    i32::try_from(frac.len() as i64 - exp).ok()?;
    let all: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    let lead = all.iter().take_while(|&&d| d == 0).count();
    let mut digits = all[lead..].to_vec();
    while digits.last() == Some(&0) {
        digits.pop();
    }
    Some(Decimal {
        neg,
        digits,
        pos: int.len() as i64 + exp - lead as i64,
    })
}

/// `day`는 KST 날짜. 날짜만 읽힌 값은 `instant` = 그날 KST 00:00.
struct When {
    instant: DateTime<Utc>,
    day: NaiveDate,
    date_only: bool,
}

impl When {
    /// KST 날짜가 chrono 범위를 넘으면 None(panic 대신).
    fn at(instant: DateTime<Utc>) -> Option<Self> {
        let utc = instant.naive_utc();
        let offset = Seoul.offset_from_utc_datetime(&utc).fix();
        Some(When {
            instant,
            day: utc.checked_add_offset(offset)?.date(),
            date_only: false,
        })
    }

    fn on(day: NaiveDate) -> Option<Self> {
        Some(When {
            instant: kst(day.and_time(NaiveTime::MIN))?,
            day,
            date_only: true,
        })
    }

    fn compare(&self, other: &When) -> Ordering {
        if self.date_only || other.date_only {
            self.day.cmp(&other.day)
        } else {
            self.instant.cmp(&other.instant)
        }
    }
}

/// Java `LocalDateTime.atZone(KST)` — 겹치면 이른 쪽, 틈이면 틈 전 오프셋.
fn kst(local: NaiveDateTime) -> Option<DateTime<Utc>> {
    match Seoul.from_local_datetime(&local).earliest() {
        Some(t) => Some(t.to_utc()),
        None => {
            let probe = local.checked_sub_signed(TimeDelta::hours(12))?;
            let before = Seoul.offset_from_utc_datetime(&probe).fix();
            Some(local.checked_sub_offset(before)?.and_utc())
        }
    }
}

fn date(value: &Value, now: DateTime<Utc>) -> Option<When> {
    if value.is_null() {
        return None;
    }
    let raw = java_str(value);
    let s = strip(&raw);
    match s {
        "" => return None,
        "오늘" => return When::on(When::at(now)?.day),
        "지금" => return When::at(now),
        _ => {}
    }
    let t = if s.len() > 10 && s.as_bytes()[10] == b' ' && s[..10].is_ascii() {
        format!("{}T{}", &s[..10], &s[11..])
    } else {
        s.to_string()
    };
    let mut p = Cursor::new(&t);
    if let Some(local) = p.local_date_time() {
        if p.done() {
            return When::at(kst(local)?);
        }
        if let Some(off) = p.offset_id()
            && p.done()
        {
            return When::at(local.checked_sub_offset(off)?.and_utc());
        }
    }
    let dashed = s.replace(['.', '/'], "-");
    let mut p = Cursor::new(&dashed);
    if let Some(d) = p.local_date()
        && p.done()
    {
        return When::on(d);
    }
    if UNIX.is_match(s) {
        let n: i64 = s.parse().ok()?;
        let at = if s.len() == 10 {
            DateTime::from_timestamp(n, 0)
        } else {
            DateTime::from_timestamp_millis(n)
        };
        return at.and_then(When::at);
    }
    if let Some(k) = KOREAN_DATE.captures(s) {
        let n = |i: usize| k[i].parse::<u32>().ok();
        return NaiveDate::from_ymd_opt(n(1)? as i32, n(2)?, n(3)?).and_then(When::on);
    }
    rfc_1123(s).and_then(When::at)
}

/// Java `DateTimeFormatter`의 고정 형식 일부를 읽는 커서(ISO는 엄격, 대소문자 무시).
struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Cursor {
            s: s.as_bytes(),
            i: 0,
        }
    }

    fn done(&self) -> bool {
        self.i == self.s.len()
    }

    fn lit(&mut self, c: u8) -> bool {
        let ok = self
            .s
            .get(self.i)
            .is_some_and(|b| b.eq_ignore_ascii_case(&c));
        self.i += ok as usize;
        ok
    }

    /// 숫자 min..=max자리(욕심껏).
    fn digits(&mut self, min: usize, max: usize) -> Option<u64> {
        let start = self.i;
        while self.i - start < max && self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
            self.i += 1;
        }
        if self.i - start < min {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
    }

    /// ISO_LOCAL_DATE: yyyy-MM-dd(엄격).
    fn local_date(&mut self) -> Option<NaiveDate> {
        let y = self.digits(4, 4)?;
        self.lit(b'-').then_some(())?;
        let m = self.digits(2, 2)?;
        self.lit(b'-').then_some(())?;
        let d = self.digits(2, 2)?;
        NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
    }

    /// ISO_LOCAL_DATE_TIME: 날짜 'T' HH:mm[:ss[.0~9자리]].
    fn local_date_time(&mut self) -> Option<NaiveDateTime> {
        let date = self.local_date()?;
        self.lit(b'T').then_some(())?;
        let h = self.digits(2, 2)?;
        self.lit(b':').then_some(())?;
        let m = self.digits(2, 2)?;
        let (mut sec, mut nano) = (0, 0);
        let back = self.i;
        if self.lit(b':') {
            match self.digits(2, 2) {
                None => self.i = back,
                Some(s) => {
                    sec = s;
                    if self.lit(b'.') {
                        let start = self.i;
                        let f = self.digits(0, 9).unwrap_or(0);
                        nano = f * 10u64.pow(9 - (self.i - start) as u32);
                    }
                }
            }
        }
        let time = NaiveTime::from_hms_nano_opt(h as u32, m as u32, sec as u32, nano as u32)?;
        Some(date.and_time(time))
    }

    /// ISO 오프셋(Java 21 실측): Z, ±HH, ±HH:MM, ±HH:MM:ss.
    fn offset_id(&mut self) -> Option<FixedOffset> {
        if self.lit(b'Z') {
            return FixedOffset::east_opt(0);
        }
        self.offset(true)
    }

    /// `colon`이면 ±HH[:MM[:ss]], 아니면 ±HH[MM[ss]](RFC 1123, Java 21 실측).
    fn offset(&mut self, colon: bool) -> Option<FixedOffset> {
        let sign = match self.s.get(self.i)? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        self.i += 1;
        let h = self.digits(2, 2)?;
        let mut rest = [0u64; 2];
        for part in &mut rest {
            let back = self.i;
            if colon && !self.lit(b':') {
                break;
            }
            match self.digits(2, 2) {
                Some(v) => *part = v,
                None => {
                    self.i = back;
                    break;
                }
            }
        }
        let [m, s] = rest;
        let total = h * 3600 + m * 60 + s;
        if h > 18 || m > 59 || s > 59 || total > 18 * 3600 {
            return None;
        }
        FixedOffset::east_opt(sign * total as i32)
    }

    fn word(&mut self, names: &[&str]) -> Option<usize> {
        let found = names.iter().position(|n| {
            self.s
                .get(self.i..self.i + n.len())
                .is_some_and(|w| w.eq_ignore_ascii_case(n.as_bytes()))
        })?;
        self.i += names[found].len();
        Some(found)
    }
}

/// Java `RFC_1123_DATE_TIME`(너그러운 숫자, SMART 해석): `[EEE, ]d MMM yyyy HH:mm[:ss] (GMT|+HHMM)`.
fn rfc_1123(s: &str) -> Option<DateTime<Utc>> {
    const DOW: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MOY: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut p = Cursor::new(s);
    let dow = p.word(&DOW).filter(|_| p.lit(b',') && p.lit(b' '));
    if dow.is_none() {
        p.i = 0;
    }
    let num = |p: &mut Cursor| p.digits(1, 19);
    let d = num(&mut p)?;
    p.lit(b' ').then_some(())?;
    let m = p.word(&MOY)? as u32 + 1;
    p.lit(b' ').then_some(())?;
    let y = num(&mut p)?;
    p.lit(b' ').then_some(())?;
    let h = num(&mut p)?;
    p.lit(b':').then_some(())?;
    let min = num(&mut p)?;
    let back = p.i;
    let sec = if p.lit(b':') { num(&mut p) } else { None };
    if sec.is_none() {
        p.i = back;
    }
    let sec = sec.unwrap_or(0);
    p.lit(b' ').then_some(())?;
    let off = if p.word(&["GMT"]).is_some() {
        FixedOffset::east_opt(0)
    } else {
        p.offset(false)
    }?;
    if !p.done() || !(1..=31).contains(&d) || y > 999_999_999 || h > 24 || min > 59 || sec > 59 {
        return None;
    }
    // SMART: 달 길이를 넘는 날은 그 달 마지막 날, 24:00:00은 다음 날 0시.
    // chrono 범위(±262143년) 밖은 Java와 달리 못 읽는다 — 연산은 전부 checked(panic 금지).
    let y = i32::try_from(y).ok()?;
    let date = (1..=d as u32)
        .rev()
        .find_map(|day| NaiveDate::from_ymd_opt(y, m, day))?;
    if dow.is_some_and(|w| w != date.weekday().num_days_from_monday() as usize) {
        return None;
    }
    let local = if h == 24 {
        (min == 0 && sec == 0).then_some(())?;
        date.checked_add_days(Days::new(1))?
            .and_time(NaiveTime::MIN)
    } else {
        date.and_hms_opt(h as u32, min as u32, sec as u32)?
    };
    Some(local.checked_sub_offset(off)?.and_utc())
}

fn bool_of(value: &Value) -> Option<bool> {
    match text(value, false).as_str() {
        "true" | "yes" | "y" | "예" | "1" => Some(true),
        "false" | "no" | "n" | "아니오" | "0" => Some(false),
        _ => None,
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        other => matches!(strip(&java_str(other)), "" | "[]" | "{}"),
    }
}
