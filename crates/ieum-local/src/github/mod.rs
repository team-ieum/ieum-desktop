//! GitHub 이슈·조회 노드 — REST 호출 기반.

mod api;
mod input;
mod issues;
mod lookup;
mod options;
mod pulls;
mod review;
pub mod watch;

use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::{NodeCtx, NodeError, NodeRunner};
use serde_json::{Map, Value};

pub use api::{GithubApi, Resp};
pub use options::{OptionItem, OptionList, options};

/// 저장소에 쓰는 노드 — 빌더 테스트 전에 사람이 확인한다.
pub const GITHUB_WRITE_EVENTS: [&str; 10] = [
    "github.create_issue",
    "github.update_issue",
    "github.find_or_create_issue",
    "github.add_labels",
    "github.create_comment",
    "github.create_pull_request",
    "github.update_pull_request",
    "github.request_reviewers",
    "github.find_or_create_pull_request",
    "github.submit_pr_review",
];

/// 노드 하나를 실행할 때마다 토큰을 준다. 캐시하지 않는다.
#[async_trait]
pub trait GithubToken: Send + Sync {
    async fn token(&self) -> Result<String, NodeError>;
}

/// 토큰 원문. `Debug`가 값을 가린다.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// 헤더에 넣을 때만 쓴다.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

/// 폴링용 토큰 — 매분 부르므로 캐시할 수 있다.
#[async_trait]
pub trait WatchToken: Send + Sync {
    /// force = 전체 연결 확인을 지금 한다(401 받았을 때).
    async fn token(&self, force: bool) -> Result<Secret, NodeError>;
}

/// GitHub 노드 전부를 event로 나눠 돌린다.
pub struct GithubRunner {
    api: GithubApi,
    token: Arc<dyn GithubToken>,
    created: Arc<watch::CreatedIssues>,
}

impl GithubRunner {
    pub fn new(api: GithubApi, token: Arc<dyn GithubToken>) -> Self {
        GithubRunner {
            api,
            token,
            created: Arc::new(watch::CreatedIssues::in_memory()),
        }
    }

    /// 만든 이슈를 파일로 기억하는 저장소를 쓴다(새 이슈 트리거와 공유).
    pub fn with_created(mut self, created: Arc<watch::CreatedIssues>) -> Self {
        self.created = created;
        self
    }
}

#[async_trait]
impl NodeRunner for GithubRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let (api, token, c) = (&self.api, self.token.as_ref(), &ctx.config);
        let work = async {
            match ctx.node.event.as_str() {
                "github.create_issue" => issues::create_issue(api, token, &self.created, c).await,
                "github.update_issue" => issues::update_issue(api, token, c).await,
                "github.find_or_create_issue" => {
                    issues::find_or_create_issue(api, token, &self.created, c).await
                }
                "github.find_issue" => issues::find_issue(api, token, c).await,
                "github.list_issues" => issues::list_issues(api, token, c).await,
                "github.add_labels" => issues::add_labels(api, token, c).await,
                "github.create_comment" => issues::create_comment(api, token, c).await,
                "github.find_repo" => lookup::find_repo(api, token, c).await,
                "github.find_org" => lookup::find_org(api, token, c).await,
                "github.find_user" => lookup::find_user(api, token, c).await,
                "github.check_org_membership" => lookup::check_org_membership(api, token, c).await,
                "github.find_branch" => lookup::find_branch(api, token, c).await,
                "github.get_file" => lookup::get_file(api, token, c).await,
                "github.create_pull_request" => pulls::create_pull_request(api, token, c).await,
                "github.update_pull_request" => pulls::update_pull_request(api, token, c).await,
                "github.request_reviewers" => pulls::request_reviewers(api, token, c).await,
                "github.find_pull_request" => pulls::find_pull_request(api, token, c).await,
                "github.find_or_create_pull_request" => {
                    pulls::find_or_create_pull_request(api, token, c).await
                }
                "github.get_pr_files" => review::get_pr_files(api, token, c).await,
                "github.get_pr_reviews" => review::get_pr_reviews(api, token, c).await,
                "github.submit_pr_review" => review::submit_pr_review(api, token, c).await,
                other => Err(NodeError {
                    message: format!("{other} 노드는 아직 실행할 수 없어요."),
                    code: None,
                }),
            }
        };
        // 취소되면 토큰 읽기·진행 중인 요청을 끊는다(HttpRunner와 같은 문구).
        tokio::select! {
            _ = ctx.cancel.cancelled() => Err(NodeError {
                message: "취소됐어요.".into(),
                code: None,
            }),
            r = work => r,
        }
    }
}
