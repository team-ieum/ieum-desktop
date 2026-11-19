//! PR 노드 — agent `github_create_pull_request`·`github_update_pull_request`·`github_request_reviewers`·
//! `github_find_or_create_pull_request`를 옮겼다. 입력 검사를 모두 끝낸 뒤 토큰을 읽는다.

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{csv, head_query, head_ref, is_login, issue_no, pr_refs, text, to_int};
use super::issues::{MAX_SEARCH_VALUE, get, object, repo_path};
use super::lookup::found;

const PR_SEARCH_BY: [&str; 5] = ["number", "branch", "title", "body", "author"];

fn pull_number(c: &Map<String, Value>) -> Result<u64, NodeError> {
    issue_no(get(c, "pull_number"))
        .ok_or_else(|| input_error("pull_number는 양의 정수여야 합니다."))
}

/// 불리언 칸 — JSON bool 또는 글 `"true"`/`"false"`. 그 밖·빈 값은 기본값.
fn flag(v: &Value, default: bool) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => default,
        },
        _ => default,
    }
}

pub(crate) fn pr_out(pr: &Value) -> Map<String, Value> {
    object(json!({
        "number": pr["number"],
        "url": pr["html_url"],
        "title": pr["title"],
        "state": pr["state"],
        "draft": pr["draft"].as_bool().unwrap_or(false),
    }))
}

/// 조회 결과 한 건 — 검색 응답에도 있는 키만.
pub(crate) fn pr_item(pr: &Value) -> Value {
    json!({
        "number": pr["number"],
        "title": pr["title"],
        "state": pr["state"],
        "url": pr["html_url"],
        "draft": pr["draft"].as_bool().unwrap_or(false),
        "author": pr["user"]["login"],
        "created_at": pr["created_at"],
    })
}

/// 검사를 마친 PR 만들기 칸.
struct NewPr {
    repo: String,
    payload: Value,
}

fn new_pr(c: &Map<String, Value>) -> Result<NewPr, NodeError> {
    let repo = repo_path(c)?;
    let (head, base) = pr_refs(get(c, "head"), get(c, "base"))?;
    if text(get(c, "title")).is_empty() {
        return Err(input_error("title을 입력해 주세요."));
    }
    let body = get(c, "body").as_str().unwrap_or("");
    let payload = json!({
        "title": get(c, "title"),
        "head": head,
        "base": base,
        "body": body,
        "draft": flag(get(c, "draft"), false),
        "maintainer_can_modify": flag(get(c, "maintainer_can_modify"), true),
    });
    Ok(NewPr { repo, payload })
}

async fn post_pr(api: &GithubApi, t: &str, n: &NewPr) -> Result<Value, NodeError> {
    api.call(
        t,
        Method::POST,
        &format!("{}/pulls", n.repo),
        &[],
        Some(&n.payload),
    )
    .await?
    .expect(201)
}

pub(crate) async fn create_pull_request(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let n = new_pr(c)?;
    let t = token.token().await?;
    Ok(pr_out(&post_pr(api, &t, &n).await?))
}

pub(crate) async fn update_pull_request(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = pull_number(c)?;
    // 비어 있지 않은 title·body만 원문 그대로 보낸다.
    let mut payload = Map::new();
    for key in ["title", "body"] {
        if !text(get(c, key)).is_empty() {
            payload.insert(key.into(), get(c, key).clone());
        }
    }
    let state = text(get(c, "state"));
    if !state.is_empty() {
        if state != "open" && state != "closed" {
            return Err(input_error("state는 open, closed 중 하나여야 합니다."));
        }
        payload.insert("state".into(), state.into());
    }
    if !text(get(c, "base")).is_empty() {
        // 만들기(pr_refs)와 같은 규칙 — 앞뒤 공백을 걷은 뒤 ':'가 있으면 거절한다.
        let b = head_ref(get(c, "base"))
            .filter(|b| !b.contains(':'))
            .ok_or_else(|| input_error("base 브랜치 형식이 올바르지 않습니다."))?;
        payload.insert("base".into(), b.into());
    }
    if payload.is_empty() {
        return Err(input_error("바꿀 칸이 없습니다."));
    }
    let t = token.token().await?;
    let pr = api
        .call(
            &t,
            Method::PATCH,
            &format!("{repo}/pulls/{n}"),
            &[],
            Some(&Value::Object(payload)),
        )
        .await?
        .expect(200)?;
    let mut out = pr_out(&pr);
    out.remove("draft");
    Ok(out)
}

