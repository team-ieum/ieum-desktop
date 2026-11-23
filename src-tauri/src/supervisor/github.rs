//! GitHub 트리거 확인 루프(설계 "확인 루프"·"새 항목 가리기"·"오류·한도"). 스케줄 루프와 따로 도는 async 작업.
//! 네트워크를 기다리는 동안 std 잠금을 잡지 않는다 — 요청을 먼저 다 하고, `tick_lock` 안에서 저장본을 다시 읽어
//! 그사이 끄거나 칸을 바꾸거나 껐다 켠(세대가 다른) 워크플로우의 결과는 버린다.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use ieum_engine::schedule::SCHEDULE_ZONE;
use ieum_engine::{Node, NodeError};
use ieum_local::github::watch::{
    PAGE, PrFilter, WatchEntry, WatchKind, baseline_issues, baseline_prs, diff_issues, diff_prs,
    pick_issue_sample, pick_pr_sample,
};
use ieum_local::github::{Resp, Secret, is_path_name};
use serde_json::{Map, Value};

use super::{Job, JobSource, Pushed, Supervisor, enabled_github, github_hash, lock, trigger_sig};

const NEED_CONNECT: &str = "GitHub 연결이 필요해요.";

/// 요청이 GitHub에 닿지 못했을 때(연결 실패·시간 초과) 샘플 요청에 보일 문구.
const UNREACHABLE: &str = "GitHub에 닿지 못했어요. 잠시 뒤 다시 해 주세요.";

/// 연결은 그대로인데 gh·GitHub에 잠시 닿지 못한 토큰 오류.
fn unreachable(e: &NodeError) -> bool {
    e.code.as_deref() == Some("GITHUB_UNREACHABLE")
}

/// (소문자 owner, 소문자 repo, 종류) — 같은 저장소를 보는 워크플로우가 여럿이어도 요청은 하나.
pub(super) type RepoKey = (String, String, WatchKind);

/// 이번 확인에서 볼 워크플로우 하나(확인 시작 때의 값).
struct Target {
    id: String,
    kind: WatchKind,
    owner: String,
    repo: String,
    key: RepoKey,
    hash: String,
    generation: u64,
    config: Map<String, Value>,
}

/// 저장소 하나의 확인 결과.
enum Got {
    /// 목록 + 기준점용 전체 최신 PR 번호(PR 기준점이 필요할 때만).
    List(Vec<Value>, Option<u64>),
    /// 그 저장소를 보는 워크플로우에 보일 문구.
    Error(&'static str),
}

/// 요청 하나의 결과.
enum Fetched {
    Ok(Resp),
    Error(&'static str),
    /// 일시 오류(5xx·연결 실패 등) — 상태도 오류 표시도 건드리지 않고 다음 확인에서 다시.
    Skip,
    /// 이번 확인 중단 — GitHub 트리거 워크플로우 모두에 보일 문구.
    Stop(String),
}

fn list_query(state: &str, sort: &str, per_page: usize) -> Vec<(&'static str, String)> {
    vec![
        ("state", state.to_string()),
        ("sort", sort.to_string()),
        ("direction", "desc".to_string()),
        ("per_page", per_page.to_string()),
    ]
}

/// Asia/Seoul "HH:MM".
fn hhmm(t: DateTime<Utc>) -> String {
    t.with_timezone(&SCHEDULE_ZONE).format("%H:%M").to_string()
}

impl Supervisor {
    /// `github_poll`마다, 또는 깨우면(기준점 없는 트리거가 생김) 확인한다. 루프는 `Inner`를 약하게만 쥔다.
    pub(super) fn spawn_github_loop(&self) {
        let (weak, stopped) = (std::sync::Arc::downgrade(&self.0), self.0.stopped.clone());
        let (wake, every) = (self.0.github_wake.clone(), self.0.deps.github_poll);
        // 앱을 켤 때 켜진 GitHub 트리거가 있으면 바로 한 번 — 기준점 없는 것을 1분 기다리지 않게.
        let any = self
            .store()
            .list_workflows()
            .is_ok_and(|l| l.iter().any(|w| enabled_github(w).is_some()));
        if any {
            wake.notify_one();
        }
        self.0.rt.spawn(async move {
            loop {
                tokio::select! {
                    () = stopped.cancelled() => break,
                    () = tokio::time::sleep(every) => {}
                    () = wake.notified() => {}
                }
                match weak.upgrade() {
                    Some(inner) => Supervisor(inner).github_tick().await,
                    None => break,
                }
            }
        });
    }

