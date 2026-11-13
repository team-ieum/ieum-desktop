//! 시스템 알림 규칙·문구·모아 보내기.

use std::time::Duration;

use ieum_desktop_lib::notify::{
    BATCH_WINDOW, Batch, Batcher, Lang, Notice, Prefs, Route, Target, finish_notice,
    lang_from_apple_languages, route, summarize, undo_failed_notice,
};
use ieum_desktop_lib::store::RunTrigger;
use ieum_desktop_lib::supervisor::{NoticeKind, RunNotice};

const ALL: Prefs = Prefs {
    manual_start: true,
    manual_end: true,
    watch_end: true,
};

fn notice(trigger: RunTrigger, kind: NoticeKind) -> RunNotice {
    RunNotice {
        workflow_id: "wf".into(),
        workflow_name: "영수증 정리".into(),
        run_id: "run1".into(),
        trigger,
        kind,
    }
}

fn finished(ok: bool, failed_node: Option<&str>) -> NoticeKind {
    NoticeKind::Finished {
        ok,
        secs: 12,
        failed_node: failed_node.map(Into::into),
    }
}

fn now(r: Route) -> Notice {
    match r {
        Route::Now(n) => n,
        r => panic!("{r:?}"),
    }
}

#[test]
fn route_manual_start_sent_even_when_focused() {
    let n = now(route(
        &notice(RunTrigger::Manual, NoticeKind::Started),
        &ALL,
        true,
        Lang::Ko,
    ));
    assert_eq!(n.title, "영수증 정리");
    assert_eq!(n.body, "시작했어요");
    assert_eq!(n.target, Target::Run("run1".into()));
    let off = Prefs {
        manual_start: false,
        ..ALL
    };
    let r = route(
        &notice(RunTrigger::Manual, NoticeKind::Started),
        &off,
        false,
        Lang::Ko,
    );
    assert_eq!(r, Route::Drop);
}

#[test]
fn route_manual_end_respects_toggle() {
    let n = notice(RunTrigger::Manual, finished(true, None));
    assert_eq!(now(route(&n, &ALL, true, Lang::Ko)).body, "끝났어요 (12초)");
    let off = Prefs {
        manual_end: false,
        ..ALL
    };
    assert_eq!(route(&n, &off, false, Lang::Ko), Route::Drop);
}

#[test]
fn route_watch_start_dropped() {
    let n = notice(RunTrigger::Watch, NoticeKind::Started);
    assert_eq!(route(&n, &ALL, false, Lang::Ko), Route::Drop);
}

#[test]
fn route_watch_end_batched_unless_focused() {
    let n = notice(RunTrigger::Watch, finished(false, None));
    assert_eq!(route(&n, &ALL, false, Lang::Ko), Route::Batch);
    assert_eq!(route(&n, &ALL, true, Lang::Ko), Route::Drop);
    let off = Prefs {
        watch_end: false,
        ..ALL
    };
    assert_eq!(route(&n, &off, false, Lang::Ko), Route::Drop);
}

#[test]
fn failure_body_uses_node_label_not_error() {
    let n = notice(RunTrigger::Manual, finished(false, Some("파일 옮기기")));
    let body = now(route(&n, &ALL, false, Lang::Ko)).body;
    assert_eq!(body, "실패했어요: 파일 옮기기 단계");
    assert!(!body.contains("/Users"));
    let n = notice(RunTrigger::Manual, finished(false, None));
    assert_eq!(now(route(&n, &ALL, false, Lang::Ko)).body, "실패했어요");
}

#[test]
fn batcher_merges_within_window() {
    let mut b = Batcher::default();
    let first: Vec<bool> = (0..20)
        .map(|i| b.push("wf", "영수증 정리", i != 7))
        .collect();
    assert!(first[0]);
    assert!(first[1..].iter().all(|f| !f));
    let n = flush(&mut b, "wf", Lang::Ko).unwrap();
    assert_eq!(n.title, "영수증 정리");
    assert_eq!(n.body, "19건 완료, 1건 실패");
    assert_eq!(n.target, Target::History);
    assert_eq!(b.take("wf"), None);
    // 다 가져간 뒤의 push는 다시 첫 push.
    assert!(b.push("wf", "영수증 정리", true));
}

#[test]
fn batcher_keeps_workflows_apart() {
    let mut b = Batcher::default();
    assert!(b.push("a", "A", true));
    assert!(b.push("b", "B", true));
    assert!(!b.push("a", "A", true));
    assert!(!b.push("b", "B", false));
    assert!(!b.push("a", "A", true));
    assert_eq!(flush(&mut b, "a", Lang::Ko).unwrap().body, "3건 완료");
    let nb = flush(&mut b, "b", Lang::Ko).unwrap();
    assert_eq!(
        (nb.title.as_str(), nb.body.as_str()),
        ("B", "1건 완료, 1건 실패")
    );
}

