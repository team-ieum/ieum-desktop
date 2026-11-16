//! 뽑기 포맷터 2개 — BE `TextFormats.extract`·`extractPattern`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fancy_regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use super::inputs::{In, MAX_ITEMS, MAX_PATTERN, R, invalid, no_code, too_large};
use super::{Entry, out};

pub(crate) const OPS: &[Entry] = &[
    (
        "formatter.text_extract",
        "이메일·전화·URL·숫자 뽑기",
        extract,
    ),
    (
        "formatter.text_extract_pattern",
        "패턴으로 뽑기",
        extract_pattern,
    ),
];

type Out = R<Map<String, Value>>;

/// 사용자 패턴 역추적 상한(찾기 한 번에 누적) — 10만으로 낮추면 10만 자 글의 `(\w+)\s+\1` 같은 정상 패턴이 걸린다(실측).
const BACKTRACK_LIMIT: usize = 1_000_000;
const DEADLINE: Duration = Duration::from_secs(1);
/// 동시에 도는 사용자 정규식 스레드 상한 — 마감에 버려진 스레드가 쌓여 CPU를 다 쓰지 않게.
const MAX_RUNNING: usize = 2;
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// 정규식 스레드 하나 몫 — 스레드가 끝나면(패닉 포함) 놓인다.
struct Running;

impl Running {
    fn start() -> Option<Self> {
        RUNNING
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < MAX_RUNNING).then_some(n + 1)
            })
            .ok()
            .map(|_| Running)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 한국 전화 앞자리(0 뒤) — BE `TextFormats.AREA`.
pub(crate) const AREA: &str = "(?:2|1[016789]|[3-6][1-5]|70|80|50[0-9]?)";

// BE 패턴 그대로 — Java `\d`·`\s`는 ASCII라 클래스로 풀고, 소유격 `++`는 원자 그룹으로.
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    builtin(
        r"(?<![A-Za-z0-9._%+-])(?>[A-Za-z0-9._%+-]+)@[A-Za-z0-9-][A-Za-z0-9.-]*(?<=\.[A-Za-z]{2,63})",
    )
});
static PHONE: LazyLock<Regex> = LazyLock::new(|| {
    builtin(&format!(
        r"(?<![0-9])(?<![0-9]-)(?:(?:\+82[- ]?0?|0){AREA}(?:\) ?|[- .])?[0-9]{{3,4}}[- .]?[0-9]{{4}}|1[5-9][0-9]{{2}}-[0-9]{{4}})(?![0-9])(?!-[0-9])"
    ))
});
static URL: LazyLock<Regex> = LazyLock::new(|| builtin(r#"https?://[^\t-\r <>"']+"#));
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| builtin(r"-?[0-9]{1,3}(?:,[0-9]{3})+(?:\.[0-9]+)?|-?[0-9]+(?:\.[0-9]+)?"));
/// Java `Character.isLetterOrDigit`(BMP 한 글자).
static LETTER_OR_DIGIT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[\p{L}\p{Nd}]").unwrap());

fn builtin(p: &str) -> Regex {
    // 고정 패턴은 선형이라 상한을 두지 않는다 — 긴 입력에서 시작 위치마다 쌓인 역추적으로 실패하지 않게.
    RegexBuilder::new(p)
        .backtrack_limit(usize::MAX)
        .build()
        .unwrap()
}

fn extract(i: &In, _: DateTime<Utc>) -> Out {
    let re = match i.choice("kind", None, &["email", "phone", "url", "number"])? {
        "email" => &*EMAIL,
        "phone" => &*PHONE,
        "url" => &*URL,
        _ => &*NUMBER,
    };
    let url = std::ptr::eq(re, &*URL);
    find_all(re, &i.text("input")?, i.flag("all"), url, "input", None)
}

fn extract_pattern(i: &In, _: DateTime<Utc>) -> Out {
    let pattern = i.required("pattern", "정규식 패턴")?;
    if pattern.encode_utf16().count() > MAX_PATTERN {
        return Err(too_large("정규식 패턴이 200자를 넘어요"));
    }
    let (s, all) = (i.text("input")?, i.flag("all"));
    let bad = |e: String| invalid("pattern", format!("정규식 패턴 오류예요: {e}"));
    let (p, hard) = java_regex(&pattern, &s).map_err(bad)?;
    let re = RegexBuilder::new(&p)
        .backtrack_limit(if hard {
            BACKTRACK_LIMIT
        } else {
            EASY_BACKTRACK_LIMIT
        })
        .build()
        .map_err(|e| bad(e.to_string()))?;
    capped(move |deadline| find_all(&re, &s, all, false, "pattern", Some(deadline)))
}

/// 앞·뒤 확인이 역추적마다 글 끝까지 훑으면 상한 안에서도 10초를 넘는다 — BE처럼 1초에서 끊는다.
/// 작업은 마감 시각을 받아 찾기 사이마다 확인한다.
// ponytail: 끊긴 스레드는 진행 중인 찾기 한 번을 끝까지 돈다(역추적 상한까지, release 실측 최악 약 4.5초 CPU) —
// 동시에 MAX_RUNNING개까지만 두고 넘으면 바로 실패시킨다. 찾기 도중 취소가 필요하면 엔진 교체.
fn capped(job: impl FnOnce(Instant) -> Out + Send + 'static) -> Out {
    // 패턴 잘못이 아니다 — 코드를 비워 AI 빌더가 멀쩡한 패턴을 고치려 들지 않게.
    let guard = Running::start().ok_or_else(|| {
        no_code("다른 패턴 검사가 아직 끝나지 않았어요. 잠시 뒤 다시 실행해 주세요.")
    })?;
    let deadline = Instant::now() + DEADLINE;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _guard = guard;
        tx.send(job(deadline))
    });
    rx.recv_timeout(DEADLINE)
        .unwrap_or_else(|_| Err(invalid("pattern", "패턴이 너무 복잡해요")))
}