    #[doc(hidden)]
    pub async fn github_tick_for_test(&self) {
        self.github_tick().await;
    }

    /// 확인 한 번. 켜진 GitHub 트리거가 없으면 토큰도 읽지 않는다.
    async fn github_tick(&self) {
        if self.is_stopped() {
            return;
        }
        let now = (self.0.deps.now)();
        if lock(&self.0.github_pause).is_some_and(|p| p > now) {
            return;
        }
        let list = match self.store().list_workflows() {
            Ok(l) => l,
            Err(e) => return eprintln!("[ieum] GitHub 트리거를 확인하지 못했어요: {e}"),
        };
        let gens = lock(&self.0.github_gen).clone();
        let targets: Vec<Target> = list
            .iter()
            .filter_map(|wf| {
                let (kind, owner, repo) = enabled_github(wf)?;
                Some(Target {
                    id: wf.id.clone(),
                    key: (owner.to_lowercase(), repo.to_lowercase(), kind),
                    hash: github_hash(wf)?,
                    generation: gens.get(&wf.id).copied().unwrap_or(0),
                    config: trigger_sig(wf)?.1,
                    kind,
                    owner,
                    repo,
                })
            })
            .collect();
        // 꺼지거나 지워진 워크플로우의 오류는 잊는다.
        lock(&self.0.github_errors).retain(|id, _| targets.iter().any(|t| t.id == *id));
        if targets.is_empty() {
            return;
        }
        let mut token = match self.0.deps.github_watch.token(false).await {
            Ok(t) => t,
            // 잠시 닿지 못함(잠자기 해제 직후 등) — 연결 해제가 아니니 오류 없이 다음 확인에서 다시.
            Err(e) if unreachable(&e) => return,
            Err(_) => return self.github_fail_all(&targets, NEED_CONNECT),
        };
        let state = self.github_state();
        let needs_baseline = |k: &RepoKey| {
            targets
                .iter()
                .any(|t| t.key == *k && state.get(&t.id).is_none_or(|e| e.config_hash != t.hash))
        };
        let mut keys: Vec<&RepoKey> = Vec::new();
        for t in &targets {
            if !keys.contains(&&t.key) {
                keys.push(&t.key);
            }
        }
        let (mut got, mut rechecked) = (HashMap::new(), false);
        for key in keys {
            let (owner, repo, kind) = key;
            let (path, query) = match kind {
                WatchKind::Issue => (
                    format!("/repos/{owner}/{repo}/issues"),
                    list_query("all", "created", PAGE),
                ),
                WatchKind::Pr => (
                    format!("/repos/{owner}/{repo}/pulls"),
                    list_query("open", "updated", PAGE),
                ),
            };
            let etag = lock(&self.0.github_cache).get(key).map(|(e, _)| e.clone());
            let fetched = self
                .github_get(
                    &mut token,
                    &mut rechecked,
                    &path,
                    &query,
                    etag.as_deref(),
                    true,
                )
                .await;
            let items = match fetched {
                Fetched::Stop(msg) => return self.github_fail_all(&targets, &msg),
                Fetched::Skip => continue,
                Fetched::Error(msg) => {
                    got.insert(key.clone(), Got::Error(msg));
                    continue;
                }
                // 304는 한도를 쓰지 않는다 — 마지막 목록으로 같은 판정을 지나 `last_checked_at`을 갱신한다.
                Fetched::Ok(r) if r.status == 304 => match lock(&self.0.github_cache).get(key) {
                    Some((_, l)) => l.clone(),
                    None => continue,
                },
                Fetched::Ok(r) => {
                    let Value::Array(items) = r.body else {
                        continue;
                    };
                    let mut cache = lock(&self.0.github_cache);
                    match r.etag {
                        Some(e) => cache.insert(key.clone(), (e, items.clone())),
                        None => cache.remove(key),
                    };
                    items
                }
            };
            // PR 기준점은 열린 목록만으로 잡지 않는다 — 더 큰 번호의 닫힌 PR이 다시 열릴 때 opened로 오판하지 않게.
            let mut latest = None;
            if *kind == WatchKind::Pr && needs_baseline(key) {
                let query = list_query("all", "created", 1);
                match self
                    .github_get(&mut token, &mut rechecked, &path, &query, None, true)
                    .await
                {
                    Fetched::Stop(msg) => return self.github_fail_all(&targets, &msg),
                    Fetched::Error(msg) => {
                        got.insert(key.clone(), Got::Error(msg));
                        continue;
                    }
                    Fetched::Ok(r) if r.status == 200 => {
                        let first = r.body.as_array().and_then(|a| a.first());
                        latest = Some(first.and_then(|v| v["number"].as_u64()).unwrap_or(0));
                    }
                    // 기준점은 다음 확인에서.
                    _ => {}
                }
            }
            got.insert(key.clone(), Got::List(items, latest));
        }
        self.github_commit(&targets, &got, now);
    }

