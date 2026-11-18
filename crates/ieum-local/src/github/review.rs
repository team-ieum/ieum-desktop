//! PR 변경 내용·리뷰 노드 — agent `github_get_pr_files`·`github_get_pr_reviews`·`github_submit_pr_review`를 옮겼다.
//! 입력 검사를 모두 끝낸 뒤 토큰을 읽는다.

use std::collections::{HashMap, HashSet};

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{issue_no, text, to_int};
use super::issues::{get, object, repo_path};

pub(crate) const DEFAULT_EXCLUDE_PATHS: &str =
    "*.lock, package-lock.json, yarn.lock, pnpm-lock.yaml, dist/**, build/**, *.min.js, *.map";
/// 결과 크기 상한 — 100개씩 10쪽 = 1000개(agent `_MAX_FILE_PAGES`).
pub(crate) const MAX_PAGES: usize = 10;
const FILE_META_KEYS: [&str; 5] = ["filename", "status", "additions", "deletions", "changes"];

enum Tok {
    Star,
    Any,
    Lit(char),
    /// `[!...]`면 true, 범위 목록(낱글자는 `(c, c)`). `lo > hi`인 빈 범위는 아무것도 맞지 않는다.
    Set(bool, Vec<(char, char)>),
}

/// Python `fnmatch.translate`와 같게 패턴을 나눈다. 닫히지 않은 `[`는 글자 `[`, 첫 `]`(`!` 뒤도)는 글자.
fn compile(pat: &str) -> Vec<Tok> {
    let p: Vec<char> = pat.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < p.len() {
        let c = p[i];
        i += 1;
        match c {
            '*' => {
                if !matches!(out.last(), Some(Tok::Star)) {
                    out.push(Tok::Star);
                }
            }
            '?' => out.push(Tok::Any),
            '[' => {
                let mut j = i;
                if p.get(j) == Some(&'!') {
                    j += 1;
                }
                if p.get(j) == Some(&']') {
                    j += 1;
                }
                while j < p.len() && p[j] != ']' {
                    j += 1;
                }
                if j >= p.len() {
                    out.push(Tok::Lit('['));
                    continue;
                }
                let mut body = &p[i..j];
                i = j + 1;
                let neg = body.first() == Some(&'!');
                if neg {
                    body = &body[1..];
                }
                let (mut set, mut k) = (Vec::new(), 0);
                while k < body.len() {
                    if k + 2 < body.len() && body[k + 1] == '-' {
                        set.push((body[k], body[k + 2]));
                        k += 3;
                    } else {
                        set.push((body[k], body[k]));
                        k += 1;
                    }
                }
                out.push(Tok::Set(neg, set));
            }
            c => out.push(Tok::Lit(c)),
        }
    }
    out
}

fn one(t: &Tok, c: char) -> bool {
    match t {
        Tok::Star | Tok::Any => true,
        Tok::Lit(l) => *l == c,
        Tok::Set(neg, set) => set.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)) != *neg,
    }
}

/// Python `fnmatch.fnmatchcase`와 같은 뜻 — 전체 일치, 대소문자 구분, `*`·`?`는 `/`도 넘는다.
pub(crate) fn fnmatch(name: &str, pat: &str) -> bool {
    let (toks, s): (_, Vec<char>) = (compile(pat), name.chars().collect());
    // `*`만 길이가 변하므로 마지막 `*` 자리로 되돌아가는 반복으로 충분하다.
    let (mut t, mut i, mut back) = (0, 0, None);
    while i < s.len() {
        match toks.get(t) {
            Some(Tok::Star) => {
                back = Some((t + 1, i));
                t += 1;
            }
            Some(tok) if one(tok, s[i]) => {
                t += 1;
                i += 1;
            }
            _ => match back {
                Some((bt, bi)) => {
                    back = Some((bt, bi + 1));
                    (t, i) = (bt, bi + 1);
                }
                None => return false,
            },
        }
    }
    toks[t..].iter().all(|x| matches!(x, Tok::Star))
}