const W: &str = "[0-9A-Za-z_]";
const NOT_LINE_END: &str = r"[^\n\r\x{85}\x{2028}\x{2029}]";
// 줄 끝 기호 번역은 늘 비캡처 그룹 하나 — 뒤에 수량자가 붙어도 기호 전체에 붙게.
// 그룹 안이 둘러보기 하나뿐이면 fancy-regex가 반복을 거절하므로 둘러보기를 갈래로 나눈다.
/// 멀티라인이 아닌 `$`·`\Z` — 글 끝, 또는 마지막 줄 끝 문자 앞(CRLF 가운데는 아님).
const END: &str = r"(?:(?=\r\n\z)|(?=[\r\x{85}\x{2028}\x{2029}]?\z)|(?<!\r)(?=\n\z))";
const LINE_END: &str = r"(?:(?=[\r\x{85}\x{2028}\x{2029}])|(?<!\r)(?=\n)|\z)";
/// Java는 글 끝에서 `^`가 맞지 않는다(빈 글 포함).
const LINE_START: &str = r"(?:(?:\A|(?<=[\n\x{85}\x{2028}\x{2029}])|(?<=\r)(?!\n))(?!\z))";
/// 사용자 패턴에 역추적 문법이 없을 때의 상한 — 우리 번역(상수 폭 둘러보기)만으로는 폭주하지 않는다.
/// 10만 자 여러 줄 글(줄 80자)의 번역된 `.+$`가 release 약 0.18초(실측), 1천만은 부족.
const EASY_BACKTRACK_LIMIT: usize = 50_000_000;
/// 중첩 반복 횟수의 곱 — 반복 한 번마다 그 폭을 훑어 역추적 한 번이 비싸다(재리뷰 2 실측).
/// 넘으면 패턴 오류: 1백만 상한에서도 버려진 스레드가 수 초 넘게 돈다.
const MAX_REPEAT: u64 = 1_000;
/// 이 이상이면 5천만이 아니라 1백만 상한.
const HARD_REPEAT: u64 = 32;
/// 유니코드 일반 범주 — Java `\p{..}`와 뜻이 같다.
const CATEGORIES: &[&str] = &[
    "L", "Lu", "Ll", "Lt", "Lm", "Lo", "LC", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P",
    "Pc", "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "S", "Sm", "Sc", "Sk", "So", "Z", "Zs", "Zl", "Zp",
    "C", "Cc", "Cf", "Co", "Cs", "Cn",
];
/// `\p{Is..}`에서 러스트와 뜻이 같은 Java 이진 속성(대문자).
const SAME_BINARY: &[&str] = &[
    "ALPHABETIC",
    "LETTER",
    "LOWERCASE",
    "UPPERCASE",
    "WHITESPACE",
    "WHITE_SPACE",
    "IDEOGRAPHIC",
    "PUNCTUATION",
];
/// 그 밖에 Java가 아는 속성 이름(대문자) — POSIX(ASCII)·Java 고유 정의라 뜻이 다를 수 있다.
const JAVA_NAMES: &[&str] = &[
    "ALPHA",
    "LOWER",
    "UPPER",
    "SPACE",
    "PUNCT",
    "XDIGIT",
    "ALNUM",
    "CNTRL",
    "DIGIT",
    "BLANK",
    "GRAPH",
    "PRINT",
    "ASSIGNED",
    "CONTROL",
    "EMOJI",
    "EMOJI_PRESENTATION",
    "EMOJI_MODIFIER",
    "EMOJI_MODIFIER_BASE",
    "EMOJI_COMPONENT",
    "EXTENDED_PICTOGRAPHIC",
    "HEXDIGIT",
    "HEX_DIGIT",
    "JOINCONTROL",
    "JOIN_CONTROL",
    "NONCHARACTERCODEPOINT",
    "NONCHARACTER_CODE_POINT",
    "TITLECASE",
    "WORD",
    "L1",
    "LD",
    "ALL",
    "ASCII",
];

