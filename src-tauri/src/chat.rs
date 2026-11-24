//! 채팅 패널의 앱 쪽 — 대화 루프(`ieum_local::chat`)를 돌리고 대화·사용량을 채팅 파일에 남긴다.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use ieum_local::builder::{
    AiDefault, Blueprint, BlueprintStatus, Builder, ChatFile, ChatMessage, Op, Reply, Role,
    UsageEntry, prompt_summary, validate_stored,
};
use ieum_local::chat::{Actions, BlueprintTurn, Card, Limits, Stage, Turn, run_turn};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};

use crate::active::Active;
use crate::builder_host::{PickFolder, builder_of};
use crate::key_limits::{after_use, used_keys};
use crate::store::now;
use crate::supervisor::{AppEvent, Supervisor};

/// 그 워크플로우의 턴이 이미 돌고 있다.
pub const CHAT_BUSY: &str = "CHAT_BUSY";
/// `api:openai`는 기본 모델이 없다.
pub const CHAT_MODEL_REQUIRED: &str = "CHAT_MODEL_REQUIRED";
pub const CHAT_EMPTY: &str = "CHAT_EMPTY";
/// 화면이 보낸 설계서 데이터 모델이 검사(슬롯 경로 섞임·크기)를 못 넘었다. 규칙 이름은 싣지 않는다.
pub const CHAT_BAD_DATA: &str = "CHAT_BAD_DATA";

/// 실험 설정에서 온 턴 옵션. 기본값 = 설계서 없는 지금까지의 채팅.
#[derive(Default)]
pub struct ChatOpts {
    /// 설계서 모드 — 지시문에 카탈로그를, 프롬프트에 "## 설계서" 구역을 싣는다.
    pub blueprint: bool,
    /// 화면 카드의 현재 데이터 모델. 떠 있는(Active) 설계서에 턴 전에 적는다.
    pub data: Option<Value>,
    /// 이 글을 보내기 전에 카드를 고쳤는가(실측용).
    pub touched: bool,
    /// 실측 기록 파일. None이면 쓰지 않는다.
    pub metrics: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
    pub say: String,
    pub cards: Vec<Card>,
    pub error: Option<String>,
}

/// 턴이 도는 워크플로우 id. 한 워크플로우에 턴은 한 번에 하나.
static RUNNING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// 빠져나갈 때(오류·panic 포함) 표시를 지운다.
struct Running(String);

impl Running {
    fn start(workflow_id: &str) -> Result<Running, String> {
        let mut set = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        if !set.insert(workflow_id.to_string()) {
            return Err(CHAT_BUSY.into());
        }
        Ok(Running(workflow_id.to_string()))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// 루프의 동작을 Builder로 잇는다. AI 노드는 이 채팅의 키·모델로 채워진다.
struct BuilderActions<'a> {
    builder: &'a Builder,
    workflow_id: &'a str,
    ai: &'a AiDefault,
}

#[async_trait]
impl Actions for BuilderActions<'_> {
    async fn apply(&self, op: Op) -> Reply {
        self.builder
            .apply(self.workflow_id, op, Some(self.ai.clone()))
            .await
    }
}

