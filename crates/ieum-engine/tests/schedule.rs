use chrono::{DateTime, Utc};
use ieum_engine::schedule::{
    Decision, Schedule, decide, schedule_cron, schedule_output, to_quartz, triggered_at,
};
use serde_json::{Map, Value, json};

fn cfg(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn utc(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn sched(quartz: &str) -> Schedule {
    Schedule::parse(quartz).unwrap()
}

#[test]
fn fields_to_cron_matches_be() {
    let cases = [
        ("schedule.hourly", json!({}), "0 0 * * * ?"),
        (
            "schedule.hourly",
            json!({"weekends": false}),
            "0 0 * ? * MON-FRI",
        ),
        (
            "schedule.hourly",
            json!({"weekends": "false"}),
            "0 0 * ? * MON-FRI",
        ),
        ("schedule.hourly", json!({"weekends": true}), "0 0 * * * ?"),
        ("schedule.daily", json!({"hour": "9"}), "0 0 9 * * ?"),
        ("schedule.daily", json!({"hour": 9}), "0 0 9 * * ?"),
        (
            "schedule.daily",
            json!({"hour": "9", "weekends": false}),
            "0 0 9 ? * MON-FRI",
        ),
        (
            "schedule.weekly",
            json!({"weekday": "MON", "hour": "9"}),
            "0 0 9 ? * MON",
        ),
        (
            "schedule.monthly",
            json!({"day": "L", "hour": "23"}),
            "0 0 23 L * ?",
        ),
        ("schedule.every", json!({"cron": "0 9 * * 5"}), "0 9 * * 5"),
    ];
    for (event, config, want) in cases {
        assert_eq!(
            schedule_cron(event, &cfg(config.clone())).as_deref(),
            Ok(want),
            "{event} {config}"
        );
    }
}

#[test]
fn fields_out_of_range_err() {
    let cases = [
        ("schedule.daily", json!({"hour": "24"})),
        ("schedule.daily", json!({})),
        ("schedule.weekly", json!({"weekday": "MONDAY", "hour": "9"})),
        ("schedule.monthly", json!({"day": "29", "hour": "9"})),
        ("schedule.every", json!({"cron": ""})),
        ("schedule.every", json!({"cron": 5})),
        ("schedule.every", json!({})),
    ];
    for (event, config) in cases {
        assert!(
            schedule_cron(event, &cfg(config.clone())).is_err(),
            "{event} {config}"
        );
    }
}

#[test]
fn to_quartz_matches_be() {
    assert_eq!(to_quartz("0 9 * * *").as_deref(), Ok("0 0 9 * * ?"));
    assert_eq!(to_quartz("*/5 * * * *").as_deref(), Ok("0 */5 * * * ?"));
    assert_eq!(to_quartz("0 9 * * 1-5").as_deref(), Ok("0 0 9 ? * 2-6"));
    assert_eq!(to_quartz("0 9 * * 0,7").as_deref(), Ok("0 0 9 ? * 1,1"));
    assert_eq!(to_quartz("0 9 * * 0-7").as_deref(), Ok("0 0 9 ? * *"));
    assert_eq!(to_quartz("0 9 * * */2").as_deref(), Ok("0 0 9 ? * */2"));
    assert_eq!(
        to_quartz("0 9 * * MON-FRI").as_deref(),
        Ok("0 0 9 ? * MON-FRI")
    );
    assert_eq!(
        to_quartz("0 0 9 ? * MON-FRI").as_deref(),
        Ok("0 0 9 ? * MON-FRI")
    );
    assert!(to_quartz("0 9 1 * 1").is_err());
    assert!(to_quartz("0 9 * * 8").is_err());
    assert!(to_quartz("0 9 * * MON/2").is_err());
    assert!(to_quartz("9 * * *").is_err());
    assert!(to_quartz("").is_err());
    assert!(to_quartz("0 0 99 * * ?").is_err());
}

#[test]
fn every_unix_weekday_friday() {
    let s = sched(&to_quartz("0 9 * * 5").unwrap());
    // 2026-11-16 월 09:00 KST → 금 09:00 KST
    assert_eq!(
        s.next_after(utc("2026-11-16T00:00:00Z")),
        Some(utc("2026-11-20T00:00:00Z"))
    );
}

#[test]
fn daily_is_kst_not_local() {
    // 고정 UTC 인스턴트만 넣고 Asia/Seoul로 계산하므로 프로세스 TZ와 무관하다(로컬 시간대를 읽지 않음).
    let s = sched(&schedule_cron("schedule.daily", &cfg(json!({"hour": "9"}))).unwrap());
    assert_eq!(
        s.next_after(utc("2026-11-16T01:00:00Z")),
        Some(utc("2026-11-17T00:00:00Z"))
    );
}

#[test]
fn quartz_features() {
    let from = utc("2026-11-01T00:00:00Z");
    assert_eq!(
        sched("0 0 9 L * ?").next_after(from),
        Some(utc("2026-11-30T00:00:00Z"))
    );
    // 15일(일)에 가장 가까운 평일 → 16일(월)
    assert_eq!(
        sched("0 0 9 15W * ?").next_after(from),
        Some(utc("2026-11-16T00:00:00Z"))
    );
    // 셋째 금요일(Quartz 6=금)
    assert_eq!(
        sched("0 0 9 ? * 6#3").next_after(from),
        Some(utc("2026-11-20T00:00:00Z"))
    );
    // 마지막 금요일
    assert_eq!(
        sched("0 0 9 ? * 6L").next_after(from),
        Some(utc("2026-11-27T00:00:00Z"))
    );
    // Quartz 숫자 요일 1=일
    assert_eq!(
        sched("0 0 9 ? * 1").next_after(from),
        Some(utc("2026-11-08T00:00:00Z"))
    );
    assert_eq!(
        sched("0 0 9 * * ? 2027").next_after(from),
        Some(utc("2027-01-01T00:00:00Z"))
    );
    // Quartz는 일·요일 중 정확히 하나가 '?'
    assert!(Schedule::parse("0 0 9 * * MON").is_err());
    assert!(Schedule::parse("0 0 9 ? * ?").is_err());
    assert!(Schedule::parse("0 9 * * *").is_err());
}

#[test]
fn decide_cases() {
    let hourly = sched("0 0 * * * ?");
    let last = utc("2026-11-16T00:00:00Z"); // 09:00 KST
    assert_eq!(
        decide(&hourly, last, utc("2026-11-16T00:30:00Z")),
        Decision::Nothing
    );
    // 10·11·12시 놓침 → 마지막 한 번
    assert_eq!(
        decide(&hourly, last, utc("2026-11-16T03:10:00Z")),
        Decision::Run {
            slot: utc("2026-11-16T03:00:00Z")
        }
    );
    assert_eq!(
        decide(&hourly, last, utc("2026-11-16T05:30:00Z")),
        Decision::Run {
            slot: utc("2026-11-16T05:00:00Z")
        }
    );
    // 차례 시각과 같은 now도 그 차례에 포함
    assert_eq!(
        decide(&hourly, last, utc("2026-11-16T01:00:00Z")),
        Decision::Run {
            slot: utc("2026-11-16T01:00:00Z")
        }
    );

    let daily = sched("0 0 9 * * ?");
    let yesterday = utc("2026-11-15T00:00:00Z");
    assert_eq!(
        decide(&daily, yesterday, utc("2026-11-16T02:01:00Z")),
        Decision::Expired {
            slot: utc("2026-11-16T00:00:00Z")
        }
    );
    assert_eq!(
        decide(&daily, yesterday, utc("2026-11-16T02:00:00Z")),
        Decision::Run {
            slot: utc("2026-11-16T00:00:00Z")
        }
    );
    assert_eq!(
        decide(&daily, last, utc("2026-11-15T23:00:00Z")),
        Decision::ClockBack
    );
}

#[test]
fn latest_at_or_before_window() {
    let hourly = sched("0 0 * * * ?");
    let t = utc("2026-11-16T00:00:00Z");
    assert_eq!(hourly.latest_at_or_before(t, t), None);
    assert_eq!(
        hourly.latest_at_or_before(t, utc("2026-11-16T02:59:59Z")),
        Some(utc("2026-11-16T02:00:00Z"))
    );
}

#[test]
fn output_shape() {
    let now = utc("2026-11-17T00:00:00Z");
    assert_eq!(triggered_at(now), "2026-11-17T09:00:00");
    assert_eq!(
        Value::Object(schedule_output(
            "schedule.daily",
            &cfg(json!({"hour": "9"})),
            now
        )),
        json!({"triggeredAt": "2026-11-17T09:00:00"})
    );
    assert_eq!(
        Value::Object(schedule_output(
            "schedule.every",
            &cfg(json!({"cron": "0 9 * * 5"})),
            now
        )),
        json!({"triggeredAt": "2026-11-17T09:00:00", "cron": "0 9 * * 5"})
    );
}

/// `from`에서 이어지는 다음 차례 n개.
fn nexts(s: &Schedule, from: &str, n: usize) -> Vec<DateTime<Utc>> {
    let mut t = utc(from);
    (0..n)
        .map(|_| {
            t = s.next_after(t).unwrap();
            t
        })
        .collect()
}

fn utcs(list: &[&str]) -> Vec<DateTime<Utc>> {
    list.iter().map(|s| utc(s)).collect()
}

// 아래 기대값은 Quartz 2.5.2 CronExpression과 대조한 리뷰 결과.

#[test]
fn quartz_start_step() {
    // Unix '0/15' = 0분부터 15분마다
    let s = sched(&to_quartz("0/15 * * * *").unwrap());
    assert_eq!(
        nexts(&s, "2026-11-16T00:07:00Z", 2),
        utcs(&["2026-11-16T00:15:00Z", "2026-11-16T00:30:00Z"])
    );
    // 토 09:00 KST → 월 09:00·09:15
    let s = sched("0 0/15 9-17 ? * MON-FRI");
    assert_eq!(
        nexts(&s, "2026-11-14T00:00:00Z", 2),
        utcs(&["2026-11-16T00:00:00Z", "2026-11-16T00:15:00Z"])
    );
    // 1일부터 5일마다 12시: 16일 13시 → 21일 12시
    let s = sched("0 0 12 1/5 * ?");
    assert_eq!(
        nexts(&s, "2026-11-16T04:00:00Z", 1),
        utcs(&["2026-11-21T03:00:00Z"])
    );
}

#[test]
fn weekday_range_wraps() {
    let weekend = utcs(&[
        "2026-11-21T01:00:00Z",
        "2026-11-22T01:00:00Z",
        "2026-11-28T01:00:00Z",
    ]);
    let s = sched(&to_quartz("0 10 * * 6-7").unwrap());
    assert_eq!(nexts(&s, "2026-11-16T00:00:00Z", 3), weekend);
    let s = sched("0 0 10 ? * SAT-SUN");
    assert_eq!(nexts(&s, "2026-11-16T00:00:00Z", 3), weekend);
    let daily = utcs(&[
        "2026-11-17T00:00:00Z",
        "2026-11-18T00:00:00Z",
        "2026-11-19T00:00:00Z",
        "2026-11-20T00:00:00Z",
        "2026-11-21T00:00:00Z",
        "2026-11-22T00:00:00Z",
        "2026-11-23T00:00:00Z",
    ]);
    let s = sched(&to_quartz("0 9 * * 1-7").unwrap());
    assert_eq!(nexts(&s, "2026-11-16T00:00:00Z", 7), daily);
    let s = sched("0 0 9 ? * MON-SUN");
    assert_eq!(nexts(&s, "2026-11-16T00:00:00Z", 7), daily);
    // 금·토·일·월
    let s = sched("0 0 9 ? * FRI-MON");
    assert_eq!(
        nexts(&s, "2026-11-17T00:00:00Z", 4),
        utcs(&[
            "2026-11-20T00:00:00Z",
            "2026-11-21T00:00:00Z",
            "2026-11-22T00:00:00Z",
            "2026-11-23T00:00:00Z",
        ])
    );
}

#[test]
fn weekday_l_alone_is_saturday() {
    let s = sched("0 0 9 ? * L");
    assert_eq!(
        nexts(&s, "2026-11-16T00:00:00Z", 2),
        utcs(&["2026-11-21T00:00:00Z", "2026-11-28T00:00:00Z"])
    );
}

#[test]
fn unsupported_quartz_rejected() {
    for q in ["0 0 9 ? * 2#1,6#3", "0 0 9 ? * 6L,2", "0 0 9 L-3 * ?"] {
        assert!(Schedule::parse(q).is_err(), "{q}");
    }
    let e = Schedule::parse("0 0 9 L-3 * ?").unwrap_err();
    assert!(e.0.contains("지원하지 않는 cron 형식"), "{e:?}");
}

#[test]
fn unix_weekday_single_digit_only() {
    assert!(to_quartz("0 9 * * 07").is_err());
    assert!(to_quartz("0 9 * * 00").is_err());
}

/// 월·요일 이름 뒤 간격: Quartz는 간격을 버리고 croner는 적용해 다른 시각에 돈다 → 거절. 숫자 간격은 같다.
#[test]
fn name_with_step_rejected() {
    for q in [
        "0 0 9 ? * MON-FRI/2",
        "0 0 9 ? * MON/2",
        "0 0 9 ? * SUN/3",
        "0 0 9 1 JAN/3 ?",
        "0 0 9 1 JAN-DEC/3 ?",
    ] {
        assert!(Schedule::parse(q).is_err(), "{q}");
    }
    for q in ["0 0 9 ? * 2/2", "0 0 9 1 1/3 ?"] {
        assert!(Schedule::parse(q).is_ok(), "{q}");
    }
}