/// Java `Pattern` 문법을 fancy-regex로 옮긴다 — 뜻이 다른 문법은 `Err(설명)`.
/// 줄 끝·단어 경계를 둘러보기로 옮기면 역추적 엔진으로 돌아 느려지므로, 입력에 그 차이가 드러날 때만 옮긴다.
/// 둘째 값: 사용자 패턴 자체에 역추적 문법(둘러보기·역참조·원자 그룹·소유격)이 있는가.
fn java_regex(p: &str, input: &str) -> Result<(String, bool), String> {
    let other_end = input.contains(['\r', '\u{85}', '\u{2028}', '\u{2029}']);
    // 비 멀티라인 `$`·`\Z`: Java만 마지막 줄 끝 문자 앞에서도 맞는다 — 글이 줄 끝 문자로 끝날 때만 다르다.
    let end_differs = input.ends_with(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']);
    // 멀티라인 `$`: Java는 LF 말고 CR·U+0085·U+2028·U+2029 앞에서도 맞고 CRLF 가운데에선 안 맞는다.
    let line_end_differs = other_end;
    // 멀티라인 `^`: 그 줄 끝 문자 뒤에서도 맞고, 글 끝(빈 글·LF로 끝난 글)에선 Java만 안 맞는다.
    let line_start_differs = other_end || input.is_empty() || input.ends_with('\n');
    // 점(DOTALL 아님)은 늘 클래스로 옮긴다 — 둘러보기가 아니라 비용이 없다(줄 끝 문자 4개가 있을 때만 값이 다르다).
    let ascii = input.is_ascii();
    let cs: Vec<char> = p.chars().collect();
    let mut res = String::with_capacity(p.len() * 2);
    // 그룹마다 (멀티라인, DOTALL).
    let mut flags = vec![(false, false)];
    // 그룹마다 안에서 가장 큰 반복 횟수 곱 / 바로 앞 토큰의 곱 — 거친 상한이다.
    let mut reps = vec![1u64];
    let mut last = 1u64;
    let mut class = 0usize;
    let mut hard = false;
    // 바로 앞 토큰이 수량자인가 / 클래스에서 지금 `&&`의 한쪽이 비었는가(여는 괄호·`&&` 바로 뒤).
    let (mut quant, mut side_empty) = (false, false);
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        i += 1;
        let prev = std::mem::replace(&mut last, 1);
        let (multi, dotall) = *flags.last().unwrap();
        let after_quant = std::mem::take(&mut quant);
        let empty = std::mem::take(&mut side_empty);
        if c == '\\' {
            let Some(&n) = cs.get(i) else {
                return Err("패턴이 역슬래시로 끝나요".into());
            };
            i += 1;
            match n {
                'd' => res.push_str("[0-9]"),
                'D' => res.push_str("[^0-9]"),
                'w' => res.push_str(W),
                'W' => res.push_str("[^0-9A-Za-z_]"),
                's' => res.push_str(r"[\t-\r ]"),
                'S' => res.push_str(r"[^\t-\r ]"),
                'p' | 'P' => {
                    let end = if cs.get(i) == Some(&'{') {
                        cs[i..].iter().position(|&c| c == '}').map(|e| i + e + 1)
                    } else {
                        cs.get(i).map(|_| i + 1)
                    };
                    let Some(end) = end else {
                        return Err(format!("\\{n} 뒤 속성 이름이 없어요"));
                    };
                    let name: String = cs[i..end]
                        .iter()
                        .filter(|&&c| c != '{' && c != '}')
                        .collect();
                    if !same_property(&name) {
                        return Err(format!("Java와 뜻이 다른 속성이에요: \\{n}{{{name}}}"));
                    }
                    res.push('\\');
                    res.extend(&cs[i - 1..end]);
                    i = end;
                }
                '0' => {
                    let oct = |k: usize| cs.get(k).and_then(|c| c.to_digit(8));
                    let Some(a) = oct(i) else {
                        return Err("\\0 뒤 8진수가 없어요".into());
                    };
                    let (mut v, mut k) = (a, i + 1);
                    if let Some(b) = oct(k) {
                        v = v * 8 + b;
                        k += 1;
                        if let Some(c) = oct(k).filter(|_| a <= 3) {
                            v = v * 8 + c;
                            k += 1;
                        }
                    }
                    res.push_str(&format!(r"\x{{{v:x}}}"));
                    i = k;
                }
                'x' if cs.get(i) == Some(&'{') => {
                    let end = cs[i..]
                        .iter()
                        .position(|&c| c == '}')
                        .map_or(cs.len(), |e| i + e + 1);
                    res.push_str(r"\x");
                    res.extend(&cs[i..end]);
                    i = end;
                }
                '1'..='9' | 't' | 'n' | 'r' | 'f' | 'a' | 'e' | 'x' | 'u' => {
                    hard |= n.is_ascii_digit() && class == 0;
                    res.push('\\');
                    res.push(n);
                }
                'A' | 'z' | 'k' if class == 0 => {
                    hard |= n == 'k';
                    res.push('\\');
                    res.push(n);
                }
                'Z' if class == 0 => res.push_str(if end_differs { END } else { r"\z" }),
                'b' if class == 0 => res.push_str(if ascii {
                    r"\b"
                } else {
                    "(?:(?<=[0-9A-Za-z_])(?![0-9A-Za-z_])|(?<![0-9A-Za-z_])(?=[0-9A-Za-z_]))"
                }),
                'B' if class == 0 => res.push_str(if ascii {
                    r"\B"
                } else {
                    "(?:(?<=[0-9A-Za-z_])(?=[0-9A-Za-z_])|(?<![0-9A-Za-z_])(?![0-9A-Za-z_]))"
                }),
                n if n.is_ascii_alphanumeric() => {
                    return Err(format!("지원하지 않는 이스케이프예요: \\{n}"));
                }
                // 글자 그대로 — `\<` 같은 fancy-regex 고유 뜻을 피한다.
                n => res.push_str(&format!(r"\x{{{:x}}}", u32::from(n))),
            }
            continue;
        }
        if class > 0 {
            match c {
                '[' => {
                    class += 1;
                    res.push('[');
                    if cs.get(i) == Some(&'^') {
                        res.push('^');
                        i += 1;
                    }
                    // 여는 괄호 바로 뒤 `]`는 글자다.
                    side_empty = cs.get(i) != Some(&']');
                    if !side_empty {
                        res.push_str(r"\x{5d}");
                        i += 1;
                    }
                    // Java의 `[:alpha:]`는 글자 묶음 — 러스트가 POSIX 클래스로 읽지 않게 첫 `:`를 범위로 쓴다
                    // (fancy-regex가 클래스 안 이스케이프를 풀어 다시 쓰므로 `\x{3a}`로는 안 된다).
                    let posix = cs[i..]
                        .iter()
                        .skip(1)
                        .position(|&c| !c.is_ascii_alphabetic() && c != '^');
                    if cs.get(i) == Some(&':')
                        && posix.is_some_and(|k| cs[i + 1 + k..].starts_with(&[':', ']']))
                    {
                        res.push_str(":-:");
                        side_empty = false;
                        i += 1;
                    }
                }
                // Java는 `&&`의 빈 쪽을 무시한다(`[a&&]` = `[a]`) — 러스트와 값이 달라 거절.
                ']' | '&' if empty && (c == ']' || cs.get(i) == Some(&'&')) => {
                    return Err("문자 클래스의 && 양쪽에 글자가 있어야 해요".into());
                }
                ']' => {
                    class -= 1;
                    res.push(']');
                }
                '&' if cs.get(i) == Some(&'&') => {
                    res.push_str("&&");
                    side_empty = true;
                    i += 1;
                }
                '-' if cs.get(i) == Some(&'-') => {
                    return Err("문자 클래스 안의 --는 지원하지 않아요".into());
                }
                '^' | '-' | '&' => res.push(c),
                // `[:alpha:]`·`~~`처럼 러스트에서만 뜻이 있는 글자를 글자 그대로.
                c if c.is_ascii_punctuation() => {
                    res.push_str(&format!(r"\x{{{:x}}}", u32::from(c)));
                }
                c => res.push(c),
            }
            continue;
        }
        match c {
            '[' => {
                class = 1;
                res.push('[');
                if cs.get(i) == Some(&'^') {
                    res.push('^');
                    i += 1;
                }
                side_empty = cs.get(i) != Some(&']');
                if !side_empty {
                    res.push_str(r"\x{5d}");
                    i += 1;
                }
            }
            '(' => {
                let mut f = (multi, dotall);
                if cs.get(i) == Some(&'?')
                    && cs
                        .get(i + 1)
                        .is_some_and(|c| c.is_ascii_alphabetic() || *c == '-')
                {
                    let mut k = i + 1;
                    let mut on = true;
                    while let Some(&c) = cs.get(k).filter(|c| !matches!(c, ')' | ':')) {
                        match c {
                            '-' => on = false,
                            'm' => f.0 = on,
                            's' => f.1 = on,
                            'i' | 'u' => {}
                            // 끄는 쪽은 Java도 기본값이라 뜻이 같다.
                            'x' if !on => {}
                            c => return Err(format!("지원하지 않는 플래그예요: {c}")),
                        }
                        k += 1;
                    }
                    res.extend(&cs[i - 1..k]);
                    i = k;
                    if cs.get(k) == Some(&')') {
                        *flags.last_mut().unwrap() = f;
                        res.push(')');
                        i += 1;
                        continue;
                    }
                } else if cs.get(i) == Some(&'?') {
                    // `(?=` `(?!` `(?<=` `(?<!` `(?>` — 사용자 둘러보기·원자 그룹.
                    let k: String = cs[i + 1..].iter().take(2).collect();
                    let look = k.starts_with(['=', '!', '>']) || k == "<=" || k == "<!";
                    let named =
                        k.starts_with('<') && k[1..].starts_with(|c: char| c.is_ascii_alphabetic());
                    // Java가 받는 모양만 — 나머지(`(?~` `(?(` `(?#` `(?'`)는 러스트가 다르게 읽는다.
                    if !(look || named || k.starts_with(':')) {
                        return Err(format!("Java에 없는 그룹 문법이에요: (?{k}"));
                    }
                    hard |= look;
                    res.push_str("(?");
                    i += 1;
                } else if cs.get(i) == Some(&'*') {
                    return Err("Java에 없는 그룹 문법이에요: (*".into());
                } else {
                    res.push('(');
                }
                flags.push(f);
                reps.push(1);
            }
            ')' => {
                if flags.len() > 1 {
                    flags.pop();
                    last = reps.pop().unwrap();
                    let top = reps.last_mut().unwrap();
                    *top = (*top).max(last);
                }
                res.push(')');
            }
            '.' => res.push_str(if dotall { "." } else { NOT_LINE_END }),
            '^' => res.push_str(if multi && line_start_differs {
                LINE_START
            } else {
                "^"
            }),
            '$' => res.push_str(match multi {
                true if line_end_differs => LINE_END,
                false if end_differs => END,
                _ => "$",
            }),
            // Java는 `x{1}{2}`·`a*{2}`를 겹쳐 반복한다 — 러스트와 값이 달라 거절.
            '{' if after_quant => return Err("반복 횟수 앞에 다른 수량자가 있어요".into()),
            '{' => {
                let end = cs[i..].iter().position(|&c| c == '}').map(|e| i + e);
                let body: String = end.map_or_else(String::new, |e| cs[i..e].iter().collect());
                let (lo, hi) = body.split_once(',').unwrap_or((&body, "0"));
                let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
                if lo.is_empty() || !digits(lo) || !digits(hi) {
                    return Err("{ 뒤는 반복 횟수({n}·{n,}·{n,m})여야 해요".into());
                }
                res.push('{');
                res.push_str(&body);
                res.push('}');
                i = end.unwrap() + 1;
                quant = true;
                let n = match body.split_once(',') {
                    Some((_, hi)) if !hi.is_empty() => hi,
                    _ => lo,
                };
                last = prev.saturating_mul(n.parse::<u64>().unwrap_or(u64::MAX).max(1));
                if last > MAX_REPEAT {
                    return Err("반복 횟수가 너무 커요".into());
                }
                hard |= last >= HARD_REPEAT;
                let top = reps.last_mut().unwrap();
                *top = (*top).max(last);
            }
            '*' | '+' | '?' => {
                // 수량자 바로 뒤 `+`는 소유격.
                hard |= after_quant && c == '+';
                quant = true;
                last = prev;
                res.push(c);
            }
            c => res.push(c),
        }
    }
    Ok((res, hard))
}