pub(crate) async fn request_reviewers(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = pull_number(c)?;
    let names = csv(get(c, "reviewers"));
    if names.is_empty() || !names.iter().all(|x| is_login(&Value::from(x.as_str()))) {
        return Err(input_error(
            "reviewers는 쉼표로 구분한 GitHub 사용자 이름이어야 합니다.",
        ));
    }
    let t = token.token().await?;
    let res = api
        .call(
            &t,
            Method::POST,
            &format!("{repo}/pulls/{n}/requested_reviewers"),
            &[],
            Some(&json!({ "reviewers": names })),
        )
        .await?
        .expect(201)?;
    let requested: Vec<&Value> = res["requested_reviewers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|u| &u["login"])
        .collect();
    Ok(object(
        json!({"number": n, "requested_reviewers": requested}),
    ))
}

pub(crate) async fn find_or_create_pull_request(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let n = new_pr(c)?;
    let t = token.token().await?;
    let owner = get(c, "owner").as_str().unwrap_or("");
    let (head, base) = (
        n.payload["head"].as_str().unwrap_or(""),
        n.payload["base"].as_str().unwrap_or(""),
    );
    let found = api
        .call(
            &t,
            Method::GET,
            &format!("{}/pulls", n.repo),
            &[
                ("head", head_query(owner, head)),
                ("base", base.to_string()),
                ("state", "open".into()),
                ("per_page", "1".into()),
            ],
            None,
        )
        .await?
        .expect(200)?;
    let (created, pr) = match found.as_array().and_then(|a| a.first()) {
        Some(pr) => (false, pr.clone()),
        None => (true, post_pr(api, &t, &n).await?),
    };
    let mut out = Map::new();
    out.insert("created".into(), created.into());
    out.extend(pr_out(&pr));
    Ok(out)
}

pub(crate) async fn find_pull_request(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let by = get(c, "search_by").as_str().unwrap_or("");
    if !PR_SEARCH_BY.contains(&by) {
        return Err(input_error(&format!(
            "search_by는 {} 중 하나여야 합니다.",
            PR_SEARCH_BY.join(", ")
        )));
    }
    let state = Some(text(get(c, "state")))
        .filter(|s| !s.is_empty())
        .unwrap_or("open".into());
    if !["open", "closed", "all"].contains(&state.as_str()) {
        return Err(input_error("state는 open, closed, all 중 하나여야 합니다."));
    }
    // 번호는 참조식 결과로 정수가 올 수 있다.
    let value = match get(c, "value") {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
        v => text(v),
    };
    if value.is_empty() {
        return Err(input_error("value를 입력해 주세요."));
    }
    let limit = to_int(get(c, "max_results")).unwrap_or(20).clamp(1, 100) as usize;
    let v = Value::String(value.clone());
    let (number, head) = (issue_no(&v), head_ref(&v));
    match by {
        "number" if number.is_none() => {
            return Err(input_error("PR 번호는 양의 정수여야 합니다."));
        }
        "branch" if head.is_none() => {
            return Err(input_error("브랜치 형식이 올바르지 않습니다."));
        }
        "author" if !is_login(&v) => {
            return Err(input_error("GitHub 사용자 이름 형식이 올바르지 않습니다."));
        }
        "title" | "body" if value.chars().count() > MAX_SEARCH_VALUE => {
            return Err(input_error(&format!(
                "검색어는 {MAX_SEARCH_VALUE}자 이하여야 합니다."
            )));
        }
        _ => {}
    }
    let t = token.token().await?;
    let owner = get(c, "owner").as_str().unwrap_or("");
    let (path, query) = match (by, number, head) {
        ("number", Some(n), _) => (format!("{repo}/pulls/{n}"), vec![]),
        ("branch", _, Some(h)) => (
            format!("{repo}/pulls"),
            vec![
                ("head", head_query(owner, &h)),
                ("state", state),
                ("per_page", limit.to_string()),
            ],
        ),
        _ => {
            let r = repo.trim_start_matches("/repos/");
            let state_q = if state == "all" {
                String::new()
            } else {
                format!(" state:{state}")
            };
            let q = if by == "author" {
                format!("author:{value} is:pr repo:{r}{state_q}")
            } else {
                format!("{value} in:{by} repo:{r} is:pr{state_q}")
            };
            (
                "/search/issues".to_string(),
                vec![("q", q), ("per_page", limit.to_string())],
            )
        }
    };
    let r = api.call(&t, Method::GET, &path, &query, None).await?;
    if r.status == 404 {
        return Ok(found(vec![], None));
    }
    let body = r.expect(200)?;
    let hits: Vec<&Value> = if path == "/search/issues" {
        let want = format!("https://api.github.com{repo}").to_lowercase();
        body["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| {
                i["repository_url"].as_str().unwrap_or("").to_lowercase() == want
                    && i.get("pull_request").is_some()
            })
            .collect()
    } else if let Some(a) = body.as_array() {
        a.iter().collect()
    } else {
        vec![&body]
    };
    Ok(found(
        hits.into_iter().take(limit).map(pr_item).collect(),
        None,
    ))
}