/// 경로 전체에 맞거나, 패턴에 `/`가 없으면 파일 이름에 맞아도 참(`*.lock`이 `web/x.lock`에 맞게).
pub(crate) fn path_matches(path: &str, pat: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    fnmatch(path, pat) || (!pat.contains('/') && fnmatch(name, pat))
}

/// 목록 API를 `per_page=100`으로 최대 10쪽. `keep`을 통과한 것만 모으고 `stop_at`개 모이면 멈춘다.
/// bool은 마지막 쪽이 꽉 찼는지(더 있을 수 있음) — `stop_at`이 없으면 10쪽이 다 찼다는 뜻(truncated).
pub(crate) async fn fetch_pages(
    api: &GithubApi,
    token: &str,
    path: &str,
    extra: &[(&str, String)],
    mut keep: impl FnMut(&Value) -> bool,
    stop_at: Option<usize>,
) -> Result<(Vec<Value>, bool), NodeError> {
    let (mut out, mut full) = (Vec::new(), false);
    for page in 1..=MAX_PAGES {
        let mut q = vec![("per_page", "100".to_string()), ("page", page.to_string())];
        q.extend(extra.iter().cloned());
        let body = api
            .call(token, Method::GET, path, &q, None)
            .await?
            .expect(200)?;
        let batch = match body {
            Value::Array(a) => a,
            _ => Vec::new(),
        };
        full = batch.len() >= 100;
        out.extend(batch.into_iter().filter(|v| keep(v)));
        if !full || stop_at.is_some_and(|n| out.len() >= n) {
            break;
        }
    }
    Ok((out, full))
}

/// 쉼표로 나눠 트림·빈 것 버림(agent `_split_patterns`). 글이 아니면 없음.
fn patterns(v: &Value) -> Vec<String> {
    v.as_str()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

pub(crate) async fn get_pr_files(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    // agent처럼 `to_int` — `#3`은 받지 않는다.
    let n = to_int(get(c, "pull_number"))
        .filter(|n| *n >= 1)
        .ok_or_else(|| input_error("pull_number는 양의 정수여야 합니다."))?;
    let max_files = to_int(get(c, "max_files")).unwrap_or(50).clamp(1, 300) as usize;
    let max_diff = to_int(get(c, "max_diff_chars"))
        .unwrap_or(60000)
        .clamp(1000, 200_000) as usize;
    let includes = patterns(get(c, "include_paths"));
    // 칸이 없으면 기본 제외, 빈 글이면 제외 없음.
    let excludes = match c.get("exclude_paths") {
        Some(v) => patterns(v),
        None => patterns(&Value::from(DEFAULT_EXCLUDE_PATHS)),
    };
    let t = token.token().await?;
    let pr_path = format!("{repo}/pulls/{n}");
    let pr = api
        .call(&t, Method::GET, &pr_path, &[], None)
        .await?
        .expect(200)?;
    let (files, truncated) =
        fetch_pages(api, &t, &format!("{pr_path}/files"), &[], |_| true, None).await?;

    // 처리 순서: 경로 필터 → max_files → diff 누적(넘친 파일부터 max_diff). 길이는 글자 수.
    let (mut skipped, mut diff, mut diff_len) = (Vec::new(), String::new(), 0);
    let (mut taken, mut overflow) = (0, false);
    for f in &files {
        let path = f["filename"].as_str().unwrap_or("");
        let hit = |ps: &[String]| ps.iter().any(|p| path_matches(path, p));
        let reason = if (!includes.is_empty() && !hit(&includes)) || hit(&excludes) {
            "filtered"
        } else if taken >= max_files {
            "max_files"
        } else {
            taken += 1;
            match f["patch"].as_str() {
                None => "no_patch",
                Some(patch) => {
                    let piece = format!("--- {path}\n{patch}\n");
                    let len = piece.chars().count();
                    if overflow || diff_len + len > max_diff {
                        overflow = true;
                        "max_diff"
                    } else {
                        diff.push_str(&piece);
                        diff_len += len;
                        continue;
                    }
                }
            }
        };
        skipped.push(json!({"filename": path, "reason": reason}));
    }
    let items: Vec<Value> = files
        .iter()
        .map(|f| {
            Value::Object(
                FILE_META_KEYS
                    .iter()
                    .map(|k| (k.to_string(), f[*k].clone()))
                    .collect(),
            )
        })
        .collect();
    Ok(object(json!({
        "found": !items.is_empty(),
        "total": items.len(),
        "items": items,
        "diff": diff,
        "head_sha": pr["head"]["sha"],
        "skipped": skipped,
        "truncated": truncated,
    })))
}

const REVIEW_STATES: [&str; 4] = ["APPROVED", "CHANGES_REQUESTED", "COMMENTED", "DISMISSED"];
const MAX_REVIEWS_OUT: usize = 50;
const MAX_COMMENTS_OUT: usize = 100;
const MAX_REVIEW_TEXT: usize = 2000;
const MAX_COMMENT_TEXT: usize = 1000;
const MAX_TEXT_TOTAL: usize = 150_000;

fn author_type(user: &Value) -> &'static str {
    if user["type"] == "Bot" {
        "bot"
    } else {
        "human"
    }
}

