//! 이슈 노드 — agent `github_create_issue`·`github_update_issue`·`github_add_labels`·`github_create_comment`를 옮겼다.
//! 입력 검사를 모두 끝낸 뒤 토큰을 읽는다(잘못된 칸이면 gh도 GitHub도 건드리지 않게).

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{
    csv, dropped, is_login, is_path_name, issue_fields, issue_no, same_title, text, to_int,
};

const MAX_COMMENT_BODY: usize = 65536;
const SEARCH_BY: [&str; 6] = ["number", "label", "assignee", "author", "title", "body"];
const MAX_PAGES: usize = 10;
const MAX_SEARCH_VALUE: usize = 200;
const STATE_REASONS: [&str; 4] = ["completed", "not_planned", "duplicate", "reopened"];

fn get<'a>(c: &'a Map<String, Value>, key: &str) -> &'a Value {
    c.get(key).unwrap_or(&Value::Null)
}

/// 검사를 통과한 `/repos/{owner}/{repo}`.
fn repo_path(c: &Map<String, Value>) -> Result<String, NodeError> {
    match (get(c, "owner"), get(c, "repo")) {
        (o, r) if is_path_name(o) && is_path_name(r) => Ok(format!(
            "/repos/{}/{}",
            o.as_str().unwrap_or(""),
            r.as_str().unwrap_or("")
        )),
        _ => Err(input_error("owner/repo 형식이 올바르지 않습니다.")),
    }
}

fn number(c: &Map<String, Value>) -> Result<u64, NodeError> {
    issue_no(get(c, "issue_number"))
        .ok_or_else(|| input_error("issue_number는 양의 정수여야 합니다."))
}

fn object(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// 이슈 만들기 POST — title·body는 원문 그대로(빈 값도) 보낸다. 만든 이슈 응답을 돌려준다.
async fn post_issue(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
    repo: &str,
    fields: &Map<String, Value>,
) -> Result<Value, NodeError> {
    let mut payload = Map::new();
    for key in ["title", "body"] {
        payload.insert(key.into(), c.get(key).cloned().unwrap_or(json!("")));
    }
    payload.extend(fields.clone());
    let t = token.token().await?;
    api.call(
        &t,
        Method::POST,
        &format!("{repo}/issues"),
        &[],
        Some(&Value::Object(payload)),
    )
    .await?
    .expect(201)
}

pub(crate) async fn create_issue(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let fields = issue_fields(get(c, "labels"), get(c, "assignees"), get(c, "milestone"))?;
    let issue = post_issue(api, token, c, &repo, &fields).await?;
    Ok(object(json!({
        "number": issue["number"],
        "url": issue["html_url"],
        "title": issue["title"],
        "warnings": dropped(&fields, &issue),
    })))
}

pub(crate) async fn update_issue(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = number(c)?;
    let fields = issue_fields(get(c, "labels"), get(c, "assignees"), get(c, "milestone"))?;
    let (state, reason) = (text(get(c, "state")), text(get(c, "state_reason")));
    if !state.is_empty() && state != "open" && state != "closed" {
        return Err(input_error("state는 open, closed 중 하나여야 합니다."));
    }
    if !reason.is_empty() && !STATE_REASONS.contains(&reason.as_str()) {
        return Err(input_error(&format!(
            "state_reason은 {} 중 하나여야 합니다.",
            STATE_REASONS.join(", ")
        )));
    }
    // 비어 있지 않은 title·body만, 원문 그대로 보낸다.
    let mut payload = Map::new();
    for key in ["title", "body"] {
        if !text(get(c, key)).is_empty() {
            payload.insert(key.into(), get(c, key).clone());
        }
    }
    let mut warnings = Vec::new();
    if !state.is_empty() {
        payload.insert("state".into(), state.into());
        if !reason.is_empty() {
            payload.insert("state_reason".into(), reason.into());
        }
    } else if !reason.is_empty() {
        warnings.push("닫는 이유는 상태를 바꿀 때만 적용돼요.".to_string());
    }
    payload.extend(fields.clone());
    if payload.is_empty() {
        return Err(input_error("바꿀 칸이 없습니다."));
    }
    let t = token.token().await?;
    let issue = api
        .call(
            &t,
            Method::PATCH,
            &format!("{repo}/issues/{n}"),
            &[],
            Some(&Value::Object(payload)),
        )
        .await?
        .expect(200)?;
    warnings.extend(dropped(&fields, &issue));
    Ok(object(json!({
        "number": issue["number"],
        "url": issue["html_url"],
        "title": issue["title"],
        "state": issue["state"],
        "warnings": warnings,
    })))
}

pub(crate) async fn add_labels(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = number(c)?;
    let names = csv(get(c, "labels"));
    if names.is_empty() {
        return Err(input_error("labels를 입력해 주세요."));
    }
    let t = token.token().await?;
    let labels = api
        .call(
            &t,
            Method::POST,
            &format!("{repo}/issues/{n}/labels"),
            &[],
            Some(&json!({ "labels": names })),
        )
        .await?
        .expect(200)?;
    // 더한 뒤 이슈에 붙은 전체 라벨.
    let names: Vec<Value> = labels
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| l["name"].clone())
        .collect();
    Ok(object(json!({ "number": n, "labels": names })))
}

