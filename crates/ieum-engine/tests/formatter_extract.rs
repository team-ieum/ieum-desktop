//! BE `TextFormatsTest`의 뽑기 포맷터 2개 케이스.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::{Map, Value, json};

const EX: &str = "formatter.text_extract";
const PAT: &str = "formatter.text_extract_pattern";

fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

fn cfg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn run(event: &str, input: Value) -> Value {
    apply(event, &cfg(input.clone()), now())
        .unwrap_or_else(|e| panic!("{event} {input}: {}", e.message))
        .remove("output")
        .unwrap()
}

fn case(event: &str, input: Value, want: Value) {
    assert_eq!(run(event, input.clone()), want, "{event} {input}");
}

fn fails(event: &str, input: Value, reason: &str, code: &str) {
    let e = apply(event, &cfg(input.clone()), now()).unwrap_err();
    assert!(e.message.contains(reason), "{event} {input}: {}", e.message);
    assert_eq!(e.code.as_deref(), Some(code), "{event} {input}");
}

/// 사용자 정규식은 동시에 2개까지만 돈다(버려진 스레드 포함) — 패턴 테스트끼리 겹치지 않게 줄 세운다.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn timed(limit: Duration, f: impl FnOnce()) -> Duration {
    let t = Instant::now();
    f();
    let d = t.elapsed();
    assert!(d < limit, "{d:?}");
    d
}

#[test]
fn text_extract() {
    let mail = "문의 a.b@x.co.kr 또는 c@d.com";
    case(
        EX,
        json!({"input": mail, "kind": "email"}),
        json!("a.b@x.co.kr"),
    );
    case(
        EX,
        json!({"input": mail, "kind": "email", "all": true}),
        json!(["a.b@x.co.kr", "c@d.com"]),
    );
    case(
        EX,
        json!({"input": "연락 010-1234-5678, 02)345-6789, 1588-1234", "kind": "phone", "all": true}),
        json!(["010-1234-5678", "02)345-6789", "1588-1234"]),
    );
    case(
        EX,
        json!({"input": "보기 https://ex.com/a?b=1.", "kind": "url"}),
        json!("https://ex.com/a?b=1"),
    );
    case(
        EX,
        json!({"input": "총 1,200원 중 3.5", "kind": "number", "all": true}),
        json!(["1,200", "3.5"]),
    );
    case(EX, json!({"input": "없음", "kind": "email"}), json!(""));
    case(
        EX,
        json!({"input": "없음", "kind": "email", "all": true}),
        json!([]),
    );
    fails(
        EX,
        json!({"input": "a"}),
        "'kind' 칸을 골라 주세요",
        "FIELD_EMPTY kind",
    );
    fails(
        EX,
        json!({"input": "a", "kind": "zip"}),
        "'kind' 값을 알 수 없어요",
        "FIELD_INVALID kind",
    );
}

#[test]
fn extract_phone_boundaries() {
    for s in [
        "결제카드 4579-1723-4567-8901 승인",
        "카드 4579-010-1234-5678",
        "계좌 110-012-3456-7890",
    ] {
        case(
            EX,
            json!({"input": s, "kind": "phone", "all": true}),
            json!([]),
        );
    }
    case(
        EX,
        json!({"input": "+82 010-1234-5678", "kind": "phone"}),
        json!("+82 010-1234-5678"),
    );
    case(
        EX,
        json!({"input": "02) 345-6789", "kind": "phone"}),
        json!("02) 345-6789"),
    );
}

#[test]
fn extract_url_brackets() {
    let url = |s: &str, want: &str| case(EX, json!({"input": s, "kind": "url"}), json!(want));
    url("홈페이지(https://ex.com/a)를 참고", "https://ex.com/a");
    url("[여기](https://ex.com/a)에서", "https://ex.com/a");
    url(
        "https://en.wikipedia.org/wiki/Java_(programming_language)",
        "https://en.wikipedia.org/wiki/Java_(programming_language)",
    );
    url("https://...", "");
}