/// 체크박스 값. 참조식이 준 글 `"false"`는 거짓, 비었거나 없으면 참.
fn want_comments(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => !s.trim().eq_ignore_ascii_case("false"),
        _ => true,
    }
}

pub(crate) async fn get_pr_reviews(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = issue_no(get(c, "pull_number"))
        .ok_or_else(|| input_error("pull_number는 양의 정수여야 합니다."))?;
    let wanted: Vec<String> = super::input::csv(get(c, "states"))
        .iter()
        .map(|s| s.to_uppercase())
        .collect();
    if wanted.iter().any(|s| !REVIEW_STATES.contains(&s.as_str())) {
        return Err(input_error(&format!(
            "states는 {} 중에서 골라야 합니다.",
            REVIEW_STATES.join(", ")
        )));
    }
    let kind = match text(get(c, "author_type")).as_str() {
        "" => "all".to_string(),
        k @ ("human" | "bot" | "all") => k.to_string(),
        _ => {
            return Err(input_error(
                "author_type은 human, bot, all 중 하나여야 합니다.",
            ));
        }
    };
    let kept = |user: &Value| kind == "all" || author_type(user) == kind;
    let include = want_comments(get(c, "include_comments"));

    let t = token.token().await?;
    let pr_path = format!("{repo}/pulls/{n}");
    let (reviews, mut truncated) =
        fetch_pages(api, &t, &format!("{pr_path}/reviews"), &[], |_| true, None).await?;
    let mut comments = Vec::new();
    if include {
        let extra = [
            ("sort", "created".to_string()),
            ("direction", "desc".to_string()),
        ];
        let (mut found, more) = fetch_pages(
            api,
            &t,
            &format!("{pr_path}/comments"),
            &extra,
            |c| kept(&c["user"]),
            Some(MAX_COMMENTS_OUT),
        )
        .await?;
        truncated |= more || found.len() > MAX_COMMENTS_OUT;
        found.truncate(MAX_COMMENTS_OUT);
        comments = found;
    }

    // PENDING은 제출 전이라 뺀다. GitHub 응답은 오래된 것부터.
    let submitted: Vec<&Value> = reviews
        .iter()
        .filter(|r| r["state"] != "PENDING" && kept(&r["user"]))
        .collect();
    // 리뷰어마다 마지막 결정 하나(COMMENTED는 덮어쓰지 않음). login 없는 리뷰는 한 리뷰어로 묶는다.
    let mut decisions: Vec<(&Value, &str)> = Vec::new();
    for r in &submitted {
        let state = r["state"].as_str().unwrap_or("");
        if !matches!(state, "APPROVED" | "CHANGES_REQUESTED" | "DISMISSED") {
            continue;
        }
        let login = &r["user"]["login"];
        match decisions.iter_mut().find(|(l, _)| *l == login) {
            Some(d) => d.1 = state,
            None => decisions.push((login, state)),
        }
    }
    let count = |s: &str| decisions.iter().filter(|d| d.1 == s).count();
    let picked: Vec<&&Value> = submitted
        .iter()
        .rev()
        .filter(|r| wanted.is_empty() || wanted.iter().any(|w| r["state"] == w.as_str()))
        .collect();
    truncated |= picked.len() > MAX_REVIEWS_OUT;

    // 글자 수로 자르고 남은 예산에서 뺀다.
    let mut budget = MAX_TEXT_TOTAL;
    let mut cut = |v: &Value, limit: usize| -> String {
        let t = v.as_str().unwrap_or("");
        let out: String = t.chars().take(limit.min(budget)).collect();
        let len = out.chars().count();
        budget -= len;
        truncated |= len < t.chars().count();
        out
    };
    let items: Vec<Value> = picked
        .iter()
        .take(MAX_REVIEWS_OUT)
        .map(|r| {
            json!({
                "id": r["id"],
                "author": r["user"]["login"],
                "author_type": author_type(&r["user"]),
                "state": r["state"],
                "body": cut(&r["body"], MAX_REVIEW_TEXT),
                "submitted_at": r["submitted_at"],
                "commit_id": r["commit_id"],
            })
        })
        .collect();
    let out_comments: Vec<Value> = comments
        .iter()
        .map(|m| {
            let line = if m["line"].is_null() {
                &m["original_line"]
            } else {
                &m["line"]
            };
            json!({
                "path": m["path"],
                "line": line,
                "body": cut(&m["body"], MAX_COMMENT_TEXT),
                "author": m["user"]["login"],
                "created_at": m["created_at"],
            })
        })
        .collect();
    Ok(object(json!({
        "found": !items.is_empty(),
        "total": items.len(),
        "items": items,
        "message": if items.is_empty() { "결과가 없습니다" } else { "" },
        "comments": out_comments,
        "approved_count": count("APPROVED"),
        "changes_requested_count": count("CHANGES_REQUESTED"),
        "truncated": truncated,
    })))
}

