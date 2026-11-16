//! `transform.map` 실행기 — BE `TransformNodeExecutor`의 mappings 갈래.

use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::render::render_deep;
use crate::run::{NodeCtx, NodeError, NodeRunner};

/// 원본 mappings의 1단계 문자열 값만 치환한다(BE `renderVariables`) — 깊은 값은 참조식 그대로 둔다.
pub struct TransformMapRunner;

#[async_trait]
impl NodeRunner for TransformMapRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        let mappings = match ctx.node.config.get("mappings") {
            None | Some(Value::Null) => return Ok(Map::new()),
            Some(Value::Object(m)) => m,
            // BE는 Map 캐스팅 실패로 노드가 실패한다
            Some(_) => {
                return Err(NodeError {
                    message: "mappings는 객체여야 합니다.".into(),
                    code: Some("FIELD_INVALID mappings".into()),
                });
            }
        };
        Ok(mappings
            .iter()
            .map(|(k, v)| {
                let v = match v {
                    Value::String(_) => render_deep(v, ctx.outputs),
                    other => other.clone(),
                };
                (k.clone(), v)
            })
            .collect())
    }
}