#[test]
fn english_copy() {
    let n = |kind| {
        now(route(
            &notice(RunTrigger::Manual, kind),
            &ALL,
            false,
            Lang::En,
        ))
        .body
    };
    assert_eq!(n(NoticeKind::Started), "Started");
    assert_eq!(n(finished(true, None)), "Finished (12s)");
    assert_eq!(n(finished(false, Some("Move file"))), "Failed at Move file");
    assert_eq!(n(finished(false, None)), "Failed");
    let mut b = Batcher::default();
    for _ in 0..5 {
        b.push("wf", "W", true);
    }
    assert_eq!(flush(&mut b, "wf", Lang::En).unwrap().body, "5 done");
    b.push("wf", "W", true);
    b.push("wf", "W", false);
    assert_eq!(
        flush(&mut b, "wf", Lang::En).unwrap().body,
        "1 done, 1 failed"
    );
}

/// 앱이 뒤에 있고 감시 끝 알림이 켜진 타이머 끝.
fn flush(b: &mut Batcher, id: &str, lang: Lang) -> Option<Notice> {
    summarize(b.take(id), false, true, lang)
}

fn batch(done: u32, failed: u32) -> Option<Batch> {
    Some(Batch {
        name: "W".into(),
        done,
        failed,
    })
}

#[test]
fn all_failed_omits_zero_done() {
    let body = |lang| summarize(batch(0, 3), false, true, lang).unwrap().body;
    assert_eq!(body(Lang::Ko), "3건 실패");
    assert_eq!(body(Lang::En), "3 failed");
}

#[test]
fn batch_dropped_when_focused_or_watch_end_off_at_timer_end() {
    assert!(summarize(batch(2, 0), false, true, Lang::Ko).is_some());
    assert_eq!(summarize(batch(2, 0), true, true, Lang::Ko), None);
    assert_eq!(summarize(batch(2, 0), false, false, Lang::Ko), None);
    assert_eq!(summarize(None, false, true, Lang::Ko), None);
}

#[test]
fn batch_window_is_ten_seconds() {
    assert_eq!(BATCH_WINDOW, Duration::from_secs(10));
}

#[test]
fn system_lang_from_apple_languages() {
    assert_eq!(
        lang_from_apple_languages("(\n    \"ko-KR\",\n    \"en-KR\"\n)\n"),
        Lang::Ko
    );
    assert_eq!(
        lang_from_apple_languages("(\n    \"en-US\",\n    \"ko-KR\"\n)\n"),
        Lang::En
    );
    assert_eq!(lang_from_apple_languages("(\n    ko\n)"), Lang::Ko);
    assert_eq!(lang_from_apple_languages("(\n    \"ja-JP\"\n)"), Lang::En);
    // 읽을 항목이 없으면 Ko.
    assert_eq!(lang_from_apple_languages(""), Lang::Ko);
    assert_eq!(lang_from_apple_languages("(\n)"), Lang::Ko);
}

#[test]
fn finish_notice_skipped_only_when_card_is_visible() {
    assert_eq!(finish_notice("w1", "정리", true, true, Lang::Ko), None);
    let n = finish_notice("w1", "정리", false, true, Lang::Ko).unwrap(); // 앱이 앞이어도 다른 화면
    assert_eq!(
        (n.title.as_str(), n.body.as_str()),
        ("정리", "완료 확인을 기다리고 있어요. 눌러서 켜 주세요.")
    );
    assert_eq!(n.target, Target::Workflow("w1".into()));
    assert_eq!(
        n.target.payload(),
        serde_json::json!({"kind": "workflow", "id": "w1"})
    );
    assert!(finish_notice("w1", "정리", true, false, Lang::Ko).is_some()); // 그 화면이지만 창이 뒤·숨김
    assert_eq!(
        finish_notice("w1", "Sort", false, false, Lang::En)
            .unwrap()
            .body,
        "Ready to turn on. Click to confirm."
    );
}

#[test]
fn undo_failed_notice_skipped_only_when_banner_is_visible() {
    assert_eq!(undo_failed_notice("w1", "정리", true, true, Lang::Ko), None);
    let n = undo_failed_notice("w1", "정리", false, true, Lang::Ko).unwrap(); // 앱이 앞이어도 다른 화면
    assert_eq!(
        (n.title.as_str(), n.body.as_str()),
        ("정리", "테스트 후 파일을 되돌리지 못했어요")
    );
    assert_eq!(n.target, Target::Workflow("w1".into())); // 누르면 그 편집 화면
    assert!(!format!("{n:?}").contains('/')); // 파일 경로 없음
    assert!(undo_failed_notice("w1", "정리", true, false, Lang::Ko).is_some()); // 그 화면이지만 창이 뒤·숨김
    assert_eq!(
        undo_failed_notice("w1", "Sort", false, false, Lang::En)
            .unwrap()
            .body,
        "Couldn't put files back after the test"
    );
}
