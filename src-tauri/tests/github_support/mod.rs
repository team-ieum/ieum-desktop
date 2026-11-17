//! 테스트 `Deps`의 GitHub 러너. 토큰은 늘 실패한다 — 테스트가 진짜 gh·GitHub에 닿지 않게.

use std::sync::Arc;

use async_trait::async_trait;
use ieum_engine::NodeError;
use ieum_local::github::{GithubApi, GithubRunner, GithubToken};

struct NoToken;

#[async_trait]
impl GithubToken for NoToken {
    async fn token(&self) -> Result<String, NodeError> {
        Err(NodeError {
            message: "GitHub 연결이 필요해요. 설정 > 연동에서 연결해 주세요.".into(),
            code: Some("GITHUB_NOT_CONNECTED".into()),
        })
    }
}

pub fn no_github() -> Arc<GithubRunner> {
    Arc::new(GithubRunner::new(GithubApi::new(), Arc::new(NoToken)))
}
