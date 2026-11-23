//! GitHub 트리거 판정 — 기준점·새 항목·PR 변화·출력·샘플. 네트워크·시계 없이 순수 함수만.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use ieum_local::github::watch::*;
use serde_json::{Map, Value, json};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 23, 12, 0, 0).unwrap()
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn issue(n: u64, created: DateTime<Utc>) -> Value {
    json!({
        "number": n, "title": format!("t{n}"), "body": "b",
        "html_url": format!("https://github.com/o/r/issues/{n}"),
        "user": {"login": "u"}, "labels": [],
        "repository_url": "https://api.github.com/repos/o/r",
        "created_at": ts(created), "updated_at": ts(created),
    })
}

fn issue_pr(n: u64, created: DateTime<Utc>) -> Value {
    let mut v = issue(n, created);
    v["pull_request"] = json!({"url": "x"});
    v
}

fn pr(n: u64, sha: &str, draft: bool, created: DateTime<Utc>, updated: DateTime<Utc>) -> Value {
    json!({
        "number": n, "title": format!("p{n}"), "body": "b",
        "html_url": format!("https://github.com/o/r/pull/{n}"),
        "user": {"login": "u"}, "author_association": "MEMBER", "draft": draft,
        "created_at": ts(created), "updated_at": ts(updated),
        "base": {"ref": "main", "repo": {"name": "r", "full_name": "o/r", "owner": {"login": "o"}}},
        "head": {"ref": "feat", "sha": sha},
    })
}

fn all_on() -> PrFilter {
    PrFilter::from_config(&Map::new())
}

fn pr_seen(pr_max: u64, open: &[(u64, &str, bool)], complete: bool) -> Seen {
    Seen::Pr {
        pr_max,
        open: open
            .iter()
            .map(|(n, sha, draft)| {
                (
                    *n,
                    PrSeen {
                        head_sha: sha.to_string(),
                        draft: *draft,
                    },
                )
            })
            .collect(),
        open_complete: complete,
    }
}

fn open_of(s: &Seen) -> &BTreeMap<u64, PrSeen> {
    match s {
        Seen::Pr { open, .. } => open,
        _ => panic!("PR 상태가 아님"),
    }
}

fn actions(runs: &[Map<String, Value>]) -> Vec<&str> {
    runs.iter().map(|r| r["action"].as_str().unwrap()).collect()
}

#[test]
fn issue_baseline_counts_prs() {
    let list = [issue(3, now()), issue_pr(7, now())];
    assert_eq!(baseline_issues(&list), Seen::Issue { issue_max: 7 });
    assert_eq!(baseline_issues(&[]), Seen::Issue { issue_max: 0 });
}

#[test]
fn issue_new_runs_and_skips_prs_and_own() {
    let seen = Seen::Issue { issue_max: 7 };
    let list = [
        issue(10, now()),
        issue_pr(9, now()),
        issue(8, now() - Duration::minutes(1)),
    ];
    let (runs, next) = diff_issues(&seen, &list, now(), &|n| n == 10);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["issueNumber"], 8);
    assert_eq!(next, Seen::Issue { issue_max: 10 });
}

#[test]
fn issue_outside_grace_recorded_only() {
    let seen = Seen::Issue { issue_max: 7 };
    let list = [issue(8, now() - Duration::hours(3))];
    let (runs, next) = diff_issues(&seen, &list, now(), &|_| false);
    assert!(runs.is_empty());
    assert_eq!(next, Seen::Issue { issue_max: 8 });
}

#[test]
fn issue_runs_oldest_first() {
    let seen = Seen::Issue { issue_max: 7 };
    let list = [
        issue(9, now() - Duration::minutes(1)),
        issue(8, now() - Duration::minutes(2)),
    ];
    let (runs, _) = diff_issues(&seen, &list, now(), &|_| false);
    let nums: Vec<_> = runs.iter().map(|r| r["issueNumber"].clone()).collect();
    assert_eq!(nums, [json!(8), json!(9)]);
}

#[test]
fn issue_output_shape() {
    let mut v = issue(8, now());
    v["labels"] = json!([{"name": "bug"}, {"name": ""}, {"name": "ui"}]);
    v["body"] = Value::Null;
    v["user"] = Value::Null;
    v["created_at"] = json!("2026-11-22T10:02:03+09:00");
    let out = issue_output(&v);
    assert_eq!(out["issueNumber"], 8);
    assert_eq!(out["title"], "t8");
    assert_eq!(out["url"], "https://github.com/o/r/issues/8");
    assert_eq!(out["labels"], json!(["bug", "ui"]));
    assert_eq!(out["body"], "");
    assert_eq!(out["author"], "");
    assert_eq!(out["repo"], "o/r");
    assert_eq!(out["createdAt"], "2026-11-22T01:02:03Z");

    // repository_url이 없으면 html_url에서
    v.as_object_mut().unwrap().remove("repository_url");
    v["created_at"] = json!("어제");
    let out = issue_output(&v);
    assert_eq!(out["repo"], "o/r");
    assert_eq!(out["createdAt"], "어제");
}

