//! 내 PC 실행 시스템 알림: 규칙(수동/감시·앱 앞 여부·토글), 폴더 감시 알림 모아 보내기, 보내기.
//! 문구엔 워크플로우 이름·노드 label만 — 경로·오류 원문·앱 이름은 넣지 않는다.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::active::Active;
use crate::limit_watch::Suggestion;
use crate::store::{Language, RunTrigger, Settings};
use crate::supervisor::{NoticeKind, RunNotice};

/// 폴더 감시 끝 알림을 워크플로우별로 모으는 시간.
pub const BATCH_WINDOW: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ko,
    En,
}

impl From<Language> for Lang {
    fn from(l: Language) -> Self {
        match l {
            Language::En => Lang::En,
            Language::Ko => Lang::Ko,
            Language::System => system_lang(),
        }
    }
}

/// `defaults read -g AppleLanguages` 출력의 첫 언어가 ko면 Ko, 아니면 En. 읽을 항목이 없으면 Ko.
pub fn lang_from_apple_languages(out: &str) -> Lang {
    let first = out
        .split(|c: char| c == ',' || c == '(' || c == ')' || c.is_whitespace())
        .map(|s| s.trim_matches('"'))
        .find(|s| !s.is_empty());
    match first {
        Some(l) if !l.to_ascii_lowercase().starts_with("ko") => Lang::En,
        _ => Lang::Ko,
    }
}

