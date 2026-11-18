//! PR 노드 — agent `github_create_pull_request`·`github_update_pull_request`·`github_request_reviewers`·
//! `github_find_or_create_pull_request`를 옮겼다. 입력 검사를 모두 끝낸 뒤 토큰을 읽는다.

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{base_ref, csv, head_query, is_login, issue_no, pr_refs, text};
use super::issues::{get, object, repo_path};

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
#[allow(dead_code)] // 다음 작업(PR 찾기·목록)에서 쓴다.
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
        let b = base_ref(get(c, "base"))
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
