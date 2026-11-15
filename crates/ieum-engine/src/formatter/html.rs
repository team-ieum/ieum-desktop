//! HTML 포맷터 2개 — BE `TextFormats.stripHtml`·`markdownToHtml`(jsoup 1.23.2·commonmark 0.30.0).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};
use chrono::{DateTime, Utc};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use scraper::{Html, Node, Selector};
use serde_json::{Map, Value};

use super::inputs::{In, R, too_large};
use super::{Entry, out};
use crate::condition::strip;

pub(crate) const OPS: &[Entry] = &[
    ("formatter.text_strip_html", "HTML 태그 없애기", strip_html),
    (
        "formatter.text_markdown_to_html",
        "Markdown을 HTML로",
        markdown_to_html,
    ),
];

type Out = R<Map<String, Value>>;

/// jsoup 1.23.2 `Tag.isBlock()`이 참인 HTML 요소(파서 문맥에서 svg·math도 블록).
const BLOCK: &[&str] = &[
    "address",
    "applet",
    "article",
    "aside",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "li",
    "link",
    "listing",
    "main",
    "marquee",
    "menu",
    "meta",
    "nav",
    "noframes",
    "noscript",
    "ol",
    "p",
    "plaintext",
    "pre",
    "script",
    "search",
    "section",
    "style",
    "table",
    "tbody",
    "td",
    "template",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
];

fn is_block(e: &scraper::node::Element) -> bool {
    let name = e.name();
    if &*e.name.ns == "http://www.w3.org/1999/xhtml" {
        BLOCK.contains(&name)
    } else {
        matches!(name, "svg" | "math")
    }
}

/// jsoup `TextNode.text()` — HTML 공백(NBSP 포함) 덩어리는 한 칸, U+200B·U+00AD는 버린다.
fn push_text(sb: &mut String, s: &str) {
    let mut white = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r' | '\u{a0}') {
            if !white {
                sb.push(' ');
            }
            white = true;
        } else if !matches!(c, '\u{200b}' | '\u{ad}') {
            sb.push(c);
            white = false;
        }
    }
}

fn strip_html(i: &In, _: DateTime<Utc>) -> Out {
    let doc = Html::parse_document(&i.text("input")?);
    let root = Selector::parse("body, frameset").expect("고정 선택자");
    let mut sb = String::new();
    // 깊게 중첩된 입력에서 재귀로 스택이 넘치지 않게 직접 쌓는다.
    let mut stack: Vec<_> = doc.select(&root).take(1).map(|b| (*b, false)).collect();
    while let Some((n, close)) = stack.pop() {
        match n.value() {
            Node::Text(t) => push_text(&mut sb, &t.text),
            Node::Element(_) if close => sb.push('\n'),
            Node::Element(e) => {
                // jsoup이 내용을 DataNode로 두는 요소 — 글에서 빠진다.
                if matches!(e.name(), "script" | "style" | "iframe" | "xmp" | "noembed") {
                    continue;
                }
                let block = is_block(e);
                if block || e.name() == "br" {
                    sb.push('\n');
                }
                if block {
                    stack.push((n, true));
                }
                stack.extend(n.children().rev().map(|c| (c, false)));
            }
            _ => {}
        }
    }
    let lines: Vec<&str> = sb
        .split('\n')
        .map(strip)
        .filter(|l| !l.is_empty())
        .collect();
    Ok(out(lines.join("\n")))
}