#[test]
fn pr_opened_reopened_ready_sync() {
    let seen = pr_seen(10, &[(5, "a", true), (6, "b", false)], true);
    let last = now() - Duration::minutes(1);
    let s = |sec| now() - Duration::seconds(sec);
    let old = now() - Duration::days(30);
    let list = [
        pr(6, "c", false, old, s(5)),
        pr(5, "a", false, old, s(10)),
        pr(4, "z", false, old, s(20)),
        pr(11, "n", false, s(30), s(30)),
    ];
    let (runs, next) = diff_prs(&seen, &list, &all_on(), last, now());
    assert_eq!(
        actions(&runs),
        ["opened", "reopened", "ready_for_review", "synchronize"]
    );
    let want = pr_seen(
        11,
        &[
            (4, "z", false),
            (5, "a", false),
            (6, "c", false),
            (11, "n", false),
        ],
        true,
    );
    assert_eq!(next, want);
}

#[test]
fn pr_new_number_created_before_last_check_is_reopened() {
    let seen = pr_seen(10, &[], true);
    let last = now() - Duration::minutes(1);
    let list = [pr(11, "n", false, last - Duration::hours(5), now())];
    let (runs, _) = diff_prs(&seen, &list, &all_on(), last, now());
    assert_eq!(actions(&runs), ["reopened"]);
}

#[test]
fn pr_priority_one_action() {
    let seen = pr_seen(10, &[(5, "a", true)], true);
    let last = now() - Duration::minutes(1);
    let list = [pr(5, "b", false, now() - Duration::days(1), now())];
    let (runs, _) = diff_prs(&seen, &list, &all_on(), last, now());
    assert_eq!(actions(&runs), ["ready_for_review"]);
}

#[test]
fn pr_filters_member_toggle_drafts() {
    let last = now() - Duration::minutes(1);
    let old = now() - Duration::days(1);

    // 멤버가 아니면 안 돈다
    let seen = pr_seen(10, &[(6, "b", false)], true);
    let mut outsider = pr(6, "c", false, old, now());
    outsider["author_association"] = json!("CONTRIBUTOR");
    let (runs, _) = diff_prs(&seen, &[outsider], &all_on(), last, now());
    assert!(runs.is_empty());

    // 토글이 꺼져 있으면 안 돈다
    let mut c = Map::new();
    c.insert("onSynchronize".into(), json!(false));
    let (runs, _) = diff_prs(
        &seen,
        &[pr(6, "c", false, old, now())],
        &PrFilter::from_config(&c),
        last,
        now(),
    );
    assert!(runs.is_empty());

    // 초안 빼기: 새 초안 PR은 안 돌지만 기록되고, 나중에 준비 완료되면 ready_for_review
    let mut c = Map::new();
    c.insert("excludeDrafts".into(), json!("true"));
    let filter = PrFilter::from_config(&c);
    assert!(filter.exclude_drafts);
    let seen = pr_seen(10, &[], true);
    let created = now() - Duration::seconds(30);
    let (runs, next) = diff_prs(
        &seen,
        &[pr(11, "n", true, created, created)],
        &filter,
        last,
        now(),
    );
    assert!(runs.is_empty());
    assert_eq!(
        open_of(&next)[&11],
        PrSeen {
            head_sha: "n".into(),
            draft: true
        }
    );
    let (runs, _) = diff_prs(
        &next,
        &[pr(11, "n", false, created, now())],
        &filter,
        now(),
        now() + Duration::minutes(1),
    );
    assert_eq!(actions(&runs), ["ready_for_review"]);
}

#[test]
fn pr_filter_false_only_when_false() {
    let mut c = Map::new();
    c.insert("onOpened".into(), json!("false"));
    c.insert("onReopened".into(), json!(false));
    c.insert("onSynchronize".into(), json!(""));
    c.insert("excludeDrafts".into(), Value::Null);
    let f = PrFilter::from_config(&c);
    assert!(!f.on_opened && !f.on_reopened);
    assert!(f.on_synchronize && f.on_ready_for_review && f.exclude_drafts);
}

#[test]
fn pr_closed_removed_when_complete() {
    let seen = pr_seen(10, &[(5, "a", false), (6, "b", false)], true);
    let old = now() - Duration::days(1);
    let (_, next) = diff_prs(
        &seen,
        &[pr(5, "a", false, old, old)],
        &all_on(),
        now() - Duration::minutes(1),
        now(),
    );
    assert!(!open_of(&next).contains_key(&6));
    assert!(matches!(
        next,
        Seen::Pr {
            open_complete: true,
            ..
        }
    ));
}

