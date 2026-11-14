//! 채팅 패널의 앱 쪽 — 대화 루프(`ieum_local::chat`)를 돌리고 대화·사용량을 채팅 파일에 남긴다.

use std::collections::BTreeSet;
use std::sync::Mutex;

use async_trait::async_trait;
use ieum_local::builder::{AiDefault, Builder, ChatMessage, Op, Reply, Role, UsageEntry};
use ieum_local::chat::{Actions, Card, Limits, Stage, Turn, run_turn};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::active::Active;
use crate::builder_host::{PickFolder, builder_of};
use crate::store::now;
use crate::supervisor::Supervisor;

/// 그 워크플로우의 턴이 이미 돌고 있다.
pub const CHAT_BUSY: &str = "CHAT_BUSY";
/// `api:openai`는 기본 모델이 없다.
pub const CHAT_MODEL_REQUIRED: &str = "CHAT_MODEL_REQUIRED";
pub const CHAT_EMPTY: &str = "CHAT_EMPTY";

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

/// 한 턴: 사용자 글 저장 → 대화 루프 → 어시스턴트 글·사용량 저장. 실패한 턴도 쓴 만큼은 남긴다.
pub async fn send(
    sup: &Supervisor,
    builder: &Builder,
    workflow_id: &str,
    text: &str,
    ai: AiDefault,
    limits: Limits,
    on_stage: &(dyn Fn(Stage) + Send + Sync),
) -> Result<ChatTurn, String> {
    if text.trim().is_empty() {
        return Err(CHAT_EMPTY.into());
    }
    let ai = AiDefault {
        provider: ai.provider,
        credential: filled(ai.credential),
        model: filled(ai.model),
    };
    if ai.provider == "api:openai" && ai.model.is_none() {
        return Err(CHAT_MODEL_REQUIRED.into());
    }
    let _running = Running::start(workflow_id)?;
    let store = sup.store();
    // 없는 워크플로우의 채팅 파일을 만들지 않는다.
    store.get_workflow(workflow_id)?;
    let history = store.get_chat(workflow_id)?.messages;
    store.update_chat(workflow_id, |c| {
        c.messages.push(ChatMessage {
            role: Role::User,
            text: text.to_string(),
            at: now(),
        })
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
    })?;
    Ok(ChatTurn {
        say: turn.say,
        cards: turn.cards,
        error: turn.error,
    })
}

/// 채팅 패널의 한 턴. 단계는 `chat-stage {id, stage}`로 흘린다.
// 인자 모양은 화면의 `chat_send` 호출과 같다(State 셋 + 화면이 넘기는 다섯).
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
) -> Result<ChatTurn, String> {
    let builder = builder_of(&active, &pick).await?;
    let sup = active.sup().await?;
    let ai = AiDefault {
        provider,
        credential,
        model,
    };
    let on_stage = |stage: Stage| {
        if let Err(e) = app.emit("chat-stage", json!({"id": workflow_id, "stage": stage})) {
            eprintln!("[ieum] chat-stage를 보내지 못했어요: {e}");
        }
    };
    send(
        &sup,
        &builder,
        &workflow_id,
        &text,
        ai,
        Limits::default(),
        &on_stage,
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