/// `\p{name}`이 Java와 같은 글자를 고르는가 — 일반 범주·문자 체계·뜻이 같은 이진 속성만.
fn same_property(name: &str) -> bool {
    if let Some((key, value)) = name.split_once('=') {
        return match key.to_ascii_lowercase().as_str() {
            "gc" | "general_category" => CATEGORIES.contains(&value),
            _ => true,
        };
    }
    if CATEGORIES.contains(&name) {
        return true;
    }
    let Some(rest) = name.strip_prefix("Is") else {
        // 접두사 없는 이름은 일반 범주 말고는 POSIX(ASCII)·java 이름이다. `In..`(블록)은 러스트가 거절한다.
        return name.starts_with("In");
    };
    let up = rest.to_uppercase();
    CATEGORIES.contains(&rest)
        || SAME_BINARY.contains(&up.as_str())
        || !(JAVA_NAMES.contains(&up.as_str()) || up.starts_with("JAVA"))
}

/// Java `Matcher.find` 반복 — 빈 일치 뒤에는 한 글자 건너 다시 찾는다(바로 뒤 빈 일치도 센다).
fn find_all(
    re: &Regex,
    s: &str,
    all: bool,
    url: bool,
    field: &'static str,
    deadline: Option<Instant>,
) -> Out {
    let mut found = Vec::new();
    let mut pos = 0;
    while pos <= s.len() {
        let Some(m) = re
            .find_from_pos(s, pos)
            .map_err(|_| invalid(field, "패턴이 너무 복잡해요"))?
        else {
            break;
        };
        // 마감에 버려진 스레드가 남은 찾기를 끝까지 돌지 않게.
        if deadline.is_some_and(|d| Instant::now() > d) {
            return Err(invalid(field, "패턴이 너무 복잡해요"));
        }
        pos = m.end()
            + if m.start() == m.end() {
                s[m.end()..].chars().next().map_or(1, char::len_utf8)
            } else {
                0
            };
        let g = if url {
            match trim_url(m.as_str()) {
                Some(g) => g,
                None => continue,
            }
        } else {
            m.as_str()
        };
        if !all {
            return Ok(out(g));
        }
        found.push(Value::from(g));
        if found.len() > MAX_ITEMS {
            return Err(too_large("결과 목록이 1만 개를 넘어요"));
        }
    }
    Ok(out(if all { Value::from(found) } else { "".into() }))
}

