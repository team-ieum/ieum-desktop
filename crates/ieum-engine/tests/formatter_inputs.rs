use chrono::{DateTime, Utc};
use ieum_engine::formatter::apply;
use serde_json::Map;

fn now() -> DateTime<Utc> {
    "2026-11-07T04:30:00Z".parse().unwrap()
}

#[test]
fn unknown_event_message() {
    let e = apply("formatter.nope", &Map::new(), now()).unwrap_err();
    assert_eq!(e.message, "지원하지 않는 포맷터예요: formatter.nope");
    assert_eq!(e.code, None);
}
