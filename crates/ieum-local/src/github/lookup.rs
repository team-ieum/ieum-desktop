//! 저장소·조직·사용자 조회와 조직 멤버 확인 노드 — agent `github_find_repo`·`github_find_org`·
//! `github_find_user`·`github_check_org_membership`를 옮겼다. 입력 검사를 끝낸 뒤 토큰을 읽는다.

use ieum_engine::NodeError;
use reqwest::Method;
use serde_json::{Map, Value, json};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{base_ref, encode_segment, is_login, is_path_name, ref_path, text};

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

const MAX_FILE_CHARS: usize = 100_000;
const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_DIR_ITEMS: usize = 1000;

fn repo_of(c: &Map<String, Value>) -> Result<String, NodeError> {
    let (o, r) = (get(c, "owner"), get(c, "repo"));
    if !(is_path_name(o) && is_path_name(r)) {
        return Err(input_error("owner/repo 형식이 올바르지 않습니다."));
    }
    Ok(format!(
        "/repos/{}/{}",
        o.as_str().unwrap_or(""),
        r.as_str().unwrap_or("")
    ))
}

pub(crate) async fn find_branch(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let base = repo_of(c)?;
    let branch = get(c, "branch");
    let name = text(branch);
    let (path, query) = if name.is_empty() {
        (
            format!("{base}/branches"),
            vec![("per_page", "100".to_string())],
        )
    } else {
        if ref_path(branch).is_none() {
            return Err(input_error("브랜치 형식이 올바르지 않습니다."));
        }
        // 이름 전체를 한 조각으로 — `/`가 남으면 …/branches/main/protection 같은 하위 API로 닿는다.
        (format!("{base}/branches/{}", encode_segment(&name)), vec![])
    };
    let t = token.token().await?;
    let data = get_found(api, &t, &path, &query).await?;
    let list = match data {
        Some(Value::Array(a)) => a,
        Some(v @ Value::Object(_)) => vec![v],
        _ => vec![],
    };
    let items = list
        .iter()
        .take(100)
        .map(|b| {
            json!({
                "name": b["name"],
                "sha": b["commit"]["sha"],
                "protected": b["protected"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    Ok(found(items, None))
}

fn file_item(e: &Value, content: String, truncated: bool) -> Value {
    json!({
        "name": e["name"], "path": e["path"], "type": e["type"], "sha": e["sha"],
        "size": e["size"], "content": content, "truncated": truncated,
    })
}

/// (content, truncated). 1MB 넘는 파일은 GitHub가 내용을 주지 않는다(encoding none). 바이너리는 "".
fn file_content(e: &Value) -> (String, bool) {
    if e["size"].as_u64().unwrap_or(0) > MAX_FILE_BYTES || e["encoding"] == "none" {
        return (String::new(), true);
    }
    let text =
        base64_decode(e["content"].as_str().unwrap_or("")).and_then(|b| String::from_utf8(b).ok());
    match text {
        // 글자(문자) 단위로 자른다 — 바이트로 자르면 한글이 깨진다.
        Some(t) if t.chars().count() > MAX_FILE_CHARS => {
            (t.chars().take(MAX_FILE_CHARS).collect(), true)
        }
        Some(t) => (t, false),
        None => (String::new(), false),
    }
}

/// 표준 base64 해독. 공백·줄바꿈은 무시하고 패딩은 받는다. 잘못된 입력은 None.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits, mut pad) = (Vec::new(), 0u32, 0u32, 0);
    for b in s.bytes().filter(|b| !b.is_ascii_whitespace()) {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                pad += 1;
                continue;
            }
            _ => return None,
        };
        if pad > 0 {
            return None;
        }
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub(crate) async fn get_file(
    api: &GithubApi,
    token: &dyn GithubToken,
    c: &Map<String, Value>,
) -> Result<Map<String, Value>, NodeError> {
    let base = repo_of(c)?;
    let file_path =
        ref_path(get(c, "path")).ok_or_else(|| input_error("path 형식이 올바르지 않습니다."))?;
    let mut query = vec![];
    if !text(get(c, "ref")).is_empty() {
        let r =
            base_ref(get(c, "ref")).ok_or_else(|| input_error("ref 형식이 올바르지 않습니다."))?;
        query.push(("ref", r));
    }
    let t = token.token().await?;
    let data = get_found(api, &t, &format!("{base}/contents/{file_path}"), &query).await?;
    let items = match data {
        None => vec![],
        Some(Value::Array(a)) => a
            .iter()
            .take(MAX_DIR_ITEMS)
            .map(|e| file_item(e, String::new(), false))
            .collect(),
        Some(e) => {
            let (content, truncated) = file_content(&e);
            vec![file_item(&e, content, truncated)]
        }
    };
    Ok(found(items, None))
}