/// jsoup `Safelist.relaxed()`의 태그·속성.
const RELAXED: &[(&str, &[&str])] = &[
    ("a", &["href", "title"]),
    ("b", &[]),
    ("blockquote", &["cite"]),
    ("br", &[]),
    ("caption", &[]),
    ("cite", &[]),
    ("code", &[]),
    ("col", &["span", "width"]),
    ("colgroup", &["span", "width"]),
    ("dd", &[]),
    ("div", &[]),
    ("dl", &[]),
    ("dt", &[]),
    ("em", &[]),
    ("h1", &[]),
    ("h2", &[]),
    ("h3", &[]),
    ("h4", &[]),
    ("h5", &[]),
    ("h6", &[]),
    ("i", &[]),
    ("img", &["align", "alt", "height", "src", "title", "width"]),
    ("li", &[]),
    ("ol", &["start", "type"]),
    ("p", &[]),
    ("pre", &[]),
    ("q", &["cite"]),
    ("small", &[]),
    ("span", &[]),
    ("strike", &[]),
    ("strong", &[]),
    ("sub", &[]),
    ("sup", &[]),
    ("table", &["summary", "width"]),
    ("tbody", &[]),
    ("td", &["abbr", "axis", "colspan", "rowspan", "width"]),
    ("tfoot", &[]),
    (
        "th",
        &["abbr", "axis", "colspan", "rowspan", "scope", "width"],
    ),
    ("thead", &[]),
    ("tr", &[]),
    ("u", &[]),
    ("ul", &["type"]),
];

/// 주소 속성이 허용하는 스킴 — commonmark `sanitizeUrls`(http·https·mailto·data)와
/// relaxed 프로토콜(a: ftp·http·https·mailto, 그 밖: http·https)을 둘 다 통과해야 남는다.
/// 상대 주소·`tel:`·`ftp:`는 속성째 빠진다.
fn schemes(tag: &str, attr: &str) -> Option<&'static [&'static str]> {
    match (tag, attr) {
        ("a", "href") => Some(&["http", "https", "mailto"]),
        ("img", "src") | ("blockquote", "cite") | ("q", "cite") => Some(&["http", "https"]),
        _ => None,
    }
}

/// 허용 스킴이면 jsoup `absUrl`처럼 스킴만 소문자로.
fn url_filter<'u>(tag: &str, attr: &str, value: &'u str) -> Option<Cow<'u, str>> {
    let Some(allowed) = schemes(tag, attr) else {
        return Some(value.into());
    };
    let (scheme, rest) = value.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    allowed
        .contains(&scheme.as_str())
        .then(|| format!("{scheme}:{rest}").into())
}

fn sanitizer() -> Builder<'static> {
    let mut b = Builder::empty();
    b.tags(RELAXED.iter().map(|(t, _)| *t).collect())
        .tag_attributes(
            RELAXED
                .iter()
                .map(|(t, a)| (*t, a.iter().copied().collect()))
                .collect::<HashMap<_, HashSet<_>>>(),
        )
        .generic_attributes(HashSet::new())
        .url_schemes(["http", "https", "mailto"].into())
        .url_relative(UrlRelative::Deny)
        .link_rel(None)
        .attribute_filter(url_filter);
    b
}

/// commonmark-java는 열린 블록 파서 상한(maxOpenBlockParsers 100)을 넘는 중첩을 잘라 내(+안쪽 문단) 결과가 달라지고,
/// html5ever는 깊을수록 크게 느려진다 — 그 전에 실패.
const MAX_DEPTH: usize = 101;

/// commonmark `escapeHtml(true)` — 원문 HTML은 글로, HTML 블록은 `<p>`로 감싼다(끝 줄바꿈 하나는 뗀다).
fn escape_raw_html(src: &str) -> R<Vec<Event<'_>>> {
    let mut it = Parser::new(src).peekable();
    let mut evs = Vec::new();
    let mut depth = 0;
    while let Some(e) = it.next() {
        match e {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        if depth > MAX_DEPTH {
            return Err(too_large("Markdown 중첩이 너무 깊어요"));
        }
        evs.push(match e {
            Event::Start(Tag::HtmlBlock) => Event::Start(Tag::Paragraph),
            Event::End(TagEnd::HtmlBlock) => Event::End(TagEnd::Paragraph),
            Event::Html(s) if matches!(it.peek(), Some(Event::End(TagEnd::HtmlBlock))) => {
                Event::Text(s.strip_suffix('\n').unwrap_or(&s).to_string().into())
            }
            Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
            e => e,
        });
    }
    Ok(evs)
}

fn markdown_to_html(i: &In, _: DateTime<Utc>) -> Out {
    let src = i.text("input")?;
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, escape_raw_html(&src)?.into_iter());
    Ok(out(strip(&sanitizer().clean(&html).to_string())))
}