pub(crate) async fn create_comment(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let n = number(c)?;
    let body = get(c, "body");
    if text(body).is_empty() {
        return Err(input_error("body를 입력해 주세요."));
    }
    if body.as_str().unwrap_or("").chars().count() > MAX_COMMENT_BODY {
        return Err(input_error("코멘트는 65536자 이하여야 합니다."));
    }
    let t = token.token().await?;
    let comment = api
        .call(
            &t,
            Method::POST,
            &format!("{repo}/issues/{n}/comments"),
            &[],
            Some(&json!({ "body": body })),
        )
        .await?
        .expect(201)?;
    Ok(object(
        json!({ "id": comment["id"], "url": comment["html_url"] }),
    ))
}

/// 찾기 결과의 이슈 한 건.
pub(crate) fn issue_item(i: &Value) -> Value {
    let names = |key: &str, name: &str| -> Vec<Value> {
        i[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| x[name].clone())
            .collect()
    };
    json!({
        "number": i["number"],
        "title": i["title"],
        "state": i["state"],
        "url": i["html_url"],
        "labels": names("labels", "name"),
        "assignees": names("assignees", "login"),
        "author": i["user"]["login"],
        "created_at": i["created_at"],
    })
}

/// 검사를 통과한 (search_by, state, value).
fn find_plan(
    search_by: &Value,
    value: &Value,
    state: &Value,
) -> Result<(String, String, String), NodeError> {
    let by = search_by.as_str().unwrap_or("");
    if !SEARCH_BY.contains(&by) {
        return Err(input_error(&format!(
            "search_by는 {} 중 하나여야 합니다.",
            SEARCH_BY.join(", ")
        )));
    }
    let state = Some(text(state))
        .filter(|s| !s.is_empty())
        .unwrap_or("open".into());
    if !["open", "closed", "all"].contains(&state.as_str()) {
        return Err(input_error("state는 open, closed, all 중 하나여야 합니다."));
    }
    // 번호는 참조식 결과로 정수가 올 수 있다.
    let value = match value {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
        v => text(v),
    };
    if value.is_empty() {
        return Err(input_error("value를 입력해 주세요."));
    }
    let v = Value::String(value.clone());
    match by {
        "number" if issue_no(&v).is_none() => {
            return Err(input_error("이슈 번호는 양의 정수여야 합니다."));
        }
        "assignee" | "author" if !is_login(&v) => {
            return Err(input_error("GitHub 사용자 이름 형식이 올바르지 않습니다."));
        }
        "title" | "body" if value.chars().count() > MAX_SEARCH_VALUE => {
            return Err(input_error(&format!(
                "검색어는 {MAX_SEARCH_VALUE}자 이하여야 합니다."
            )));
        }
        _ => {}
    }
    Ok((by.into(), state, value))
}

/// 토큰을 읽기 전에 부른다 — 칸이 틀리면 GitHub도 gh도 건드리지 않게.
pub(crate) fn find_issues_check(
    search_by: &Value,
    value: &Value,
    state: &Value,
) -> Result<(), NodeError> {
    find_plan(search_by, value, state).map(|_| ())
}