#[test]
fn extract_email_linear() {
    timed(Duration::from_secs(5), || {
        let all = |s: String| {
            case(
                EX,
                json!({"input": s, "kind": "email", "all": true}),
                json!([]),
            )
        };
        all("a".repeat(90_000));
        all(format!("a@{}", "a.".repeat(44_990)));
    });
}

#[test]
fn extract_large_input_within_time() {
    timed(Duration::from_secs(5), || {
        let html = "<a href=x>".repeat(9_000);
        case(
            EX,
            json!({"input": html, "kind": "url", "all": true}),
            json!([]),
        );
        // 뒤 확인이 있는 전화 패턴도 긴 입력에서 역추적 상한에 걸리지 않는다.
        let digits = "1234-".repeat(19_999);
        case(
            EX,
            json!({"input": digits, "kind": "phone", "all": true}),
            json!([]),
        );
    });
}

#[test]
fn url_trailing_parens_linear() {
    timed(Duration::from_secs(1), || {
        let s = format!("https://a{}", ")".repeat(99_990));
        case(EX, json!({"input": s, "kind": "url"}), json!("https://a"));
    });
}

#[test]
fn text_extract_pattern() {
    let _g = serial();
    let s = "a 123-4567 b 765-4321";
    case(
        PAT,
        json!({"input": s, "pattern": r"\d{3}-\d{4}"}),
        json!("123-4567"),
    );
    case(
        PAT,
        json!({"input": s, "pattern": r"\d{3}-\d{4}", "all": true}),
        json!(["123-4567", "765-4321"]),
    );
    fails(
        PAT,
        json!({"input": s, "pattern": "a".repeat(201)}),
        "정규식 패턴이 200자를 넘어요",
        "TOO_LARGE",
    );
    fails(
        PAT,
        json!({"input": s}),
        "'정규식 패턴' 칸이 비어 있어요",
        "FIELD_EMPTY pattern",
    );
}

#[test]
fn bad_pattern_message() {
    let _g = serial();
    let e = apply(PAT, &cfg(json!({"input": "a", "pattern": "("})), now()).unwrap_err();
    assert!(
        e.message
            .starts_with("패턴으로 뽑기: 정규식 패턴 오류예요: "),
        "{}",
        e.message
    );
    assert_eq!(e.code.as_deref(), Some("FIELD_INVALID pattern"));
}

#[test]
fn ascii_digit_class() {
    let _g = serial();
    case(
        PAT,
        json!({"input": "１２3", "pattern": r"\d+"}),
        json!("3"),
    );
    case(
        PAT,
        json!({"input": "é_1 x", "pattern": r"\w+"}),
        json!("_1"),
    );
    case(
        PAT,
        json!({"input": "a\u{a0}b c", "pattern": r"\S+"}),
        json!("a\u{a0}b"),
    );
    case(
        PAT,
        json!({"input": "１a", "pattern": r"[\D]"}),
        json!("１"),
    );
}

/// BE(JDK 9+)는 `(a+)+$`를 메모이제이션으로 바로 끝내 빈 값을 낸다 — 러스트는 선형 엔진이라 같은 값.
#[test]
fn nested_quantifier_ends_fast_with_be_result() {
    let _g = serial();
    timed(Duration::from_secs(1), || {
        let input = format!("{}!", "a".repeat(30));
        case(PAT, json!({"input": input, "pattern": "(a+)+$"}), json!(""));
    });
}

/// 역추적 엔진이 도는 패턴(뒤 확인)에서 폭주하면 상한에 걸려 1초 안에 실패한다.
#[test]
fn runaway_pattern_ends_fast() {
    let _g = serial();
    let d = timed(Duration::from_secs(1), || {
        let input = format!("{}!", "a".repeat(30));
        fails(
            PAT,
            json!({"input": input, "pattern": "(a|aa)+(?<=a)$"}),
            "패턴이 너무 복잡해요",
            "FIELD_INVALID pattern",
        );
    });
    eprintln!("runaway_pattern_ends_fast: {d:?}");
}

