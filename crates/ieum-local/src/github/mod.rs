//! GitHub 이슈 노드 — REST 호출 기반.

mod api;

pub use api::{GithubApi, Resp};
// 노드 러너(다음 작업)가 쓴다.
#[allow(unused_imports)]
pub(crate) use api::input_error;