const REVIEW_EVENTS: [&str; 3] = ["COMMENT", "APPROVE", "REQUEST_CHANGES"];
const MAX_REVIEW_COMMENTS: usize = 50;
const MAX_REVIEW_BODY: usize = 65536;

/// `@@ -a(,b)? +c(,d)? @@`로 시작하는 줄이면 c(agent `_HUNK_HEADER.match`).
fn hunk_start(line: &str) -> Option<u64> {
    fn num(s: &str) -> Option<(&str, &str)> {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        (end > 0).then(|| s.split_at(end))
    }
    fn count(s: &str) -> Option<&str> {
        match s.strip_prefix(',') {
            Some(r) => num(r).map(|(_, r)| r),
            None => Some(s),
        }
    }
    let (_, r) = num(line.strip_prefix("@@ -")?)?;
    let (c, r) = num(count(r)?.strip_prefix(" +")?)?;
    count(r)?.strip_prefix(" @@")?;
    c.parse().ok()
}

/// patch에서 PR 리뷰 줄 코멘트(side RIGHT)를 달 수 있는 새 파일 줄 번호(추가·문맥 줄).
pub(crate) fn right_side_lines(patch: &str) -> HashSet<u64> {
    let (mut lines, mut next) = (HashSet::new(), None);
    for raw in patch.split('\n') {
        if let Some(n) = hunk_start(raw) {
            next = Some(n);
        } else if let Some(n) = next.as_mut()
            && (raw.starts_with('+') || raw.starts_with(' '))
        {
            lines.insert(*n);
            *n = n.saturating_add(1);
        }
    }
    lines
}