/// 앞 확인이 역추적마다 글 끝까지 훑는 폭주는 역추적 상한 전에 1초 마감으로 끊긴다.
#[test]
fn runaway_lookahead_scan_hits_deadline() {
    let _g = serial();
    let d = timed(Duration::from_millis(1_500), || {
        let input = format!("{}{}", "a".repeat(30), "b".repeat(99_970));
        fails(
            PAT,
            json!({"input": input, "pattern": "(a|aa)+(?![ab]*$)"}),
            "패턴이 너무 복잡해요",
            "FIELD_INVALID pattern",
        );
    });
    eprintln!("runaway_lookahead_scan_hits_deadline: {d:?}");
}

/// 정상 패턴은 10만 자 글에서도 상한에 걸리지 않는다.
#[test]
fn normal_pattern_on_large_input() {
    let _g = serial();
    let d = timed(Duration::from_secs(5), || {
        let s = "123456789 ".repeat(10_000);
        for p in ["[0-9]+", r"(?<![0-9])[0-9]+(?![0-9])", r"(?<=\s|^)\d+"] {
            let got = run(PAT, json!({"input": s, "pattern": p, "all": true}));
            assert_eq!(got.as_array().unwrap().len(), 10_000, "{p}");
        }
        // 역참조는 찾기 한 번에 시작 위치마다 역추적이 쌓인다 — 10만 자에서도 상한 안.
        let words = "hello world and some text 123, foo_bar ".repeat(2_500);
        case(
            PAT,
            json!({"input": words, "pattern": r"(\w+)\s+\1", "all": true}),
            json!([]),
        );
        // 뒤 확인이 끝까지 안 맞아도 상한에 걸리지 않는다.
        case(
            PAT,
            json!({"input": "a".repeat(100_000), "pattern": "a(?=b)", "all": true}),
            json!([]),
        );
    });
    eprintln!("normal_pattern_on_large_input: {d:?}");
}

#[test]
fn result_list_cap() {
    let _g = serial();
    fails(
        PAT,
        json!({"input": "a".repeat(10_001), "pattern": "a", "all": true}),
        "결과 목록이 1만 개를 넘어요",
        "TOO_LARGE",
    );
    let got = run(
        PAT,
        json!({"input": "a".repeat(10_000), "pattern": "a", "all": true}),
    );
    assert_eq!(got.as_array().unwrap().len(), 10_000);
}

/// Java `Matcher.find`처럼 빈 일치 다음 칸에서도 다시 찾는다.
#[test]
fn empty_matches_like_java() {
    let _g = serial();
    case(
        PAT,
        json!({"input": "baa", "pattern": "a*", "all": true}),
        json!(["", "aa", ""]),
    );
}

/// BE는 Java 스택 넘침으로 실패하지만, 러스트는 선형 엔진이라 전체가 일치한다(GAPS).
#[test]
fn gap_java_stack_overflow_pattern_matches() {
    let _g = serial();
    let s = "ab".repeat(50_000);
    case(
        PAT,
        json!({"input": s.clone(), "pattern": "(a|b)*"}),
        json!(s),
    );
}

/// BE는 1초 마감으로 실패하지만, 러스트는 선형 엔진이라 빈 값으로 끝난다(GAPS).
#[test]
fn gap_java_timeout_pattern_ends() {
    let _g = serial();
    timed(Duration::from_secs(1), || {
        let input = format!("{}!", "a".repeat(40));
        case(
            PAT,
            json!({"input": input, "pattern": "(a{1,30}){1,30}$"}),
            json!(""),
        );
    });
}

