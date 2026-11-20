//! 칸 목록(소유자·저장소·브랜치·마일스톤) — 편집기 콤보박스가 쓴다. 입력 검사를 끝낸 뒤 토큰을 읽는다.

use ieum_engine::NodeError;
use reqwest::Method;
use serde::Serialize;
use serde_json::{Map, Value};

use super::GithubToken;
use super::api::{GithubApi, input_error};
use super::input::{is_login, is_path_name};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionItem {
    pub id: String,
    pub name: String,
    /// "내 계정"·"조직"·"공개"·"비공개"
    pub hint: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionList {
    pub items: Vec<OptionItem>,
    /// 3쪽(300개)이 다 차서 더 있을 수 있다.
    pub truncated: bool,
}

const PER_PAGE: usize = 100;
const MAX_PAGES: usize = 3;

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

fn item(id: String, name: String, hint: Option<&str>, updated_at: Option<String>) -> OptionItem {
    OptionItem {
        id,
        name,
        hint: hint.map(String::from),
        updated_at,
    }
}

/// 페이지를 최대 3쪽까지 모은다. 첫 쪽 상태가 `tolerate`에 있으면 `None`.
async fn pages(
    api: &GithubApi,
    tok: &str,
    path: &str,
    query: &[(&str, &str)],
    tolerate: &[u16],
) -> Result<Option<(Vec<Value>, bool)>, NodeError> {
    let mut all = Vec::new();
    for page in 1..=MAX_PAGES {
        let mut q: Vec<(&str, String)> = query.iter().map(|(k, v)| (*k, v.to_string())).collect();
        q.push(("per_page", PER_PAGE.to_string()));
        q.push(("page", page.to_string()));
        let r = api.call(tok, Method::GET, path, &q, None).await?;
        if page == 1 && tolerate.contains(&r.status) {
            return Ok(None);
        }
        let Value::Array(rows) = r.expect(200)? else {
            break;
        };
        let n = rows.len();
        all.extend(rows);
        if n < PER_PAGE {
            return Ok(Some((all, false)));
        }
    }
    Ok(Some((all, true)))
}

async fn me(api: &GithubApi, tok: &str) -> Result<String, NodeError> {
    let r = api.call(tok, Method::GET, "/user", &[], None).await?;
    Ok(s(&r.expect(200)?, "login").unwrap_or_default())
}

fn name_of(
    v: &Map<String, Value>,
    key: &str,
    check: fn(&Value) -> bool,
) -> Result<String, NodeError> {
    let val = v
        .get(key)
        .and_then(Value::as_str)
        .map(|x| Value::String(x.trim().into()));
    match val {
        Some(v) if check(&v) => Ok(v.as_str().unwrap_or_default().to_string()),
        _ => Err(input_error("owner/repo 형식이 올바르지 않습니다.")),
    }
}

pub async fn options(
    api: &GithubApi,
    token: &dyn GithubToken,
    source: &str,
    inputs: &Map<String, Value>,
) -> Result<OptionList, NodeError> {
    // 입력 검사 → 토큰 순서.
    let (owner, repo) = match source {
        "github.owners" => (String::new(), String::new()),
        "github.repos" => (name_of(inputs, "owner", is_login)?, String::new()),
        "github.branches" | "github.milestones" => (
            name_of(inputs, "owner", is_path_name)?,
            name_of(inputs, "repo", is_path_name)?,
        ),
        _ => return Err(input_error("지원하지 않는 목록이에요.")),
    };
    let tok = token.token().await?;
    let tok = tok.as_str();
    let (rows, truncated) = match source {
        "github.owners" => {
            let login = me(api, tok).await?;
            let orgs = pages(api, tok, "/user/orgs", &[], &[403, 404]).await?;
            let (orgs, truncated) = orgs.unwrap_or_default();
            let mut items = vec![item(login.clone(), login, Some("내 계정"), None)];
            for o in orgs {
                if let Some(l) = s(&o, "login") {
                    items.push(item(l.clone(), l, Some("조직"), None));
                }
            }
            return Ok(OptionList { items, truncated });
        }
        "github.repos" => {
            let found = if me(api, tok).await?.eq_ignore_ascii_case(&owner) {
                pages(
                    api,
                    tok,
                    "/user/repos",
                    &[("affiliation", "owner"), ("sort", "updated")],
                    &[],
                )
                .await?
            } else {
                let q = [("sort", "updated")];
                match pages(api, tok, &format!("/orgs/{owner}/repos"), &q, &[404]).await? {
                    Some(f) => Some(f),
                    None => pages(api, tok, &format!("/users/{owner}/repos"), &q, &[]).await?,
                }
            };
            let (rows, truncated) = found.unwrap_or_default();
            let items = rows
                .iter()
                .filter_map(|r| {
                    let n = s(r, "name")?;
                    let hint = if r.get("private").and_then(Value::as_bool) == Some(true) {
                        "비공개"
                    } else {
                        "공개"
                    };
                    Some(item(n.clone(), n, Some(hint), s(r, "updated_at")))
                })
                .collect();
            return Ok(OptionList { items, truncated });
        }
        "github.branches" => {
            pages(
                api,
                tok,
                &format!("/repos/{owner}/{repo}/branches"),
                &[],
                &[],
            )
            .await?
        }
        _ => {
            let q = [("state", "open"), ("sort", "due_on")];
            pages(
                api,
                tok,
                &format!("/repos/{owner}/{repo}/milestones"),
                &q,
                &[],
            )
            .await?
        }
    }
    .unwrap_or_default();
    let items = rows
        .iter()
        .filter_map(|r| {
            if source == "github.branches" {
                let n = s(r, "name")?;
                Some(item(n.clone(), n, None, None))
            } else {
                let id = r.get("number")?.as_u64()?.to_string();
                Some(item(id, s(r, "title")?, None, None))
            }
        })
        .collect();
    Ok(OptionList { items, truncated })
}
