//! 저장소·조직·사용자 조회와 조직 멤버 확인 노드 — agent `github_find_repo`·`github_find_org`·
//! `github_find_user`·`github_check_org_membership`를 옮겼다. 입력 검사를 끝낸 뒤 토큰을 읽는다.

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{is_login, is_path_name, text};

fn get<'a>(c: &'a Map<String, Value>, key: &str) -> &'a Value {
    c.get(key).unwrap_or(&Value::Null)
}

fn object(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// GET 한 번. 200 → `Some`, 404 → `None`(조회는 없음을 0건으로 본다), 그 밖은 상태별 오류.
pub(crate) async fn get_found(
    api: &GithubApi,
    token: &str,
    path: &str,
    query: &[(&str, String)],
) -> Result<Option<Value>, NodeError> {
    let r = api.call(token, Method::GET, path, query, None).await?;
    if r.status == 404 {
        return Ok(None);
    }
    r.expect(200).map(Some)
}

/// 조회 결과 출력. `message`가 없으면 비었을 때만 "결과가 없습니다".
pub(crate) fn found(items: Vec<Value>, message: Option<&str>) -> Map<String, Value> {
    let message = message.unwrap_or(if items.is_empty() {
        "결과가 없습니다"
    } else {
        ""
    });
    object(json!({
        "found": !items.is_empty(),
        "total": items.len(),
        "message": message,
        "items": items,
    }))
}

pub(crate) async fn find_repo(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let (o, r) = (get(c, "owner"), get(c, "repo"));
    if !(is_path_name(o) && is_path_name(r)) {
        return Err(input_error("owner/repo 형식이 올바르지 않습니다."));
    }
    let path = format!(
        "/repos/{}/{}",
        o.as_str().unwrap_or(""),
        r.as_str().unwrap_or("")
    );
    let t = token.token().await?;
    let items = get_found(api, &t, &path, &[])
        .await?
        .map(|d| {
            json!({
                "name": d["name"],
                "full_name": d["full_name"],
                "owner": d["owner"]["login"],
                "private": d["private"].as_bool().unwrap_or(false),
                "default_branch": d["default_branch"],
                "url": d["html_url"],
                "description": d["description"].as_str().unwrap_or(""),
            })
        })
        .into_iter()
        .collect();
    Ok(found(items, None))
}

pub(crate) async fn find_org(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let org = get(c, "org");
    if !is_login(org) {
        return Err(input_error("org 형식이 올바르지 않습니다."));
    }
    let t = token.token().await?;
    let path = format!("/orgs/{}", org.as_str().unwrap_or(""));
    let items = get_found(api, &t, &path, &[])
        .await?
        .map(|d| {
            json!({
                "login": d["login"],
                "name": d["name"],
                "description": d["description"].as_str().unwrap_or(""),
                "url": d["html_url"],
            })
        })
        .into_iter()
        .collect();
    Ok(found(items, None))
}

pub(crate) async fn find_user(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let name = text(get(c, "username"));
    // 비우면 연결된 계정 본인.
    let path = if name.is_empty() {
        "/user".to_string()
    } else if is_login(&Value::String(name.clone())) {
        format!("/users/{name}")
    } else {
        return Err(input_error("username 형식이 올바르지 않습니다."));
    };
    let t = token.token().await?;
    let items = get_found(api, &t, &path, &[])
        .await?
        .map(|d| json!({"login": d["login"], "name": d["name"], "type": d["type"], "url": d["html_url"]}))
        .into_iter()
        .collect();
    Ok(found(items, None))
}

pub(crate) async fn check_org_membership(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let (org, user) = (get(c, "org"), get(c, "username"));
    if !(is_login(org) && is_login(user)) {
        return Err(input_error("org/username 형식이 올바르지 않습니다."));
    }
    let (org, user) = (org.as_str().unwrap_or(""), user.as_str().unwrap_or(""));
    let t = token.token().await?;
    let mut r = api
        .call(
            &t,
            Method::GET,
            &format!("/orgs/{org}/members/{user}"),
            &[],
            None,
        )
        .await?;
    // 302: 요청자가 그 조직 멤버가 아니면 공개 멤버 확인 주소로 보낸다. 리다이렉트는 따라가지 않으니 한 번만 직접 간다.
    if r.status == 302 {
        r = api
            .call(
                &t,
                Method::GET,
                &format!("/orgs/{org}/public_members/{user}"),
                &[],
                None,
            )
            .await?;
    }
    let (items, message, member) = match r.status {
        204 => (vec![json!({"org": org, "username": user})], None, true),
        404 => (vec![], Some("멤버가 아닙니다"), false),
        _ => {
            r.expect(204)?;
            unreachable!()
        }
    };
    let mut out = found(items, message);
    out.insert("member".into(), member.into());
    Ok(out)
}