/// 같은 패턴·입력을 JDK 21 `java.util.regex`(`Matcher.find` 반복)로 돌린 값.
#[test]
fn java_regex_table() {
    let _g = serial();
    let one = |p: &str, s: &str, want: &str| {
        case(PAT, json!({"input": s, "pattern": p}), json!(want));
    };
    let all = |p: &str, s: &str, want: Value| {
        case(PAT, json!({"input": s, "pattern": p, "all": true}), want);
    };
    // 줄 끝: 멀티라인이 아닌 달러는 글 끝 또는 마지막 줄 끝 문자 앞.
    one(r"\d+$", "abc 123\n", "123");
    one(r"\d+$", "abc 123\r\n", "123");
    one(r"^\w+$", "abc\n", "abc");
    one(r"a\r$", "a\r\n", "");
    one("a$", "a\r\n", "a");
    one("a$", "a\n\n", "");
    one(r"a\Z", "a\n", "a");
    one(r"a\Z", "a\n\n", "");
    one(r"a\z", "a\n", "");
    all("$", "", json!([""]));
    all("$", "a\n", json!(["", ""]));
    all("^", "", json!([""]));
    // 멀티라인: LF·CR·CRLF(하나로)·U+0085·U+2028·U+2029, 글 끝의 ^는 없다.
    all(r"(?m)^\d+$", "1\r\n2\r\n", json!(["1", "2"]));
    all(r"(?m)^\d*", "1\n2\n", json!(["1", "2"]));
    all("(?m)^", "", json!([]));
    all("(?m)$", "a\r\nb\u{85}c\u{2029}", json!(["", "", "", ""]));
    all("(?m)a$", "a\u{85}a", json!(["a", "a"]));
    all("(?m)^a", "a\rb\r\na\u{2028}a", json!(["a", "a", "a"]));
    // 점은 줄 끝 문자를 뺀다(DOTALL이면 전부).
    one(".+", "line1\r\nline2", "line1");
    one(".+", "a\u{2028}b", "a");
    one(".", "\u{85}", "");
    one("[^a]", "\n", "\n");
    one("(?s).+", "a\r\nb", "a\r\nb");
    one("(?s:.)b", "\nb", "\nb");
    all("(?s)a(?-s:.)", "a\n a\u{2028} ab", json!(["ab"]));
    // \b \B는 \w와 같은 ASCII 기준(JDK 19+).
    one(r"\b\d+\b", "가격123원", "123");
    one(r"\b\w+\b", "café", "caf");
    one(r"\Bb", "가b", "");
    one(r"\Bb", "ab", "b");
    all(r"\b", "", json!([]));
    all(r"\B", "", json!([""]));
    // 글자 그대로 쓴 메타 문자는 건드리지 않는다.
    one(r"\$\d+\.", "a $5. b", "$5.");
    one("[.$^]+", "a.$^b", ".$^");
    all(r"\\.", "a\\\nb\\c", json!(["\\c"]));
    one(r"[\d-z]+", "x5-zq", "5-z");
    one("[[:alpha:]]+", "xa:lpz", "a:lp");
    one("[a~~b]+", "a~b", "a~b");
    one("[]a]+", "]a", "]a");
    one("[^]a]+", "]ab", "b");
    one(r"\<a\>", "<a>", "<a>");
    one("[{]", "{", "{");
    one(r"\{", "{", "{");
    one("a}", "a}", "a}");
    one("a{2}", "aaa", "aa");
    one("a{2,}", "aaa", "aaa");
    all("a{1,2}", "aaa", json!(["aa", "a"]));
    one(r"[a&&[^b]]+", "ab", "a");
    // 뜻이 같은 문법.
    one("(?u)a", "a", "a");
    one("(?iu)é", "É", "É");
    one(r"\e", "\u{1b}", "\u{1b}");
    one(r"\a", "\u{7}", "\u{7}");
    one(r"\x41\x{42}C", "ABC", "ABC");
    one(r"\0101", "A", "A");
    one(r"(a)\1", "aa", "aa");
    one(r"(?<n>a)\k<n>", "aa", "aa");
    one("a++a", "aaa", "");
    one("(?>a+)b", "aab", "aab");
    one(r"(?<=가)\d", "가1", "1");
    // 유니코드 속성은 뜻이 같은 것만.
    one(r"\p{L}+", "a가b1", "a가b");
    one(r"\p{Lu}+", "aÀB", "ÀB");
    one(r"\pL+", "1가b", "가b");
    one(r"\p{IsHangul}+", "a가나b", "가나");
    one(r"\p{IsL}+", "1가b", "가b");
    one(r"\p{IsAlphabetic}+", "1가b", "가b");
    one(r"\p{sc=Hangul}+", "a가나", "가나");
    one(r"\p{gc=Lu}+", "aBC", "BC");
    one(r"\P{L}+", "ab12cd", "12");
    one(r"[\p{L}]+", "1가b", "가b");
    one(r"[\P{L}\P{N}]+", "a1.", "a1.");
    one(r"[\D\W]+", "a1.", "a");
    one(r"[^\P{L}]+", "1ab2", "ab");
    one("[[:-z]]+", "9:az", ":az");
    one("(?m:^a)", "b\na", "a");
    one("(a$)|b", "xa\n", "a");
    one(r"\x{1F600}", "😀", "😀");
    one(r"[\x{3a}]", ":", ":");
    one("(?i)[a-c]+", "ABC", "ABC");
}

