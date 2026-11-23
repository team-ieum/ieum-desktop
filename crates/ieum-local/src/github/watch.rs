//! GitHub 트리거(새 이슈·새 PR) 판정 — 1분마다 받은 목록과 저장한 상태를 견줘 실행할 항목을 고른다.
//! 네트워크·시계를 읽지 않는다(시각은 인자). 클라우드는 웹훅 이벤트마다 돌지만 여기선 확인 사이의 변화를
//! PR당 하나로 판정한다 — 차이는 `conformance/GAPS.md` "GitHub 트리거".

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use ieum_engine::schedule::GRACE;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// 목록 한 번에 받는 건수(`per_page`). 꽉 차면 목록이 잘렸을 수 있다.
pub const PAGE: usize = 100;
/// BE와 같은 멤버 판정 — 이들이 연 PR만 실행한다.
const MEMBERS: [&str; 3] = ["OWNER", "MEMBER", "COLLABORATOR"];
/// 새 번호 PR의 `created_at`을 마지막 확인과 견줄 때 시계 차이 여유.
const SKEW: Duration = Duration::minutes(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    Issue,
    Pr,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrSeen {
    pub head_sha: String,
    pub draft: bool,
}

/// 마지막 확인에서 본 것. 상태 항목이 없으면 "기준점 없음" — 빈 저장소도 `*_max: 0`인 항목을 둔다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Seen {
    /// 이슈·PR은 번호를 함께 쓰므로 PR 번호까지 포함한 최댓값.
    Issue { issue_max: u64 },
    /// `open_complete`: 마지막 목록이 `PAGE` 미만이라 `open`이 열린 PR 전부인지.
    Pr {
        pr_max: u64,
        open: BTreeMap<u64, PrSeen>,
        open_complete: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WatchEntry {
    pub owner: String,
    pub repo: String,
    pub config_hash: String,
    pub last_checked_at: DateTime<Utc>,
    pub seen: Seen,
}

/// PR 트리거 토글.
pub struct PrFilter {
    pub on_opened: bool,
    pub on_synchronize: bool,
    pub on_reopened: bool,
    pub on_ready_for_review: bool,
    pub exclude_drafts: bool,
}

impl PrFilter {
    /// BE처럼 `false`일 때만 끈다 — 키가 없거나 그 밖의 값은 켬.
    pub fn from_config(c: &Map<String, Value>) -> Self {
        let on = |key: &str| !matches!(c.get(key), Some(v) if *v == false || v == "false");
        PrFilter {
            on_opened: on("onOpened"),
            on_synchronize: on("onSynchronize"),
            on_reopened: on("onReopened"),
            on_ready_for_review: on("onReadyForReview"),
            exclude_drafts: on("excludeDrafts"),
        }
    }

    fn allows(&self, action: &str) -> bool {
        match action {
            "opened" => self.on_opened,
            "reopened" => self.on_reopened,
            "ready_for_review" => self.on_ready_for_review,
            _ => self.on_synchronize,
        }
    }
}

fn number(v: &Value) -> u64 {
    v["number"].as_u64().unwrap_or(0)
}

fn time(v: &Value, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v[key].as_str()?)
        .ok()
        .map(|t| t.to_utc())
}

/// 유예 안인지. 시각을 읽지 못하면 밖으로 친다(실행 안 함, 상태만).
fn within(t: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    t.is_some_and(|t| now - t <= GRACE)
}

fn is_pr(v: &Value) -> bool {
    v.get("pull_request").is_some()
}

fn text(v: &Value) -> Value {
    v.as_str().unwrap_or("").into()
}

/// BE처럼 UTC `Z`로. 읽지 못하면 원문.
fn created_at(v: &Value) -> Value {
    let raw = v["created_at"].as_str().unwrap_or("");
    match DateTime::parse_from_rfc3339(raw) {
        Ok(t) => t
            .to_utc()
            .to_rfc3339_opts(SecondsFormat::AutoSi, true)
            .into(),
        Err(_) => raw.into(),
    }
}

/// `owner/name` — `repository_url`(`…/repos/{o}/{r}`)에서, 없으면 `html_url`(`https://github.com/{o}/{r}/…`)에서.
fn issue_repo(v: &Value) -> String {
    if let Some((_, rest)) = v["repository_url"]
        .as_str()
        .and_then(|u| u.split_once("/repos/"))
    {
        return rest.trim_end_matches('/').to_string();
    }
    let parts: Vec<&str> = v["html_url"].as_str().unwrap_or("").split('/').collect();
    match parts.get(3..5) {
        Some([o, r]) => format!("{o}/{r}"),
        _ => String::new(),
    }
}

fn max_number(list: &[Value]) -> u64 {
    list.iter().map(number).max().unwrap_or(0)
}

/// 시각순(오래된 것부터)으로 정렬한 실행 목록.
fn oldest_first(mut runs: Vec<(DateTime<Utc>, Map<String, Value>)>) -> Vec<Map<String, Value>> {
    runs.sort_by_key(|(t, _)| *t);
    runs.into_iter().map(|(_, m)| m).collect()
}

pub fn issue_output(v: &Value) -> Map<String, Value> {
    let labels: Vec<Value> = v["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["name"].as_str().filter(|s| !s.is_empty()))
        .map(Value::from)
        .collect();
    let out = json!({
        "issueNumber": v["number"],
        "title": v["title"],
        "body": text(&v["body"]),
        "url": v["html_url"],
        "author": text(&v["user"]["login"]),
        "labels": labels,
        "repo": issue_repo(v),
        "createdAt": created_at(v),
    });
    match out {
        Value::Object(m) => m,
        _ => unreachable!("json! 객체"),
    }
}

pub fn pr_output(v: &Value, action: &str) -> Map<String, Value> {
    let repo = &v["base"]["repo"];
    let owner = repo["owner"]["login"].as_str().unwrap_or("");
    let name = repo["name"].as_str().unwrap_or("");
    let out = json!({
        "owner": owner,
        "repoName": name,
        "repo": format!("{owner}/{name}"),
        "prNumber": v["number"],
        "title": v["title"],
        "body": text(&v["body"]),
        "url": v["html_url"],
        "author": text(&v["user"]["login"]),
        "action": action,
        "draft": v["draft"].as_bool().unwrap_or(false),
        "baseBranch": v["base"]["ref"],
        "headBranch": v["head"]["ref"],
        "headSha": v["head"]["sha"],
        "createdAt": created_at(v),
    });
    match out {
        Value::Object(m) => m,
        _ => unreachable!("json! 객체"),
    }
}

/// `/issues` 응답 전체(PR 포함)의 최대 번호. 빈 목록이면 0.
pub fn baseline_issues(list: &[Value]) -> Seen {
    Seen::Issue {
        issue_max: max_number(list),
    }
}

/// 번호가 `issue_max`보다 큰 이슈(PR·이 앱이 만든 이슈 제외) 중 유예 안의 것. 다시 열린 옛 이슈는 잡지 않는다(BE와 같음).
pub fn diff_issues(
    seen: &Seen,
    list: &[Value],
    now: DateTime<Utc>,
    is_own: &dyn Fn(u64) -> bool,
) -> (Vec<Map<String, Value>>, Seen) {
    let Seen::Issue { issue_max } = *seen else {
        return (Vec::new(), baseline_issues(list));
    };
    if list.len() >= PAGE && list.iter().all(|v| number(v) > issue_max) {
        eprintln!("[ieum] GitHub 새 이슈가 한 번에 {PAGE}건을 넘어 받은 것만 처리해요");
    }
    let runs = list
        .iter()
        .filter(|v| number(v) > issue_max && !is_pr(v) && !is_own(number(v)))
        .filter_map(|v| {
            let t = time(v, "created_at").filter(|t| now - *t <= GRACE)?;
            Some((t, issue_output(v)))
        })
        .collect();
    let next = Seen::Issue {
        issue_max: issue_max.max(max_number(list)),
    };
    (oldest_first(runs), next)
}

fn pr_seen(v: &Value) -> PrSeen {
    PrSeen {
        head_sha: v["head"]["sha"].as_str().unwrap_or("").to_string(),
        draft: v["draft"].as_bool().unwrap_or(false),
    }
}

/// 열린 PR 목록 + 기준점 때만 따로 받은 전체 최신 번호(`latest_any`) 중 큰 쪽 — 더 큰 번호의 닫힌 PR이
/// 다시 열릴 때 `opened`로 오판하지 않게.
pub fn baseline_prs(open: &[Value], latest_any: Option<u64>) -> Seen {
    Seen::Pr {
        pr_max: max_number(open).max(latest_any.unwrap_or(0)),
        open: open.iter().map(|v| (number(v), pr_seen(v))).collect(),
        open_complete: open.len() < PAGE,
    }
}

/// PR마다 변화 하나(opened > reopened > ready_for_review > synchronize)와 그 기준 시각.
/// `None`이면 실행할 변화가 없거나 알 수 없다(목록이 잘렸던 동안 처음 보는 옛 번호 — GAPS).
fn pr_action(
    v: &Value,
    pr_max: u64,
    open: &BTreeMap<u64, PrSeen>,
    open_complete: bool,
    last_checked: DateTime<Utc>,
) -> Option<(&'static str, Option<DateTime<Utc>>)> {
    let n = number(v);
    let updated = time(v, "updated_at");
    if n > pr_max {
        let created = time(v, "created_at");
        return Some(match created {
            Some(c) if c < last_checked - SKEW => ("reopened", updated),
            _ => ("opened", created),
        });
    }
    let Some(prev) = open.get(&n) else {
        return open_complete.then_some(("reopened", updated));
    };
    let now = pr_seen(v);
    if prev.draft && !now.draft {
        Some(("ready_for_review", updated))
    } else if prev.head_sha != now.head_sha {
        Some(("synchronize", updated))
    } else {
        None
    }
}

/// 실행하지 않은 PR도 상태(`open`·`pr_max`)에는 늘 기록한다 — 초안이 나중에 준비 완료될 때
/// `opened`가 아니라 `ready_for_review`로 잡히게.
pub fn diff_prs(
    seen: &Seen,
    open: &[Value],
    filter: &PrFilter,
    last_checked: DateTime<Utc>,
    now: DateTime<Utc>,
) -> (Vec<Map<String, Value>>, Seen) {
    let Seen::Pr {
        pr_max,
        open: prev,
        open_complete,
    } = seen
    else {
        return (Vec::new(), baseline_prs(open, None));
    };
    let truncated = open.len() >= PAGE;
    // 목록이 잘렸으면 목록에 없는 PR을 닫힌 것으로 치지 않는다.
    let mut next = if truncated {
        prev.clone()
    } else {
        BTreeMap::new()
    };
    let mut runs = Vec::new();
    for v in open {
        if let Some((action, t)) = pr_action(v, *pr_max, prev, *open_complete, last_checked) {
            let member = MEMBERS.contains(&v["author_association"].as_str().unwrap_or(""));
            let draft_ok = !(filter.exclude_drafts
                && v["draft"].as_bool().unwrap_or(false)
                && action != "ready_for_review");
            if member && filter.allows(action) && draft_ok && within(t, now) {
                runs.extend(t.map(|t| (t, pr_output(v, action))));
            }
        }
        next.insert(number(v), pr_seen(v));
    }
    let seen = Seen::Pr {
        pr_max: (*pr_max).max(max_number(open)),
        open: next,
        open_complete: !truncated,
    };
    (oldest_first(runs), seen)
}

/// [테스트]·[지금 실행]용 — 최근 목록에서 PR을 뺀 첫 이슈.
pub fn pick_issue_sample(list: &[Value]) -> Option<Map<String, Value>> {
    list.iter().find(|v| !is_pr(v)).map(issue_output)
}

/// BE와 같게 멤버가 연 PR만, `exclude_drafts`면 초안도 빼고 첫 PR. 토글은 적용하지 않는다.
pub fn pick_pr_sample(list: &[Value], exclude_drafts: bool) -> Option<Map<String, Value>> {
    list.iter()
        .find(|v| {
            MEMBERS.contains(&v["author_association"].as_str().unwrap_or(""))
                && !(exclude_drafts && v["draft"].as_bool().unwrap_or(false))
        })
        .map(|v| pr_output(v, "opened"))
}

/// 트리거 칸이 바뀌었는지 보는 값. 키 순서와 무관하고, owner·repo는 대소문자를 가리지 않는다(GitHub 저장소 이름).
pub fn config_hash(event: &str, config: &Map<String, Value>) -> String {
    let mut config = config.clone();
    for key in ["owner", "repo"] {
        if let Some(Value::String(s)) = config.get_mut(key) {
            *s = s.to_lowercase();
        }
    }
    let node = serde_json::from_value(json!({ "id": "", "event": event, "config": config }))
        .expect("id·event·config만 있는 노드는 항상 읽힌다");
    crate::builder::node_hash(&node)
}