/// BE `trimUrl` — 닫는 괄호 뒤 ASCII 밖 글자(조사 등)부터 버리고, 끝 문장부호와 짝 없는 `)`·`]`를 뗀다. 호스트가 없으면 None.
fn trim_url(g: &str) -> Option<&str> {
    let mut g = g;
    let mut it = g.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if matches!(c, ')' | ']') && it.peek().is_some_and(|&(_, n)| n > '\u{7f}') {
            g = &g[..=i];
            break;
        }
    }
    let bal = |close, open| {
        g.bytes()
            .map(|c| i32::from(c == close) - i32::from(c == open))
            .sum::<i32>()
    };
    let (mut parens, mut brackets) = (bal(b')', b'('), bal(b']', b'['));
    let b = g.as_bytes();
    let mut end = b.len();
    while end > 0 {
        let c = b[end - 1];
        if b".,!?;:".contains(&c) || c == b')' && parens > 0 || c == b']' && brackets > 0 {
            parens -= i32::from(c == b')');
            brackets -= i32::from(c == b']');
            end -= 1;
        } else {
            break;
        }
    }
    let g = &g[..end];
    let rest = &g[g.find("://")? + 3..];
    // Java는 UTF-16 한 칸을 보므로 BMP 밖 글자(서로게이트)는 글자로 치지 않는다.
    let first = rest.chars().next()?;
    (u32::from(first) <= 0xFFFF && LETTER_OR_DIGIT.is_match(rest)).then_some(g)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use serde_json::json;

    use super::*;

    fn pat(input: String, pattern: &str) -> Out {
        let m = json!({"input": input, "pattern": pattern});
        extract_pattern(&In(m.as_object().unwrap()), Utc::now())
    }

    /// 이 바이너리에서 capped를 부르는 테스트끼리 전역 카운터를 나눠 쓰지 않게 줄 세운다.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait_idle(limit: Duration) -> Duration {
        let t = Instant::now();
        while RUNNING.load(Ordering::SeqCst) > 0 {
            assert!(t.elapsed() < limit, "버려진 스레드가 끝나지 않아요");
            thread::sleep(Duration::from_millis(10));
        }
        t.elapsed()
    }

    // 정규식 폭주 대신 채널로 막아 둔 가짜 작업을 써서 빌드 방식(디버그·release) 속도와 무관하다.
    #[test]
    fn abandoned_threads_are_capped() {
        let _g = serial();
        let mut releases = Vec::new();
        for n in 0..4 {
            let (release, wait) = mpsc::channel::<()>();
            releases.push(release);
            let t = Instant::now();
            let e = capped(move |_| {
                let _ = wait.recv();
                Ok(Map::new())
            })
            .unwrap_err();
            let elapsed = t.elapsed();
            if n < MAX_RUNNING {
                assert_eq!(
                    (e.reason.as_str(), e.code, e.field),
                    ("패턴이 너무 복잡해요", "FIELD_INVALID", Some("pattern"))
                );
                assert!(elapsed >= DEADLINE, "{n}: {elapsed:?}");
            } else {
                assert_eq!(
                    (e.reason.as_str(), e.code, e.field),
                    (
                        "다른 패턴 검사가 아직 끝나지 않았어요. 잠시 뒤 다시 실행해 주세요.",
                        "",
                        None
                    )
                );
                assert!(elapsed < Duration::from_millis(100), "{n}: {elapsed:?}");
            }
            assert_eq!(RUNNING.load(Ordering::SeqCst), (n + 1).min(MAX_RUNNING));
        }
        // 자리가 찼을 때 노드 실패에는 코드가 없다(빌더가 패턴을 고치려 들지 않게).
        let cfg = json!({"input": "a", "pattern": "a"});
        let e = crate::formatter::apply(
            "formatter.text_extract_pattern",
            cfg.as_object().unwrap(),
            Utc::now(),
        )
        .unwrap_err();
        assert!(e.message.contains("다른 패턴 검사가"), "{}", e.message);
        assert_eq!(e.code, None);
        drop(releases);
        wait_idle(Duration::from_secs(10));
        assert_eq!(pat("a1b22".into(), "[0-9]+").unwrap()["output"], json!("1"));
    }

    /// 찾기 한 번은 상한 안이지만 찾기가 수천 번인 폭주(all 켬) — 마감 뒤 버려진 스레드는 찾기 한 번 안에 멈춘다.
    #[test]
    fn abandoned_find_loop_stops_at_deadline() {
        let _g = serial();
        // 블록이 짧아 찾기 한 번은 짧고(debug 1초 안팎), 찾기 수천 번을 다 돌면 release에서도 수십 초다.
        let input = format!("{}x", "a".repeat(10)).repeat(9_090);
        let t = Instant::now();
        let m = json!({"input": input, "pattern": "(?:(a|aa)+(?![ax]*$)|x)", "all": true});
        let e = extract_pattern(&In(m.as_object().unwrap()), Utc::now()).unwrap_err();
        let failed = t.elapsed();
        assert_eq!(
            (e.reason.as_str(), e.field),
            ("패턴이 너무 복잡해요", Some("pattern"))
        );
        assert!(failed < Duration::from_secs(3), "{failed:?}");
        let idle = wait_idle(Duration::from_secs(10));
        eprintln!("abandoned_find_loop_stops_at_deadline: 실패 {failed:?}, 스레드 정리 {idle:?}");
        assert_eq!(pat("a1b22".into(), "[0-9]+").unwrap()["output"], json!("1"));
    }
}
