//! API 가격표(`pricing.json`, USD / 100만 토큰)와 예상 금액. 표는 공급원 공식 가격 페이지에서 옮겼다.

use std::collections::HashMap;
use std::sync::OnceLock;

use ieum_local::ai::AiUsage;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pricing {
    /// 가격 페이지를 읽은 날(YYYY-MM-DD).
    pub as_of: String,
    pub models: HashMap<String, Price>,
}

/// 앱에 실은 가격표.
pub fn table() -> &'static Pricing {
    static T: OnceLock<Pricing> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../pricing.json")).expect("pricing.json"))
}

/// 예상 금액. 구독 CLI(`cli:*`)·표에 없는 모델·모델 모름은 None.
// ponytail: 구간 요금(Haiku 5.5 10만 토큰 초과 등)·Gemini 캐시 보관료는 빼고 기본 구간 단가만 쓴다.
// 캐시 단가가 표에 없으면 입력 단가로 셈한다.
pub fn estimate_usd(u: &AiUsage, table: &Pricing) -> Option<f64> {
    if u.source.starts_with("cli:") {
        return None;
    }
    let p = table.models.get(u.model.as_deref()?)?;
    let cost = |n: u64, per_m: f64| n as f64 * per_m / 1_000_000.0;
    Some(
        cost(u.input_tokens, p.input)
            + cost(u.output_tokens, p.output)
            + cost(
                u.cache_read_tokens.unwrap_or(0),
                p.cache_read.unwrap_or(p.input),
            )
            + cost(
                u.cache_write_tokens.unwrap_or(0),
                p.cache_write.unwrap_or(p.input),
            ),
    )
}