#[test]
fn pr_truncated_keeps_open_and_records_unknown() {
    let old = now() - Duration::days(1);
    let last = now() - Duration::minutes(1);
    let seen = pr_seen(200, &[(6, "b", false)], true);
    let full: Vec<Value> = (101..=200).map(|n| pr(n, "s", false, old, old)).collect();
    assert_eq!(full.len(), PAGE);
    let (runs, next) = diff_prs(&seen, &full, &all_on(), last, now());
    assert!(runs.is_empty());
    assert!(open_of(&next).contains_key(&6));
    assert!(matches!(
        next,
        Seen::Pr {
            open_complete: false,
            ..
        }
    ));

    // 목록이 잘렸던 동안 처음 보는 옛 번호는 다시 열림인지 알 수 없다 — 기록만
    let (runs, after) = diff_prs(
        &next,
        &[pr(3, "t", false, old, now())],
        &all_on(),
        now(),
        now(),
    );
    assert!(runs.is_empty());
    assert!(open_of(&after).contains_key(&3));
}

#[test]
fn pr_outside_grace_recorded_only() {
    let seen = pr_seen(10, &[(6, "b", false)], true);
    let old = now() - Duration::days(1);
    let (runs, next) = diff_prs(
        &seen,
        &[pr(6, "c", false, old, now() - Duration::hours(3))],
        &all_on(),
        now() - Duration::minutes(1),
        now(),
    );
    assert!(runs.is_empty());
    assert_eq!(open_of(&next)[&6].head_sha, "c");
}

#[test]
fn pr_baseline_takes_larger_number() {
    let old = now() - Duration::days(1);
    let list = [pr(5, "a", true, old, old)];
    assert_eq!(
        baseline_prs(&list, Some(9)),
        pr_seen(9, &[(5, "a", true)], true)
    );
    assert_eq!(baseline_prs(&[], None), pr_seen(0, &[], true));
}

#[test]
fn pr_output_shape() {
    let mut v = pr(11, "abc", true, now(), now());
    v["body"] = Value::Null;
    let out = pr_output(&v, "opened");
    assert_eq!(out["owner"], "o");
    assert_eq!(out["repoName"], "r");
    assert_eq!(out["repo"], "o/r");
    assert_eq!(out["prNumber"], 11);
    assert_eq!(out["title"], "p11");
    assert_eq!(out["body"], "");
    assert_eq!(out["url"], "https://github.com/o/r/pull/11");
    assert_eq!(out["author"], "u");
    assert_eq!(out["action"], "opened");
    assert_eq!(out["draft"], true);
    assert_eq!(out["baseBranch"], "main");
    assert_eq!(out["headBranch"], "feat");
    assert_eq!(out["headSha"], "abc");
    assert_eq!(out["createdAt"], "2026-11-23T12:00:00Z");
}

#[test]
fn samples() {
    let issues = [issue_pr(9, now()), issue(8, now()), issue(7, now())];
    assert_eq!(pick_issue_sample(&issues).unwrap()["issueNumber"], 8);
    assert!(pick_issue_sample(&[issue_pr(9, now())]).is_none());

    let mut outsider = pr(9, "a", false, now(), now());
    outsider["author_association"] = json!("NONE");
    let prs = [
        outsider,
        pr(8, "b", true, now(), now()),
        pr(7, "c", false, now(), now()),
    ];
    let s = pick_pr_sample(&prs, false).unwrap();
    assert_eq!(
        (s["prNumber"].clone(), s["action"].clone()),
        (json!(8), json!("opened"))
    );
    assert_eq!(pick_pr_sample(&prs, true).unwrap()["prNumber"], 7);
    assert!(pick_pr_sample(&prs[..2], true).is_none());
}

#[test]
fn config_hash_stable() {
    let a = json!({"owner": "Octo", "repo": "Hello", "onOpened": true});
    let b = json!({"onOpened": true, "repo": "hello", "owner": "octo"});
    let c = json!({"owner": "octo", "repo": "other", "onOpened": true});
    let h = |v: &Value| config_hash("github.new_pull_request", v.as_object().unwrap());
    assert_eq!(h(&a), h(&b));
    assert_ne!(h(&a), h(&c));
    assert_ne!(
        h(&a),
        config_hash("github.new_issue", a.as_object().unwrap())
    );
}

#[test]
fn created_issues_case_insensitive() {
    let c = CreatedIssues::in_memory();
    c.record("MyOrg", "Repo", 5, now());
    assert!(c.contains("myorg", "repo", 5, now()));
    assert!(!c.contains("myorg", "repo", 6, now()));
}

#[test]
fn created_issues_expire_after_24h() {
    let c = CreatedIssues::in_memory();
    c.record("o", "r", 5, now());
    assert!(c.contains("o", "r", 5, now() + Duration::hours(23)));
    assert!(!c.contains("o", "r", 5, now() + Duration::hours(25)));
}

#[test]
fn created_issues_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("created.json");
    CreatedIssues::open(path.clone()).record("o", "r", 5, now());
    assert!(CreatedIssues::open(path.clone()).contains("o", "r", 5, now()));
    std::fs::write(&path, "깨짐").unwrap();
    assert!(!CreatedIssues::open(path).contains("o", "r", 5, now()));
}
