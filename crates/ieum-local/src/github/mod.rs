//! GitHub 이슈·조회 노드 — REST 호출 기반.

mod api;
mod input;
mod issues;
mod lookup;
mod pulls;

use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::{NodeCtx, NodeError, NodeRunner};
use serde_json::{Map, Value};

pub use api::{GithubApi, Resp};

/// 저장소에 쓰는 노드 — 빌더 테스트 전에 사람이 확인한다.
pub const GITHUB_WRITE_EVENTS: [&str; 5] = [
    "github.create_issue",
    "github.update_issue",
    "github.find_or_create_issue",
    "github.add_labels",
    "github.create_comment",
];

/// 노드 하나를 실행할 때마다 토큰을 준다. 캐시하지 않는다.
#[async_trait]
pub trait GithubToken: Send + Sync {
    async fn token(&self) -> Result<String, NodeError>;
}

/// GitHub 노드 전부를 event로 나눠 돌린다.
pub struct GithubRunner {
    api: GithubApi,
    token: Arc<dyn GithubToken>,
}

impl GithubRunner {
    pub fn new(api: GithubApi, token: Arc<dyn GithubToken>) -> Self {
        GithubRunner { api, token }
    }
}

#[async_trait]
impl NodeRunner for GithubRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let (api, token, c) = (&self.api, self.token.as_ref(), &ctx.config);
        let work = async {
            match ctx.node.event.as_str() {
                "github.create_issue" => issues::create_issue(api, token, c).await,
                "github.update_issue" => issues::update_issue(api, token, c).await,
                "github.find_or_create_issue" => issues::find_or_create_issue(api, token, c).await,
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