/// Java와 뜻이 다르거나 옮길 수 없는 문법은 패턴 오류로 실패한다.
#[test]
fn java_regex_unsupported() {
    let _g = serial();
    for p in [
        r"\h+",
        r"\H",
        r"\v",
        r"\V",
        r"\R",
        r"\X",
        r"\G",
        r"\K",
        r"\N",
        r"\Qa.b\E",
        r"[\b]",
        r"[\h]",
        r"\p{Alpha}+",
        r"\p{Digit}",
        r"\p{Upper}",
        r"\p{Lower}",
        r"\p{Punct}",
        r"\p{Space}",
        r"\p{Alnum}",
        r"\p{javaLowerCase}",
        r"\p{IsAlpha}",
        r"\p{IsDigit}",
        r"\p{gc=Alpha}",
        r"[\p{Alpha}]",
        "x{,3}",
        "x{",
        "x{a}",
        "{",
        "[--/]",
        "(?d).",
        "(?x)a b",
        "(?U)a",
        "(?R)a",
    ] {
        let e = apply(PAT, &cfg(json!({"input": "a", "pattern": p})), now()).unwrap_err();
        assert!(
            e.message
                .starts_with("패턴으로 뽑기: 정규식 패턴 오류예요: "),
            "{p}: {}",
            e.message
        );
        assert_eq!(e.code.as_deref(), Some("FIELD_INVALID pattern"), "{p}");
    }
}

/// 번역 결과는 200자 상한 뒤에 길어져도 된다.
#[test]
fn translated_pattern_may_exceed_limit() {
    let _g = serial();
    let p = r"\b.".repeat(66);
    assert!(p.len() <= 200);
    let s = "a".repeat(66);
    case(PAT, json!({"input": s, "pattern": p}), json!(""));
    case(PAT, json!({"input": "가", "pattern": p}), json!(""));
}

/// Java는 대소문자 무시가 ASCII만이다 — 러스트는 유니코드까지 무시한다(GAPS).
#[test]
fn gap_case_insensitive_is_unicode() {
    let _g = serial();
    case(PAT, json!({"input": "É", "pattern": "(?i)é"}), json!("É"));
    case(
        PAT,
        json!({"input": "\u{212a}", "pattern": "(?i)k"}),
        json!("\u{212a}"),
    );
    case(PAT, json!({"input": "A", "pattern": "(?i)a"}), json!("A"));
}

/// Java는 서로게이트 쌍 가운데에서도 빈 일치를 센다(빈 값 3개) — 러스트는 글자 단위라 2개(GAPS).
#[test]
fn gap_empty_match_inside_surrogate_pair() {
    let _g = serial();
    case(
        PAT,
        json!({"input": "😀", "pattern": "x*", "all": true}),
        json!(["", ""]),
    );
}

/// Java \b는 글자·숫자 뒤의 결합 문자(Mn)를 단어 글자로 친다(경계 2개) — 러스트는 ASCII \w만(경계 4개, GAPS).
#[test]
fn gap_word_boundary_combining_mark() {
    let _g = serial();
    case(
        PAT,
        json!({"input": "e\u{301}x", "pattern": r"\b", "all": true}),
        json!(["", "", "", ""]),
    );
}

#[test]
fn empty_input() {
    let _g = serial();
    case(EX, json!({"input": "", "kind": "email"}), json!(""));
    case(
        EX,
        json!({"input": "", "kind": "email", "all": true}),
        json!([]),
    );
    case(PAT, json!({"input": "", "pattern": "a"}), json!(""));
}