/// 이슈 찾기. 404는 0건, PR은 뺀다.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn find_issues(
    api: &GithubApi,
    token: &str,
    owner: &str,
    repo: &str,
    search_by: &Value,
    value: &Value,
    state: &Value,
    limit: usize,
) -> Result<Vec<Value>, NodeError> {
    let (by, state, value) = find_plan(search_by, value, state)?;
    let repo_api = format!("/repos/{owner}/{repo}");
    if by == "number" {
        let n = issue_no(&Value::String(value)).unwrap_or(0);
        let r = api
            .call(
                token,
                Method::GET,
                &format!("{repo_api}/issues/{n}"),
                &[],
                None,
            )
            .await?;
        if r.status == 404 {
            return Ok(vec![]);
        }
        let issue = r.expect(200)?;
        return Ok(if issue.get("pull_request").is_some() {
            vec![]
        } else {
            vec![issue_item(&issue)]
        });
    }
    if by != "title" && by != "body" {
        let name = match by.as_str() {
            "label" => "labels",
            "author" => "creator",
            _ => "assignee",
        };
        let mut items = Vec::new();
        for page in 1..=MAX_PAGES {
            let q = [
                (name, value.clone()),
                ("state", state.clone()),
                ("per_page", "100".into()),
                ("page", page.to_string()),
            ];
            let r = api
                .call(token, Method::GET, &format!("{repo_api}/issues"), &q, None)
                .await?;
            if r.status == 404 {
                return Ok(vec![]);
            }
            let batch = r.expect(200)?;
            let batch = batch.as_array().cloned().unwrap_or_default();
            items.extend(
                batch
                    .iter()
                    .filter(|i| i.get("pull_request").is_none())
                    .map(issue_item),
            );
            if items.len() >= limit || batch.len() < 100 {
                break;
            }
        }
        items.truncate(limit);
        return Ok(items);
    }
    let state_q = if state == "all" {
        String::new()
    } else {
        format!(" state:{state}")
    };
    let q = [
        (
            "q",
            format!("{value} in:{by} repo:{owner}/{repo} is:issue{state_q}"),
        ),
        ("per_page", limit.to_string()),
    ];
    let r = api
        .call(token, Method::GET, "/search/issues", &q, None)
        .await?;
    if r.status == 404 {
        return Ok(vec![]);
    }
    let body = r.expect(200)?;
    let want = format!("https://api.github.com/repos/{owner}/{repo}").to_lowercase();
    let mut items: Vec<Value> = body["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| {
            i["repository_url"].as_str().unwrap_or("").to_lowercase() == want
                && i.get("pull_request").is_none()
        })
        .map(issue_item)
        .collect();
    items.truncate(limit);
    Ok(items)
}

pub(crate) async fn find_issue(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    repo_path(c)?;
    let (search_by, value, state) = (get(c, "search_by"), get(c, "value"), get(c, "state"));
    find_issues_check(search_by, value, state)?;
    let limit = to_int(get(c, "max_results")).unwrap_or(20).clamp(1, 100) as usize;
    let t = token.token().await?;
    let (o, r) = (text(get(c, "owner")), text(get(c, "repo")));
    let items = find_issues(api, &t, &o, &r, search_by, value, state, limit).await?;
    Ok(object(json!({
        "found": !items.is_empty(),
        "total": items.len(),
        "message": if items.is_empty() { "결과가 없습니다" } else { "" },
        "items": items,
    })))
}

pub(crate) async fn list_issues(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    // agent와 같게 state는 검사하지 않는다.
    let state = Some(text(get(c, "state")))
        .filter(|s| !s.is_empty())
        .unwrap_or("open".into());
    let t = token.token().await?;
    let q = [
        ("per_page", "20".to_string()),
        ("state", state),
        ("sort", "updated".into()),
    ];
    let list = api
        .call(&t, Method::GET, &format!("{repo}/issues"), &q, None)
        .await?
        .expect(200)?;
    let issues: Vec<Value> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i.get("pull_request").is_none())
        .map(|i| {
            let labels: Vec<&Value> = i["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|l| &l["name"])
                .collect();
            json!({"number": i["number"], "title": i["title"], "state": i["state"],
                   "labels": labels, "created_at": i["created_at"]})
        })
        .collect();
    Ok(object(json!({ "total": issues.len(), "issues": issues })))
}

pub(crate) async fn find_or_create_issue(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let fields = issue_fields(get(c, "labels"), get(c, "assignees"), get(c, "milestone"))?;
    if text(get(c, "title")).is_empty() {
        return Err(input_error("title을 입력해 주세요."));
    }
    let (search_by, value, state) = (get(c, "search_by"), get(c, "value"), get(c, "state"));
    // 비교도 검색과 같은 정규화 값으로 — 참조식이 준 숫자도 글로 바꿔 둔다(agent `str(value)`).
    let (_, _, want) = find_plan(search_by, value, state)?;
    let t = token.token().await?;
    let (o, r) = (text(get(c, "owner")), text(get(c, "repo")));
    // 제목 검색은 토큰 일치라 부분 일치도 걸린다 → 100건 받아 제목 완전 일치만 남긴다.
    let by_title = search_by.as_str() == Some("title");
    let limit = if by_title { 100 } else { 1 };
    let mut items = find_issues(api, &t, &o, &r, search_by, value, state, limit).await?;
    if by_title {
        items.retain(|i| same_title(i["title"].as_str().unwrap_or(""), &want));
    }
    if let Some(i) = items.first() {
        return Ok(object(json!({
            "created": false, "number": i["number"], "url": i["url"],
            "title": i["title"], "state": i["state"], "warnings": [],
        })));
    }
    let issue = post_issue(api, token, c, &repo, &fields).await?;
    Ok(object(json!({
        "created": true,
        "number": issue["number"],
        "url": issue["html_url"],
        "title": issue["title"],
        "state": issue.get("state").filter(|s| s.is_string()).cloned().unwrap_or(json!("open")),
        "warnings": dropped(&fields, &issue),
    })))
}