fn filled(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

/// 만들기를 기다리는 설계서 — 제출됐고 아직 오류 없이 끝난 만들기 턴이 없다.
fn building(b: &mut Option<Blueprint>) -> Option<&mut Blueprint> {
    b.as_mut()
        .filter(|b| b.status == BlueprintStatus::Submitted && b.build_pending)
}

/// 설계서 없는 한 턴(`send_with`의 기본 옵션).
pub async fn send(
    sup: &Supervisor,
    builder: &Builder,
    workflow_id: &str,
    text: &str,
    ai: AiDefault,
    limits: Limits,
    on_stage: &(dyn Fn(Stage) + Send + Sync),
) -> Result<ChatTurn, String> {
    send_with(
        sup,
        builder,
        workflow_id,
        text,
        ai,
        limits,
        on_stage,
        ChatOpts::default(),
    )
    .await
}

/// 한 턴: 사용자 글 저장 → 대화 루프 → 어시스턴트 글·사용량 저장. 실패한 턴도 쓴 만큼은 남긴다.
#[allow(clippy::too_many_arguments)]
pub async fn send_with(
    sup: &Supervisor,
    builder: &Builder,
    workflow_id: &str,
    text: &str,
    ai: AiDefault,
    limits: Limits,
    on_stage: &(dyn Fn(Stage) + Send + Sync),
    opts: ChatOpts,
) -> Result<ChatTurn, String> {
    if text.trim().is_empty() {
        return Err(CHAT_EMPTY.into());
    }
    let ai = AiDefault {
        provider: ai.provider,
        credential: filled(ai.credential),
        model: filled(ai.model),
        effort: filled(ai.effort),
    };
    if ai.provider == "api:openai" && ai.model.is_none() {
        return Err(CHAT_MODEL_REQUIRED.into());
    }
    let _running = Running::start(workflow_id)?;
    let store = sup.store();
    // 없는 워크플로우의 채팅 파일을 만들지 않는다.
    store.get_workflow(workflow_id)?;
    // 카드에서 고친 값을 AI가 보게 하는 유일한 길. 거절이면 채팅 파일을 건드리기 전에 멈춘다.
    if opts.blueprint
        && let Some(data) = opts.data
        && !builder.sync_blueprint_data(workflow_id, data).await.ok
    {
        return Err(CHAT_BAD_DATA.into());
    }
    let chat = store.get_chat(workflow_id)?;
    // 저장 파일은 믿지 않는다 — 검증을 못 넘은 설계서는 프롬프트에 싣지 않는다.
    let summary = chat
        .state
        .blueprint
        .filter(|b| opts.blueprint && validate_stored(b))
        .map(|b| prompt_summary(&b));
    let history = chat.messages;
    store.update_chat(workflow_id, |c| {
        c.messages.push(ChatMessage {
            role: Role::User,
            text: text.to_string(),
            at: now(),
        });
        // 턴 전에 센다 — 턴 도중 앱이 꺼져도 시도 횟수는 남는다.
        if let Some(b) = building(&mut c.state.blueprint) {
            b.build_attempts += 1;
        }
    })?;
    let turn = run_turn(
        sup.ai(),
        &BuilderActions {
            builder,
            workflow_id,
            ai: &ai,
        },
        Turn {
            text,
            history: &history,
            ai: &ai,
            now: &chrono::Local::now().to_rfc3339(),
            blueprint: opts.blueprint.then_some(BlueprintTurn {
                summary: summary.as_deref(),
            }),
        },
        limits,
        on_stage,
    )
    .await;
    store.update_chat(workflow_id, |c| {
        if !turn.say.is_empty() {
            c.messages.push(ChatMessage {
                role: Role::Assistant,
                text: turn.say.clone(),
                at: now(),
            });
        }
        c.usage.extend(turn.usage.iter().map(|u| UsageEntry {
            at: now(),
            usage: u.clone(),
        }));
        // 오류·잘림으로 끝난 만들기 턴은 [이어서 만들기]가 남도록 그대로 둔다.
        if turn.error.is_none()
            && !turn.cut_short
            && let Some(b) = building(&mut c.state.blueprint)
        {
            b.build_pending = false;
        }
    })?;
    // 사용량을 채팅 파일에 쓴 뒤 — 이번 턴 지출이 이번 달 합계에 들어가야 한다.
    let used = used_keys(turn.usage.iter().cloned());
    for n in after_use(store, &used, chrono::Local::now()) {
        sup.emit(AppEvent::KeyLimit(n));
    }
    Ok(ChatTurn {
        say: turn.say,
        cards: turn.cards,
        error: turn.error,
    })
}

/// 설계서 카드의 [이대로 만들기]. 도는 턴은 제출 전 설계서를 보고 있으므로 그동안은 막는다.
pub async fn submit(
    builder: &Builder,
    workflow_id: &str,
    version: u32,
    data: Value,
) -> Result<Reply, String> {
    let _running = Running::start(workflow_id)?;
    Ok(builder.submit_blueprint(workflow_id, version, data).await)
}

/// 화면에 줄 채팅 파일. 검증을 못 넘은 저장본 설계서는 없는 것으로 본다.
pub fn visible_chat(mut f: ChatFile) -> ChatFile {
    f.state.blueprint = f.state.blueprint.filter(validate_stored);
    f
}

/// 채팅 패널의 한 턴. 단계는 `chat-stage {id, stage}`로 흘린다. 실험 토글은 기기 설정에서 읽는다.
// 인자 모양은 화면의 `chat_send` 호출과 같다(State 셋 + 화면이 넘기는 여덟).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    active: State<'_, Active>,
    pick: State<'_, PickFolder>,
    workflow_id: String,
    text: String,
    provider: String,
    credential: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    blueprint_data: Option<Value>,
    blueprint_touched: Option<bool>,
) -> Result<ChatTurn, String> {
    let builder = builder_of(&active, &pick).await?;
    let sup = active.sup().await?;
    let settings = active.settings().get_settings()?;
    let opts = ChatOpts {
        blueprint: settings.chat_blueprint,
        data: blueprint_data,
        touched: blueprint_touched.unwrap_or(false),
        metrics: settings
            .chat_metrics
            .then(|| active.app_data().join("metrics/chat.jsonl")),
    };
    let ai = AiDefault {
        provider,
        credential,
        model,
        effort,
    };
    let on_stage = |stage: Stage| {
        if let Err(e) = app.emit("chat-stage", json!({"id": workflow_id, "stage": stage})) {
            eprintln!("[ieum] chat-stage를 보내지 못했어요: {e}");
        }
    };
    send_with(
        &sup,
        &builder,
        &workflow_id,
        &text,
        ai,
        Limits::default(),
        &on_stage,
        opts,
    )
    .await
}

/// 폴더 고르기 카드의 버튼 — 사람이 눌러야만 시스템 폴더 창이 열린다.
#[tauri::command]
pub async fn chat_pick_folder(
    active: State<'_, Active>,
    pick: State<'_, PickFolder>,
    workflow_id: String,
    node_id: String,
    field: String,
) -> Result<Reply, String> {
    let op = Op::PickFolder { id: node_id, field };
    Ok(builder_of(&active, &pick)
        .await?
        .apply(&workflow_id, op, None)
        .await)
}

/// 설계서 카드의 [이대로 만들기].
#[tauri::command]
pub async fn chat_submit_blueprint(
    active: State<'_, Active>,
    pick: State<'_, PickFolder>,
    workflow_id: String,
    version: u32,
    data: Value,
) -> Result<Reply, String> {
    let builder = builder_of(&active, &pick).await?;
    submit(&builder, &workflow_id, version, data).await
}

/// 설계서 카드의 앱 전용 폴더 칸 — 사람이 눌러야만 폴더 창이 열린다. 성공 data의 경로는 화면 전용.
#[tauri::command]
pub async fn chat_pick_slot(
    active: State<'_, Active>,
    pick: State<'_, PickFolder>,
    workflow_id: String,
    slot: String,
    version: u32,
) -> Result<Reply, String> {
    Ok(builder_of(&active, &pick)
        .await?
        .pick_slot(&workflow_id, &slot, version)
        .await)
}