    /// 테스트·지금 실행용 샘플 — 그 저장소의 최신 실제 항목(PR은 멤버가 연 것만)으로 트리거 출력을 만든다.
    /// 요청·오류 문구는 확인 루프와 같은 길(`github_get`)을 탄다 — 본문·토큰은 문구에 싣지 않는다.
    pub(super) async fn github_sample(&self, node: &Node) -> Result<Map<String, Value>, String> {
        let text = |k: &str| node.config.get(k).and_then(Value::as_str).unwrap_or("");
        let (owner, repo) = (text("owner"), text("repo"));
        let usable = |s: &str| !s.is_empty() && !s.contains("{{") && is_path_name(&Value::from(s));
        if !usable(owner) || !usable(repo) {
            return Err("저장소를 먼저 골라 주세요.".into());
        }
        let (owner, repo) = (owner.to_lowercase(), repo.to_lowercase());
        let issue = node.event == "github.new_issue";
        let path = format!(
            "/repos/{owner}/{repo}/{}",
            if issue { "issues" } else { "pulls" }
        );
        let mut token = match self.0.deps.github_watch.token(false).await {
            Ok(t) => t,
            Err(e) if unreachable(&e) => return Err(UNREACHABLE.into()),
            Err(_) => return Err(NEED_CONNECT.into()),
        };
        let query = list_query("all", "created", 20);
        let r = match self
            .github_get(&mut token, &mut false, &path, &query, None, false)
            .await
        {
            Fetched::Ok(r) => r,
            Fetched::Error(m) => return Err(m.into()),
            Fetched::Stop(m) => return Err(m),
            Fetched::Skip => return Err(UNREACHABLE.into()),
        };
        let list = r.body.as_array().map_or(&[][..], Vec::as_slice);
        if issue {
            pick_issue_sample(list).ok_or_else(|| "이 저장소에 이슈가 없어요.".into())
        } else {
            let exclude_drafts = PrFilter::from_config(&node.config).exclude_drafts;
            pick_pr_sample(list, exclude_drafts)
                .ok_or_else(|| "이 저장소에 고를 수 있는 PR이 없어요(멤버가 연 PR만).".into())
        }
    }

