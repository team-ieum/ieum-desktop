//! 뽑기 포맷터 2개 — BE `TextFormats.extract`·`extractPattern`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, mpsc};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fancy_regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use super::inputs::{In, MAX_ITEMS, MAX_PATTERN, R, invalid, too_large};
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
    find_all(re, &i.text("input")?, i.flag("all"), url, "input")
}

fn extract_pattern(i: &In, _: DateTime<Utc>) -> Out {
    let pattern = i.required("pattern", "정규식 패턴")?;
    if pattern.encode_utf16().count() > MAX_PATTERN {
        return Err(too_large("정규식 패턴이 200자를 넘어요"));
    }
    let re = RegexBuilder::new(&java_ascii(&pattern))
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
        .map_err(|e| invalid("pattern", format!("정규식 패턴 오류예요: {e}")))?;
    let (s, all) = (i.text("input")?, i.flag("all"));
    capped(move || find_all(&re, &s, all, false, "pattern"))
}

/// 앞·뒤 확인이 역추적마다 글 끝까지 훑으면 상한 안에서도 10초를 넘는다 — BE처럼 1초에서 끊는다.
// ponytail: 끊긴 스레드는 역추적 상한까지 혼자 돌다 끝난다(최악 십여 초 CPU) — 동시에 MAX_RUNNING개까지만 두고
// 넘으면 바로 실패시킨다. 취소가 필요하면 엔진 교체.
fn capped(job: impl FnOnce() -> Out + Send + 'static) -> Out {
    let guard = Running::start().ok_or_else(|| invalid("pattern", "패턴이 너무 복잡해요"))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _guard = guard;
        tx.send(job())
    });
    rx.recv_timeout(DEADLINE)
        .unwrap_or_else(|_| Err(invalid("pattern", "패턴이 너무 복잡해요")))
}

/// Java 기본값처럼 `\d`·`\w`·`\s`(와 부정)를 ASCII 클래스로 바꾼다. 글자 클래스 안에서도 중첩 클래스로 쓰인다.
fn java_ascii(p: &str) -> String {
    let mut res = String::with_capacity(p.len());
    let mut cs = p.chars();
    while let Some(c) = cs.next() {
        if c != '\\' {
            res.push(c);
            continue;
        }
        match cs.next() {
            Some('d') => res.push_str("[0-9]"),
            Some('D') => res.push_str("[^0-9]"),
            Some('w') => res.push_str("[0-9A-Za-z_]"),
            Some('W') => res.push_str("[^0-9A-Za-z_]"),
            Some('s') => res.push_str(r"[\t-\r ]"),
            Some('S') => res.push_str(r"[^\t-\r ]"),
            Some(n) => {
                res.push('\\');
                res.push(n);
            }
            None => res.push('\\'),
        }
    }
    res
}

/// Java `Matcher.find` 반복 — 빈 일치 뒤에는 한 글자 건너 다시 찾는다(바로 뒤 빈 일치도 센다).
fn find_all(re: &Regex, s: &str, all: bool, url: bool, field: &'static str) -> Out {
    let mut found = Vec::new();
    let mut pos = 0;
    while pos <= s.len() {
        let Some(m) = re
            .find_from_pos(s, pos)
            .map_err(|_| invalid(field, "패턴이 너무 복잡해요"))?
        else {
            break;
        };
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

    // 이 바이너리에서 capped를 부르는 테스트는 이것뿐이라 전역 카운터를 혼자 쓴다.
    // 정규식 폭주 대신 채널로 막아 둔 가짜 작업을 써서 빌드 방식(디버그·release) 속도와 무관하다.
    #[test]
    fn abandoned_threads_are_capped() {
        let mut releases = Vec::new();
        for n in 0..4 {
            let (release, wait) = mpsc::channel::<()>();
            releases.push(release);
            let t = Instant::now();
            let e = capped(move || {
                let _ = wait.recv();
                Ok(Map::new())
            })
            .unwrap_err();
            assert_eq!(
                (e.reason.as_str(), e.field),
                ("패턴이 너무 복잡해요", Some("pattern"))
            );
            let elapsed = t.elapsed();
            if n < MAX_RUNNING {
                assert!(elapsed >= DEADLINE, "{n}: {elapsed:?}");
            } else {
                assert!(elapsed < Duration::from_millis(100), "{n}: {elapsed:?}");
            }
            assert_eq!(RUNNING.load(Ordering::SeqCst), (n + 1).min(MAX_RUNNING));
        }
        drop(releases);
        let t = Instant::now();
        while RUNNING.load(Ordering::SeqCst) > 0 {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "버려진 스레드가 끝나지 않아요"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(pat("a1b22".into(), "[0-9]+").unwrap()["output"], json!("1"));
    }
}
