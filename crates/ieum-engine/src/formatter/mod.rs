//! `formatter.*` 실행기 — BE `Formatter`. 포맷터는 런타임이 치환한 설정(`ctx.config`)만 읽는다.

mod date;
mod extract;
mod html;
pub(crate) mod inputs;
mod list;
mod number;
mod text;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::run::{NodeCtx, NodeError, NodeRunner};
use inputs::{In, R, json, summary};

pub(crate) type Op = fn(&In, DateTime<Utc>) -> R<Map<String, Value>>;
/// (event, BE `Formatter.OPS` title, 변환).
pub(crate) type Entry = (&'static str, &'static str, Op);

pub(crate) fn out(v: impl Into<Value>) -> Map<String, Value> {
    Map::from_iter([("output".to_string(), v.into())])
}

fn ops() -> impl Iterator<Item = &'static Entry> {
    [
        text::OPS,
        extract::OPS,
        html::OPS,
        number::OPS,
        date::OPS,
        list::OPS,
    ]
    .into_iter()
    .flatten()
}

/// 등록된 event 전부.
pub fn events() -> Vec<&'static str> {
    ops().map(|e| e.0).collect()
}

/// 실패는 `"<title>: <이유> (입력: '<요약>')"` 메시지.
pub fn apply(
    event: &str,
    config: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<Map<String, Value>, NodeError> {
    let Some((_, title, op)) = ops().find(|e| e.0 == event) else {
        return Err(NodeError {
            message: format!("지원하지 않는 포맷터예요: {event}"),
            code: None,
        });
    };
    op(&In(config), now).map_err(|f| {
        let input = config.get("input").map_or(String::new(), json);
        NodeError {
            message: format!("{title}: {} (입력: '{}')", f.reason, summary(&input)),
            code: Some(match f.field {
                Some(field) => format!("{} {field}", f.code),
                None => f.code.to_string(),
            }),
        }
    })
}

/// 시계를 주입할 수 있다 — `None`이면 시스템 시계.
#[derive(Default)]
pub struct FormatterRunner {
    now: Option<DateTime<Utc>>,
}

impl FormatterRunner {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self { now: Some(now) }
    }
}

#[async_trait]
impl NodeRunner for FormatterRunner {
    async fn run(&self, ctx: NodeCtx<'_>) -> Result<Map<String, Value>, NodeError> {
        apply(
            &ctx.node.event,
            &ctx.config,
            self.now.unwrap_or_else(Utc::now),
        )
    }
}