    /// GET 하나. 401이면 이번 확인에서 한 번만 전체 연결 확인을 다시 하고 새 토큰으로 다시 요청한다.
    async fn github_get(
        &self,
        token: &mut Secret,
        rechecked: &mut bool,
        path: &str,
        query: &[(&str, String)],
        etag: Option<&str>,
        polling: bool,
    ) -> Fetched {
        let api = self.0.deps.github.api();
        let r = loop {
            let Ok(r) = api.get_if(token.expose(), path, query, etag).await else {
                return Fetched::Skip;
            };
            if r.status != 401 {
                break r;
            }
            if std::mem::replace(rechecked, true) {
                return Fetched::Stop(NEED_CONNECT.into());
            }
            match self.0.deps.github_watch.token(true).await {
                Ok(t) => *token = t,
                Err(e) if unreachable(&e) => return Fetched::Skip,
                Err(_) => return Fetched::Stop(NEED_CONNECT.into()),
            }
        };
        let now = (self.0.deps.now)();
        let until = r
            .reset_at
            .and_then(|s| DateTime::from_timestamp(s, 0))
            .unwrap_or(now + chrono::Duration::minutes(1));
        // 쉬기는 폴러만 정한다 — 샘플이 쉬게 하면 오류 표시 없이 폴러가 멈춘다. 다음 확인이 같은 응답으로
        // 쉬기와 오류 문구를 함께 정한다.
        if matches!(r.status, 403 | 429) && r.rate_limited {
            if polling {
                *lock(&self.0.github_pause) = Some(until);
            }
            return Fetched::Stop(format!("GitHub 한도 초과 — {}에 다시 확인", hhmm(until)));
        }
        // 사용자의 다른 gh 작업 몫을 남긴다 — 남은 한도가 10% 아래면 풀릴 때까지 쉰다.
        if let (Some(remaining), Some(limit)) = (r.remaining, r.limit)
            && remaining * 10 < limit
        {
            // 사용자가 직접 누른 샘플은 쉬지 않고 아래에서 상태대로 판정한다 — 쉬는 것은 폴러만.
            if polling {
                *lock(&self.0.github_pause) = Some(until);
                return Fetched::Stop(format!(
                    "GitHub 요청 여유가 적어 {}에 다시 확인해요",
                    hhmm(until)
                ));
            }
        }
        match r.status {
            200 | 304 => Fetched::Ok(r),
            404 => Fetched::Error("저장소를 찾을 수 없어요."),
            403 => Fetched::Error("저장소를 읽을 권한이 없어요."),
            s => {
                eprintln!("[ieum] GitHub 목록을 받지 못했어요: HTTP {s}");
                Fetched::Skip
            }
        }
    }

    /// 실행을 줄에 넣고 상태를 쓴다 — 저장과 같은 잠금 안에서, 확인 시작 때와 같은 워크플로우에만.
    fn github_commit(&self, targets: &[Target], got: &HashMap<RepoKey, Got>, now: DateTime<Utc>) {
        let _one = lock(&self.0.tick_lock);
        let Some(current) = self.github_current() else {
            return;
        };
        let state = self.github_state();
        for t in targets {
            let Some(result) = got.get(&t.key) else {
                continue;
            };
            if !current(t) {
                continue;
            }
            let (items, latest) = match result {
                Got::Error(msg) => {
                    self.github_error(&t.id, msg);
                    continue;
                }
                Got::List(items, latest) => (items, *latest),
            };
            let prev = state.get(&t.id).filter(|e| e.config_hash == t.hash);
            let (runs, seen) = match (prev, t.kind) {
                (None, WatchKind::Issue) => (Vec::new(), baseline_issues(items)),
                (None, WatchKind::Pr) => match latest {
                    Some(n) => (Vec::new(), baseline_prs(items, Some(n))),
                    None => continue,
                },
                (Some(e), WatchKind::Issue) => {
                    let created = &self.0.deps.github_created;
                    let is_own = |n| created.contains(&t.owner, &t.repo, n, now);
                    diff_issues(&e.seen, items, now, &is_own)
                }
                (Some(e), WatchKind::Pr) => {
                    let filter = PrFilter::from_config(&t.config);
                    diff_prs(&e.seen, items, &filter, e.last_checked_at, now)
                }
            };
            // 상태는 실행을 넣은 뒤에 쓴다 — 그 사이 앱이 죽으면 한 번 더 돌 수 있다(놓치기보다 낫다).
            for trigger in runs {
                let source = JobSource::Github {
                    config_hash: t.hash.clone(),
                };
                let job = Job {
                    trigger,
                    source,
                    reply: None,
                };
                // 멈춘 감독은 실행을 받지 않는다 — 상태를 넘기지 않아 다음 감독(재시작·계정 복귀)이 돌린다.
                if matches!(self.push(&t.id, job), Pushed::Stopped) {
                    return;
                }
            }
            if self.is_stopped() {
                return;
            }
            let entry = WatchEntry {
                owner: t.owner.clone(),
                repo: t.repo.clone(),
                config_hash: t.hash.clone(),
                last_checked_at: now,
                seen,
            };
            self.set_github_state(&t.id, entry);
            if lock(&self.0.github_errors).remove(&t.id).is_some() {
                self.emit_status(&t.id, true, String::new());
            }
        }
    }