/// 줄 코멘트 목록. AI 출력은 JSON 글에 코드 펜스·설명이 붙어 오기 쉽다. 못 읽으면 None.
pub(crate) fn parse_comments(v: &Value) -> Option<Vec<Value>> {
    let s = match v {
        Value::Null => return Some(Vec::new()),
        Value::Array(a) => return Some(a.clone()),
        Value::String(s) if s.is_empty() => return Some(Vec::new()),
        Value::String(s) => s,
        _ => return None,
    };
    let parsed = serde_json::from_str(s).ok().or_else(|| {
        // 코드 펜스(```lang)를 모두 지우고 첫 `[`~마지막 `]`로 다시.
        let (mut text, mut rest) = (String::new(), s.as_str());
        while let Some(i) = rest.find("```") {
            text.push_str(&rest[..i]);
            rest = rest[i + 3..].trim_start_matches(|c: char| c.is_ascii_alphabetic());
        }
        text.push_str(rest);
        match (text.find('['), text.rfind(']')) {
            (Some(a), Some(b)) if a < b => serde_json::from_str(&text[a..=b]).ok(),
            _ => None,
        }
    });
    match parsed {
        Some(Value::Array(a)) => Some(a),
        _ => None,
    }
}

fn valid_comment(c: &Value) -> Option<Value> {
    let path = c["path"].as_str().filter(|p| !p.is_empty())?;
    let line = to_int(&c["line"]).filter(|n| *n > 0)?;
    let body = c["body"].as_str().filter(|b| !b.trim().is_empty())?;
    Some(json!({"path": path, "line": line, "body": body}))
}