/// macOS 언어(처음 한 번 읽어 둔다). LANG 환경변수는 GUI 앱에서 믿을 수 없어 보지 않는다. 실패하면 Ko.
fn system_lang() -> Lang {
    static LANG: std::sync::OnceLock<Lang> = std::sync::OnceLock::new();
    *LANG.get_or_init(|| {
        std::process::Command::new("/usr/bin/defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map_or(Lang::Ko, |o| {
                lang_from_apple_languages(&String::from_utf8_lossy(&o.stdout))
            })
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Prefs {
    pub manual_start: bool,
    pub manual_end: bool,
    pub watch_end: bool,
}

impl From<&Settings> for Prefs {
    fn from(s: &Settings) -> Self {
        Prefs {
            manual_start: s.notify_manual_start,
            manual_end: s.notify_manual_end,
            watch_end: s.notify_watch_end,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub target: Target,
}

/// 알림을 누르면 열 화면.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Run(String),
    History,
    /// 다른 계정 권하기(대기 중 제안은 `take_account_suggestion`으로 꺼낸다).
    Suggest,
}

impl Target {
    /// 화면 이벤트 `open-target`의 내용.
    fn payload(&self) -> Value {
        match self {
            Target::Run(id) => json!({"kind": "run", "runId": id}),
            Target::History => json!({"kind": "history"}),
            Target::Suggest => json!({"kind": "suggest"}),
        }
    }
}

/// 한도 도달 → 다른 계정 권하기 알림. 이메일은 넣지 않는다(이름표만).
pub fn suggest_notice(s: &Suggestion, lang: Lang) -> Notice {
    let company = if s.company == "codex" {
        "Codex"
    } else {
        "Claude"
    };
    let (to, n) = (&s.to_label, s.to_used_pct.round() as i64);
    let (title, body) = match lang {
        Lang::Ko => (
            format!("{company} 한도에 닿았어요"),
            format!("{to}({n}% 사용)으로 바꿔서 쓸까요?"),
        ),
        Lang::En => (
            format!("{company} limit reached"),
            format!("Switch to {to} ({n}% used)?"),
        ),
    };
    Notice {
        title,
        body,
        target: Target::Suggest,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Now(Notice),
    Batch,
    Drop,
}

/// 수동 실행은 앱이 앞이어도 보낸다. 감시 실행은 끝만, 앱이 앞이 아닐 때 모아서.
pub fn route(n: &RunNotice, p: &Prefs, focused: bool, lang: Lang) -> Route {
    let ko = lang == Lang::Ko;
    let body = match (n.trigger, &n.kind) {
        (RunTrigger::Manual, NoticeKind::Started) if p.manual_start => {
            if ko { "시작했어요" } else { "Started" }.to_string()
        }
        (RunTrigger::Manual, NoticeKind::Finished { ok: true, secs, .. }) if p.manual_end => {
            if ko {
                format!("끝났어요 ({secs}초)")
            } else {
                format!("Finished ({secs}s)")
            }
        }
        (
            RunTrigger::Manual,
            NoticeKind::Finished {
                ok: false,
                failed_node,
                ..
            },
        ) if p.manual_end => match (failed_node, ko) {
            (Some(node), true) => format!("실패했어요: {node} 단계"),
            (Some(node), false) => format!("Failed at {node}"),
            (None, true) => "실패했어요".into(),
            (None, false) => "Failed".into(),
        },
        (RunTrigger::Watch, NoticeKind::Finished { .. }) if p.watch_end && !focused => {
            return Route::Batch;
        }
        _ => return Route::Drop,
    };
    Route::Now(Notice {
        title: n.workflow_name.clone(),
        body,
        target: Target::Run(n.run_id.clone()),
    })
}

/// 워크플로우별 (이름, 성공 수, 실패 수).
#[derive(Default)]
pub struct Batcher(HashMap<String, (String, u32, u32)>);

impl Batcher {
    /// 그 워크플로우의 첫 push면 true — 호출자가 `BATCH_WINDOW` 타이머를 건다.
    pub fn push(&mut self, workflow_id: &str, workflow_name: &str, ok: bool) -> bool {
        let first = !self.0.contains_key(workflow_id);
        let e = self
            .0
            .entry(workflow_id.to_string())
            .or_insert_with(|| (workflow_name.to_string(), 0, 0));
        if ok {
            e.1 += 1;
        } else {
            e.2 += 1;
        }
        first
    }

    /// 모은 것을 꺼낸다(다음 push는 다시 첫 push).
    pub fn take(&mut self, workflow_id: &str) -> Option<Batch> {
        let (name, done, failed) = self.0.remove(workflow_id)?;
        Some(Batch { name, done, failed })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub name: String,
    pub done: u32,
    pub failed: u32,
}

/// 모아 보내기 타이머 끝: 그사이 앱을 앞으로 가져왔거나 감시 끝 알림을 껐으면 버린다.
pub fn summarize(
    batch: Option<Batch>,
    focused: bool,
    watch_end: bool,
    lang: Lang,
) -> Option<Notice> {
    let Batch { name, done, failed } = batch?;
    if focused || !watch_end {
        return None;
    }
    let body = match (lang, done, failed) {
        (Lang::Ko, _, 0) => format!("{done}건 완료"),
        (Lang::Ko, 0, _) => format!("{failed}건 실패"),
        (Lang::Ko, _, _) => format!("{done}건 완료, {failed}건 실패"),
        (Lang::En, _, 0) => format!("{done} done"),
        (Lang::En, 0, _) => format!("{failed} failed"),
        (Lang::En, _, _) => format!("{done} done, {failed} failed"),
    };
    Some(Notice {
        title: name,
        body,
        target: Target::History,
    })
}

/// 앱 상태로 둔다.
pub type Pending = Mutex<Batcher>;

pub(crate) fn settings(app: &AppHandle) -> Settings {
    app.try_state::<Active>()
        .and_then(|a| a.settings().get_settings().ok())
        .unwrap_or_default()
}

pub(crate) fn focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// supervisor의 `RunNotice`를 규칙대로 보내거나 모은다.
pub fn on_run_notice(app: &AppHandle, n: &RunNotice) {
    let s = settings(app);
    // 앞 여부(메인 루프를 기다림)는 감시 끝에만 쓴다 — 수동 알림은 기다리지 않게.
    let watch_end = matches!(
        (n.trigger, &n.kind),
        (RunTrigger::Watch, NoticeKind::Finished { .. })
    );
    let front = watch_end && focused(app);
    match route(n, &Prefs::from(&s), front, s.language.into()) {
        Route::Now(notice) => deliver(app, &notice),
        Route::Batch => {
            let Some(pending) = app.try_state::<Pending>() else {
                return;
            };
            let ok = matches!(n.kind, NoticeKind::Finished { ok: true, .. });
            let first = pending.lock().unwrap_or_else(|e| e.into_inner()).push(
                &n.workflow_id,
                &n.workflow_name,
                ok,
            );
            if first {
                let (app, id) = (app.clone(), n.workflow_id.clone());
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(BATCH_WINDOW).await;
                    let batch = app
                        .state::<Pending>()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take(&id);
                    let s = settings(&app);
                    let notice =
                        summarize(batch, focused(&app), s.notify_watch_end, s.language.into());
                    if let Some(notice) = notice {
                        deliver(&app, &notice);
                    }
                });
            }
        }
        Route::Drop => {}
    }
}

fn show(app: &AppHandle, title: &str, body: &str, target: Option<&Target>) -> Result<(), String> {
    let mut b = app.notification().builder().title(title);
    if !body.is_empty() {
        b = b.body(body);
    }
    if let Some(t) = target {
        b = b.extra("target", t.payload());
    }
    b.show().map_err(|e| e.to_string())
}

pub fn deliver(app: &AppHandle, n: &Notice) {
    if let Err(e) = show(app, &n.title, &n.body, Some(&n.target)) {
        eprintln!("[ieum] 알림을 보내지 못했어요: {e}");
    }
}

/// 앞 여부·토글과 무관하게 테스트 알림.
pub fn send_test(app: &AppHandle) -> Result<(), String> {
    let title = match settings(app).language.into() {
        Lang::Ko => "알림이 켜졌어요",
        Lang::En => "Notifications are on",
    };
    show(app, title, "", None)
}

/// 알림 클릭 → 창을 띄우고, 열 화면이 있으면 `open-target`. 알림을 보내기 전에 한 번 건다
/// (플러그인은 처리기가 있을 때만 데스크톱 클릭을 넘겨준다).
pub fn listen_clicks(app: &AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.notification()
        .on_action(move |a| {
            if a.action_id() != "tap" {
                return;
            }
            crate::tray::show_main(&handle);
            if let Some(t) = a.notification().and_then(|n| n.extra().get("target")) {
                let _ = handle.emit("open-target", t);
            }
        })
        .map_err(|e| e.to_string())
}