    fn github_error(&self, id: &str, msg: &str) {
        let mut errors = lock(&self.0.github_errors);
        if errors.get(id).map(String::as_str) != Some(msg) {
            self.emit_status(id, false, msg.to_string());
        }
        errors.insert(id.to_string(), msg.to_string());
    }

    /// 커밋과 같은 확인을 거친다 — 확인 도중 끄거나 지우거나 멈췄으면 오류를 남기지도 알리지도 않는다.
    fn github_fail_all(&self, targets: &[Target], msg: &str) {
        let _one = lock(&self.0.tick_lock);
        let Some(current) = self.github_current() else {
            return;
        };
        for t in targets.iter().filter(|t| current(t)) {
            self.github_error(&t.id, msg);
        }
    }

    /// `tick_lock`을 잡은 채 부른다. 멈췄으면 None, 아니면 "확인 시작 때와 같은 워크플로우인가"(지금 저장본이
    /// 켜져 있고 `config_hash`·세대가 같은가) 판정.
    fn github_current(&self) -> Option<impl Fn(&Target) -> bool> {
        if self.is_stopped() {
            return None;
        }
        let list = match self.store().list_workflows() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[ieum] GitHub 트리거를 확인하지 못했어요: {e}");
                return None;
            }
        };
        let gens = lock(&self.0.github_gen).clone();
        Some(move |t: &Target| {
            let hash = list.iter().find(|w| w.id == t.id).and_then(github_hash);
            hash.as_ref() == Some(&t.hash) && gens.get(&t.id).copied().unwrap_or(0) == t.generation
        })
    }

    /// 저장소 상태 + 쓰지 못한 메모리 상태(앞선다).
    fn github_state(&self) -> HashMap<String, WatchEntry> {
        let mut state = self.store().get_github_watch().unwrap_or_else(|e| {
            eprintln!("[ieum] GitHub 트리거 상태를 읽지 못했어요: {e}");
            HashMap::new()
        });
        state.extend(lock(&self.0.github_unsaved).clone());
        state
    }

    fn set_github_state(&self, id: &str, entry: WatchEntry) {
        let mut unsaved = lock(&self.0.github_unsaved);
        match self.store().set_github_watch(id, &entry) {
            Ok(()) => {
                unsaved.remove(id);
            }
            Err(e) => {
                eprintln!("[ieum] GitHub 트리거 상태를 쓰지 못했어요: {e}");
                unsaved.insert(id.to_string(), entry);
            }
        }
    }

    /// 상태·오류를 지우고 세대를 올리고 대기 중인 GitHub 실행을 버린다 — 다음 확인은 기준점부터.
    /// `tick_lock`을 잡은 채 부른다.
    pub(super) fn github_reset(&self, id: &str) {
        if let Err(e) = self.store().remove_github_watch(id) {
            eprintln!("[ieum] GitHub 트리거 상태를 지우지 못했어요: {e}");
        }
        lock(&self.0.github_unsaved).remove(id);
        lock(&self.0.github_errors).remove(id);
        *lock(&self.0.github_gen).entry(id.to_string()).or_default() += 1;
        if let Some(lane) = lock(&self.0.lanes).get_mut(id) {
            lane.jobs
                .retain(|j| !matches!(j.source, JobSource::Github { .. }));
        }
    }
}