fn cut_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub(crate) async fn submit_pr_review(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = to_int(get(c, "pull_number"))
        .filter(|n| *n >= 1)
        .ok_or_else(|| input_error("pull_number는 양의 정수여야 합니다."))?;
    // 데스크톱엔 봇 토큰이 없다 — bot을 내 계정으로 바꿔치기하지 않는다. 빈 값은 me.
    let author = get(c, "author");
    if !(author.is_null() || matches!(author.as_str(), Some("" | "me"))) {
        return Err(input_error(
            "데스크톱에서는 내 GitHub 계정으로만 리뷰를 남길 수 있어요.",
        ));
    }
    let mut event = match get(c, "event") {
        Value::Null => "COMMENT",
        v => match v.as_str() {
            Some("") => "COMMENT",
            Some(e) if REVIEW_EVENTS.contains(&e) => e,
            _ => {
                return Err(input_error(&format!(
                    "event는 {} 중 하나여야 합니다.",
                    REVIEW_EVENTS.join(", ")
                )));
            }
        },
    };
    let body = get(c, "body").as_str().unwrap_or("");
    let mut warnings: Vec<String> = Vec::new();

    let parsed = parse_comments(get(c, "comments"));
    let unreadable = parsed.is_none();
    if unreadable {
        warnings.push("줄 코멘트를 읽지 못해 본문만 제출했습니다.".into());
    }
    let parsed = parsed.unwrap_or_default();
    let mut valid: Vec<Value> = parsed.iter().filter_map(valid_comment).collect();
    if valid.len() < parsed.len() {
        warnings.push(format!(
            "형식이 틀린 줄 코멘트 {}개를 버렸습니다.",
            parsed.len() - valid.len()
        ));
    }
    if valid.len() > MAX_REVIEW_COMMENTS {
        warnings.push(format!(
            "줄 코멘트는 {MAX_REVIEW_COMMENTS}개까지라 {}개를 버렸습니다.",
            valid.len() - MAX_REVIEW_COMMENTS
        ));
        valid.truncate(MAX_REVIEW_COMMENTS);
    }
    let mut long = 0;
    for v in &mut valid {
        let b = v["body"].as_str().unwrap_or("");
        if b.chars().count() > MAX_REVIEW_BODY {
            v["body"] = cut_chars(b, MAX_REVIEW_BODY).into();
            long += 1;
        }
    }
    if long > 0 {
        warnings.push(format!(
            "{MAX_REVIEW_BODY}자를 넘는 줄 코멘트 {long}개를 잘랐습니다."
        ));
    }
    // 본문 없는 COMMENT·REQUEST_CHANGES는 GitHub가 거절한다. 안내 문구만 있는 빈 리뷰도 내지 않는다.
    let empty = body.trim().is_empty() && valid.is_empty();
    if empty && event != "APPROVE" {
        return Err(input_error("리뷰 내용이 없습니다."));
    }
    let commit_id = get(c, "commit_id").as_str().map(str::trim).unwrap_or("");

    let t = token.token().await?;
    let pr_path = format!("{repo}/pulls/{n}");
    // patch 오른쪽 줄 밖 코멘트는 본문으로 — 하나라도 범위 밖이면 리뷰 전체가 422다.
    let (mut inline, mut moved) = (Vec::new(), Vec::new());
    if !valid.is_empty() {
        let (files, _) =
            fetch_pages(api, &t, &format!("{pr_path}/files"), &[], |_| true, None).await?;
        let paths: HashSet<&str> = valid.iter().filter_map(|v| v["path"].as_str()).collect();
        let ranges: HashMap<&str, HashSet<u64>> = files
            .iter()
            .filter_map(|f| Some((f["filename"].as_str()?, f["patch"].as_str()?)))
            .filter(|(name, patch)| !patch.is_empty() && paths.contains(name))
            .map(|(name, patch)| (name, right_side_lines(patch)))
            .collect();
        for v in valid {
            let hit = ranges
                .get(v["path"].as_str().unwrap_or(""))
                .zip(v["line"].as_u64())
                .is_some_and(|(r, l)| r.contains(&l));
            if hit { &mut inline } else { &mut moved }.push(v);
        }
    }

    // 자기 PR은 승인·변경 요청을 못 한다(422) → 코멘트로 낮춘다.
    if event != "COMMENT" {
        let me = api
            .call(&t, Method::GET, "/user", &[], None)
            .await?
            .expect(200)?;
        let pr = api
            .call(&t, Method::GET, &pr_path, &[], None)
            .await?
            .expect(200)?;
        if me["login"] == pr["user"]["login"] {
            warnings.push(format!(
                "자기 PR에는 {event}를 할 수 없어 코멘트로 제출했습니다."
            ));
            event = "COMMENT";
            if empty {
                return Err(input_error("리뷰 내용이 없습니다."));
            }
        }
    }

    let review_path = format!("{pr_path}/reviews");
    let post = |inline: &[Value], moved: &[Value]| {
        let mut sections: Vec<String> = Vec::new();
        if !body.trim().is_empty() {
            sections.push(body.to_string());
        }
        if !moved.is_empty() {
            let items: Vec<String> = moved
                .iter()
                .map(|m| {
                    format!(
                        "- {}:{} — {}",
                        m["path"].as_str().unwrap_or(""),
                        m["line"],
                        m["body"].as_str().unwrap_or("")
                    )
                })
                .collect();
            sections.push(format!("### 다른 줄에 대한 의견\n{}", items.join("\n")));
        }
        if sections.is_empty() && event != "APPROVE" {
            sections.push("IEUM 리뷰".into());
        }
        if unreadable {
            sections.push("줄 코멘트를 읽지 못했습니다".into());
        }
        let text = sections.join("\n\n");
        let cut = text.chars().count() > MAX_REVIEW_BODY;
        let comments: Vec<Value> = inline
            .iter()
            .map(|v| json!({"path": v["path"], "line": v["line"], "side": "RIGHT", "body": v["body"]}))
            .collect();
        let mut payload = json!({
            "event": event,
            "body": cut_chars(&text, MAX_REVIEW_BODY),
            "comments": comments,
        });
        if !commit_id.is_empty() {
            payload["commit_id"] = commit_id.into();
        }
        let path = &review_path;
        let t = &t;
        async move {
            let r = api.call(t, Method::POST, path, &[], Some(&payload)).await?;
            Ok::<_, NodeError>((r, cut))
        }
    };
    let (mut res, mut cut) = post(&inline, &moved).await?;
    // commit_id가 최신 head가 아니면 줄 범위가 다른 diff 기준이라 422가 날 수 있다 → 줄 코멘트를 본문으로 옮겨 한 번만 다시.
    if res.status == 422 && !inline.is_empty() {
        warnings.push(format!(
            "GitHub가 줄 코멘트를 받지 않아 {}개를 본문으로 옮겨 다시 제출했습니다.",
            inline.len()
        ));
        moved.append(&mut inline);
        (res, cut) = post(&inline, &moved).await?;
    }
    let review = res.expect(200)?;
    if cut {
        warnings.push(format!("본문이 {MAX_REVIEW_BODY}자를 넘어 잘랐습니다."));
    }
    Ok(object(json!({
        "review_id": review["id"],
        "url": review["html_url"],
        "state": review["state"],
        "posted_comments": inline.len(),
        "moved_comments": moved.len(),
        "warnings": warnings,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기대값은 `python3 -c "import fnmatch; print(fnmatch.fnmatchcase(n, p))"`로 뽑았다.
    #[test]
    fn fnmatch_table() {
        for (name, pat, want) in [
            ("dist/a/b.js", "dist/**", true),
            ("web/dist/x.js", "dist/**", false),
            ("a.lock", "*.lock", true),
            ("a/b/c.lock", "*.lock", true),
            ("A.LOCK", "*.lock", false),
            ("ab", "a?", true),
            ("a/", "a?", true),
            ("b1", "[!a]1", true),
            ("a1", "[!a]1", false),
            ("]", "[]]", true),
            ("[x", "[x", true),
            ("c", "[a-c]", true),
            ("src/x.rs", "src/*", true),
            // 그 밖 모서리 — 역시 Python으로 뽑은 값.
            ("c", "[a--c]", true),
            ("b", "[a--c]", false),
            ("-", "[a-c-e]", true),
            ("d", "[a-c-e]", false),
            ("x", "[!c-a]", true),
            ("a]", "[!]]]", true),
            ("-", "[a-]", true),
            ("b", "[c-a]", false),
            ("[!]", "[!]", true),
            ("[]", "[]", true),
            ("a", "[]", false),
            ("abc", "a*c*", true),
            ("aXbXc", "a*b*c", true),
            ("ab", "a**b", true),
            ("", "*", true),
            ("^", "[^]", true),
        ] {
            assert_eq!(fnmatch(name, pat), want, "{name} {pat}");
        }
    }

    #[test]
    fn comments_fenced_json() {
        let v = Value::from("설명\n```json\n[{\"path\":\"a.rs\",\"line\":2,\"body\":\"x\"}]\n```");
        assert_eq!(
            parse_comments(&v),
            Some(vec![json!({"path": "a.rs", "line": 2, "body": "x"})])
        );
        assert_eq!(parse_comments(&json!("")), Some(vec![]));
        assert_eq!(parse_comments(&Value::Null), Some(vec![]));
        assert_eq!(parse_comments(&json!("{}")), None);
        assert_eq!(parse_comments(&json!("글만")), None);
        assert_eq!(parse_comments(&json!(3)), None);
    }

    #[test]
    fn right_side_lines_counts_new_file() {
        let got = right_side_lines("@@ -1,2 +10,3 @@\n a\n-b\n+c\n d");
        assert_eq!(got, HashSet::from([10, 11, 12]));
        // 두 번째 hunk에서 다시 센다. `\`·`@@` 아닌 머리 줄은 세지 않는다.
        let got = right_side_lines("@@ -1 +1 @@\n+a\n\\ No newline\n@@ -5 +7,2 @@ fn x\n b\n+c");
        assert_eq!(got, HashSet::from([1, 7, 8]));
        assert!(right_side_lines("+a\n b").is_empty());
    }

    #[test]
    fn path_matches_name_fallback() {
        assert!(path_matches("web/x.lock", "*.lock"));
        assert!(!path_matches("web/dist/x.js", "dist/*"));
    }
}
