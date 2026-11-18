//! API 키별 월 한도: 이번 달 지출(대시보드와 같은 `summarize`), AI 호출 전 게이트, 80%·100% 알림 단계.
//! 한도 키는 `<source>:<credential id>`(source는 `api:` 뗀 값). 구독 CLI(`cli:*`)는 다루지 않는다.

use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use chrono::{DateTime, Local};
use ieum_local::ai::{AiUsage, KeyGate};
use serde::Serialize;

use crate::credentials::api_label_in;
use crate::pricing;
use crate::store::{ChatUsage, Store, StoredRun};
use crate::usage::{range_start, summarize};

const UNREADABLE: &str = "이번 달 사용량을 읽지 못해 한도를 확인할 수 없어요.";

/// 한도를 걸 수 있는 키 id — 기본 키(`default`) 또는 저장된 키의 8자 16진 id.
pub fn valid_key_id(id: &str) -> bool {
    id == "default" || (id.len() == 8 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// 공급원 표시 이름.
pub fn provider_name(source: &str) -> &'static str {
    match source {
        "anthropic" => "Claude",
        "openai" => "OpenAI",
        "gemini" => "Gemini",
        _ => "API",
    }
}

/// `$` + 소수 둘째 자리, 끝의 `.00`은 뗀다.
pub fn money(usd: f64) -> String {
    let s = format!("${usd:.2}");
    s.strip_suffix(".00").map(str::to_string).unwrap_or(s)
}

/// 이번 달(로컬 1일 0시 ~ now) 키별 예상 지출. 키는 `<source>:<id>`.
pub fn month_spend(
    runs: &[StoredRun],
    chats: &[ChatUsage],
    now: DateTime<Local>,
) -> HashMap<String, f64> {
    // "month"는 실패하지 않는다.
    let from = range_start("month", now).unwrap_or(now);
    summarize(runs, chats, from, now, pricing::table())
        .by_credential
        .into_iter()
        .filter(|c| !c.source.starts_with("cli:"))
        .map(|c| {
            (
                format!("{}:{}", c.source, c.credential),
                c.usd.unwrap_or(0.0),
            )
        })
        .collect()
}

/// 화면이 읽는 한도 목록과 이번 달 지출. 한도만 있고 쓴 적 없는 키는 `spend`에 없다(화면이 0으로 본다).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyLimitsView {
    pub limits: BTreeMap<String, f64>,
    pub spend: HashMap<String, f64>,
}

pub fn view(store: &Store, now: DateTime<Local>) -> Result<KeyLimitsView, String> {
    let spend = month_spend(&store.all_runs()?, &store.all_chat_usage()?, now);
    Ok(KeyLimitsView {
        limits: store.key_limits().limits,
        spend,
    })
}

/// 알림 단계: 0, 80, 100.
pub fn level(spend: f64, limit: f64) -> u8 {
    if spend >= limit {
        100
    } else if spend >= limit * 0.8 {
        80
    } else {
        0
    }
}

pub fn month_key(now: DateTime<Local>) -> String {
    now.format("%Y-%m").to_string()
}

/// 한도 알림 하나. 화면 문구는 프론트가 만든다.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyLimitNotice {
    pub source: String,
    pub label: String,
    pub level: u8,
    pub spend: f64,
    pub limit: f64,
}

// ponytail: 호출마다 실행 기록 전체를 읽음, 느려지면 이번 달 파일만 읽거나 월 합계 캐시
fn read_spend(store: &Store, now: DateTime<Local>) -> Result<HashMap<String, f64>, String> {
    Ok(month_spend(
        &store.all_runs()?,
        &store.all_chat_usage()?,
        now,
    ))
}

/// AI를 쓴 뒤: 한도 있는 키 중 이번 달 알린 단계보다 높아진 것만 기록하고 돌려준다.
/// `used`는 `(source, id)`, 중복·`cli:*`는 이미 거른 값.
pub fn after_use(
    store: &Store,
    used: &[(String, String)],
    now: DateTime<Local>,
) -> Vec<KeyLimitNotice> {
    let file = store.key_limits();
    let limited: Vec<_> = used
        .iter()
        .filter_map(|(source, id)| {
            let key = format!("{source}:{id}");
            let limit = *file.limits.get(&key)?;
            Some((source, id, key, limit))
        })
        .collect();
    if limited.is_empty() {
        return Vec::new();
    }
    let spends = match read_spend(store, now) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[ieum] 키 한도 알림을 위해 사용량을 읽지 못했어요: {e}");
            return Vec::new();
        }
    };
    let month = month_key(now);
    let mut out = Vec::new();
    for (source, id, key, limit) in limited {
        let spend = spends.get(&key).copied().unwrap_or(0.0);
        let lv = level(spend, limit);
        if lv == 0 {
            continue;
        }
        // 그 키만 건너뛴다 — 이미 기록한 다른 키의 알림은 돌려줘야 영영 빠지지 않는다.
        match store.raise_notified(&key, &month, lv, limit) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                eprintln!("[ieum] 키 한도 알림 기록을 쓰지 못했어요: {e}");
                continue;
            }
        }
        out.push(KeyLimitNotice {
            source: source.clone(),
            label: api_label_in(store.root(), source, id),
            level: lv,
            spend,
            limit,
        });
    }
    out
}

/// 사용량에서 한도 대상 `(source, id)`만: `cli:*` 빼고, id 없으면 `default`, 중복 없이(처음 순서).
pub fn used_keys(usages: impl IntoIterator<Item = AiUsage>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for u in usages {
        if u.source.starts_with("cli:") {
            continue;
        }
        let k = (u.source, u.credential.unwrap_or_else(|| "default".into()));
        if !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// AI 호출 직전 게이트. 한도를 넘은 키는 막는다.
pub struct LimitGate {
    pub store: Store,
    pub now: fn() -> DateTime<Local>,
}

#[async_trait]
impl KeyGate for LimitGate {
    async fn check(&self, source: &str, credential: &str) -> Result<(), String> {
        let key = format!("{source}:{credential}");
        let Some(&limit) = self.store.key_limits().limits.get(&key) else {
            return Ok(());
        };
        // 기록을 못 읽으면 막는 쪽으로 실패한다.
        let spends = read_spend(&self.store, (self.now)()).map_err(|e| {
            eprintln!("[ieum] 키 한도를 확인하려고 사용량을 읽지 못했어요: {e}");
            UNREADABLE.to_string()
        })?;
        if spends.get(&key).copied().unwrap_or(0.0) >= limit {
            return Err(format!(
                "{} {} 키가 이번 달 한도({})를 넘었어요.",
                provider_name(source),
                api_label_in(self.store.root(), source, credential),
                money(limit)
            ));
        }
        Ok(())
    }
}
