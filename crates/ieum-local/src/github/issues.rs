//! 이슈 노드 — agent `github_create_issue`·`github_add_labels`·`github_create_comment`를 옮겼다.
//! 입력 검사를 모두 끝낸 뒤 토큰을 읽는다(잘못된 칸이면 gh도 GitHub도 건드리지 않게).

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{csv, dropped, is_path_name, issue_fields, issue_no, text};

const MAX_COMMENT_BODY: usize = 65536;

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

pub(crate) async fn create_issue(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let repo = repo_path(c)?;
    let fields = issue_fields(get(c, "labels"), get(c, "assignees"), get(c, "milestone"))?;
    // title·body는 원문 그대로(빈 값도) 보낸다.
    let mut payload = Map::new();
    for key in ["title", "body"] {
        payload.insert(key.into(), c.get(key).cloned().unwrap_or(json!("")));
    }
    payload.extend(fields.clone());
    let t = token.token().await?;
    let issue = api
        .call(
            &t,
            Method::POST,
            &format!("{repo}/issues"),
            &[],
            Some(&Value::Object(payload)),
        )
        .await?
        .expect(201)?;
    Ok(object(json!({
        "number": issue["number"],
        "url": issue["html_url"],
        "title": issue["title"],
        "warnings": dropped(&fields, &issue),
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
