//! 채팅 패널 대화 루프. Tauri·Store를 모른다 — 저장과 이벤트 송신은 앱(`src-tauri/src/chat.rs`)이 한다.
//! Builder는 `Actions` 이음매 하나로만 부른다.

mod prompt;

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};

use crate::ai::{
    AiError, AiRequest, AiRunner, AiUsage, OutputField, build_output_schema,
    parse_structured_output,
};
use crate::builder::{AiDefault, ChatMessage, Code, Op, Reply, UI_RULES};

pub use prompt::{BLUEPRINT_INSTRUCTION, INSTRUCTION};

/// 루프가 Builder를 부르는 유일한 길. 운영 구현은 `Builder::apply`를 감싼 것 하나.
#[async_trait]
pub trait Actions: Send + Sync {
    async fn apply(&self, op: Op) -> Reply;
}

/// 한 턴의 상한.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// AI 호출 수.
    pub calls: usize,
    pub time: Duration,
    /// 노드 테스트 수.
    pub tests: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            calls: 20,
            time: Duration::from_secs(540),
            tests: 3,
        }
    }
}

/// 응답 대기 중 화면에 보이는 단계.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Designing,
    Testing,
}

/// 턴 응답에만 실리는 카드(저장하지 않는다). 테스트 값은 싣지 않는다 — 화면이 `runId`로 실행 기록을 읽는다.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Card {
    Test {
        node_id: String,
        run_id: String,
        status: String,
        /// HTTP 요청을 실제로 보냈다(사람이 [보내기]).
        http_sent: bool,
    },
    PickFolder {
        node_id: String,
        field: String,
    },
    Finish {},
    /// `ui` 성공 — 화면은 `getChat`의 설계서 스냅샷을 그린다. 내용은 싣지 않는다.
    Blueprint {
        version: u32,
    },
}

/// 설계서 모드(실험 토글 켜짐). `summary` = 프롬프트 "## 설계서" 구역(`prompt_summary`), 설계서가 없으면 None.
pub struct BlueprintTurn<'a> {
    pub summary: Option<&'a str>,
}

pub struct Turn<'a> {
    pub text: &'a str,
    pub history: &'a [ChatMessage],
    pub ai: &'a AiDefault,
    pub now: &'a str,
    /// None = 설계서 모드 꺼짐 — 지시문에 카탈로그를 싣지 않고 `ui`·`bind-folder`를 거절한다.
    pub blueprint: Option<BlueprintTurn<'a>>,
}

/// AI 호출 하나의 결과 갈래(실측용).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallOutcome {
    Ok,
    /// 바깥 2칸(say·actions) 모양 실패.
    Outer,
    /// `actions` 글자가 JSON 배열이 아님.
    ActionsJson,
    /// 원문 — 메모리 안 분류용. CLI stderr 꼬리·연결 오류 원문이 섞여 있어 어디에도 쓰지 않는다.
    AiError(String),
    /// 턴 시간 상한에 걸려 끊음.
    Timeout,
}

#[derive(Debug, Clone)]
pub struct CallStat {
    pub ms: u64,
    pub outcome: CallOutcome,
    pub usage: Option<AiUsage>,
}

/// 실행한 동작 하나 — 코드·개수만. 값·경로·문구는 싣지 않는다.
#[derive(Debug, Clone, PartialEq)]
pub struct OpStat {
    /// kebab 이름(`set`·`bind-folder`…).
    pub op: String,
    pub ok: bool,
    pub code: Option<Code>,
    /// `UI_RULES` 안의 것만.
    pub rule: Option<&'static str>,
    /// 응답 `data.errors` 길이(`set`의 검증 에러 수).
    pub errors: usize,
    /// `ui`만: createSurface가 있으면 "create", 아니면 "update".
    pub ui: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub say: String,
    pub cards: Vec<Card>,
    /// 호출마다 한 건(실패한 턴도 쓴 만큼).
    pub usage: Vec<AiUsage>,
    pub error: Option<String>,
    /// 시간·호출 상한으로 잘렸다(`error`는 None) — 오류 없이 끝난 턴과 가르려고.
    pub cut_short: bool,
    pub calls: Vec<CallStat>,
    pub ops: Vec<OpStat>,
}

pub const CUT_SHORT: &str =
    "한 번에 할 수 있는 만큼 했어요. 이어서 하려면 \"계속\"이라고 보내 주세요.";
pub const BAD_ANSWER: &str = "AI가 정해진 모양으로 답하지 않았어요. 다시 보내 주세요.";
/// 프롬프트에 싣는 지난 대화 수.
pub const HISTORY_MAX: usize = 40;

/// 답 모양이 연달아 이만큼 틀리면 턴을 끝낸다(AI 노드의 재시도 수와 같다).
const BAD_ANSWERS: usize = 2;
/// 고정 문구 — AI가 낸 값은 싣지 않는다(구분선 밖이라 지시로 읽힌다).
const BAD_ANSWER_NOTICE: &str = "[시스템] 직전 답을 읽지 못했음. say와 actions 두 칸(둘 다 문자열, actions는 JSON 배열을 담은 문자열) 모양을 지켜 다시 답할 것";
const EDITING_NOTICE: &str = "[시스템] 사용자가 캔버스에서 고치는 중이라 실행하지 않았음. say로 사용자에게 알리고, 다음 답에서 status로 다시 확인한 뒤 다시 시도할 것";

fn field(key: &str, description: &str) -> OutputField {
    OutputField {
        key: key.into(),
        ftype: "text".into(),
        description: Some(description.into()),
    }
}

/// 답 글자 → (say, actions). 모양이 틀리면 갈래만 — 이유에는 AI가 낸 값이 섞여 있어 프롬프트에 싣지 않는다.
fn parse(text: &str, fields: &[OutputField]) -> Result<(String, Vec<Value>), CallOutcome> {
    let out = parse_structured_output(text, fields).map_err(|_| CallOutcome::Outer)?;
    let say = out["say"].as_str().unwrap_or("").trim().to_string();
    let raw = out["actions"].as_str().unwrap_or("").trim();
    let actions = if raw.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(raw).map_err(|_| CallOutcome::ActionsJson)?
    };
    Ok((say, actions))
}

/// `ui` 동작이면 새로 띄우는지(create) 고치는지(update).
fn ui_kind(op: &Op) -> Option<&'static str> {
    match op {
        Op::Ui { messages } if messages.iter().any(|m| m.get("createSurface").is_some()) => {
            Some("create")
        }
        Op::Ui { .. } => Some("update"),
        _ => None,
    }
}

fn op_stat(op: String, ui: Option<&'static str>, reply: &Reply) -> OpStat {
    let rule = reply.data["rule"].as_str();
    OpStat {
        op,
        ok: reply.ok,
        code: reply.code,
        rule: UI_RULES.iter().copied().find(|r| Some(*r) == rule),
        errors: reply.data["errors"].as_array().map_or(0, Vec::len),
        ui,
    }
}

/// 한 턴: AI에게 묻고 → 동작을 실행하고 → 결과를 붙여 다시 묻기를 `actions`가 빌 때까지(또는 카드·상한까지).
pub async fn run_turn(
    runner: &AiRunner,
    actions: &dyn Actions,
    turn: Turn<'_>,
    limits: Limits,
    on_stage: &(dyn Fn(Stage) + Send + Sync),
) -> TurnResult {
    let started = Instant::now();
    let cli = turn.ai.provider.starts_with("cli:");
    let fields = [
        field("say", "사용자에게 할 말"),
        field(
            "actions",
            "실행할 동작의 JSON 배열을 담은 문자열. 없으면 []",
        ),
    ];
    let schema = build_output_schema(&fields);
    let mut says: Vec<String> = Vec::new();
    let mut r = TurnResult {
        say: String::new(),
        cards: Vec::new(),
        usage: Vec::new(),
        error: None,
        cut_short: false,
        calls: Vec::new(),
        ops: Vec::new(),
    };
    let system = match turn.blueprint {
        Some(_) => format!("{INSTRUCTION}\n\n{BLUEPRINT_INSTRUCTION}"),
        None => INSTRUCTION.to_string(),
    };
    let summary = turn.blueprint.as_ref().and_then(|b| b.summary);
    let mut results: Vec<String> = Vec::new();
    let mut notice: Option<String> = None;
    let (mut calls, mut tests, mut bad) = (0, 0, 0);

    'turn: loop {
        let left = limits.time.saturating_sub(started.elapsed());
        if left.is_zero() || calls >= limits.calls {
            says.push(CUT_SHORT.into());
            r.cut_short = true;
            break;
        }
        on_stage(Stage::Designing);
        let status = actions.apply(Op::Status {}).await;
        let req = AiRequest {
            prompt: prompt::build(
                &prompt::tail(&status),
                turn.now,
                turn.history,
                turn.text,
                &results,
                notice.take().as_deref(),
                cli,
                summary,
            ),
            model: turn.ai.model.clone(),
            effort: turn.ai.effort.clone(),
            schema: Some(schema.clone()),
            key_name: None,
            config_dir: None,
            system: Some(system.clone()),
        };
        calls += 1;
        let ask = runner.ask(
            &turn.ai.provider,
            turn.ai.credential.as_deref(),
            req,
            &|_: &str| {},
        );
        let asked = Instant::now();
        let answer = tokio::time::timeout(left, ask).await;
        let ms = u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX);
        let stat = |outcome, usage| CallStat { ms, outcome, usage };
        let answer = match answer {
            Err(_) => {
                r.calls.push(stat(CallOutcome::Timeout, None));
                says.push(CUT_SHORT.into());
                r.cut_short = true;
                break;
            }
            Ok(Err(AiError(m))) => {
                r.calls.push(stat(CallOutcome::AiError(m.clone()), None));
                r.error = Some(m);
                break;
            }
            Ok(Ok(a)) => a,
        };
        r.usage.extend(answer.usage.clone());
        let parsed = parse(&answer.text, &fields);
        let outcome = parsed.as_ref().err().cloned().unwrap_or(CallOutcome::Ok);
        r.calls.push(stat(outcome, answer.usage));
        let Ok((say, todo)) = parsed else {
            bad += 1;
            if bad >= BAD_ANSWERS {
                r.error = Some(BAD_ANSWER.into());
                break;
            }
            notice = Some(BAD_ANSWER_NOTICE.into());
            continue;
        };
        bad = 0;
        if !say.is_empty() {
            says.push(say);
        }
        if todo.is_empty() {
            break;
        }
        for action in todo {
            let shown = action.to_string();
            // `from_value`가 통과하면 이것이 kebab 동작 이름이다.
            let name = action["op"].as_str().unwrap_or_default().to_string();
            let mut done = false;
            let line = match Op::from_value(action) {
                Err(reply) => reply.to_line(),
                // 설계서 모드가 꺼져 있으면 모르는 동작과 같다 — Builder를 부르지 않는다.
                Ok(op @ (Op::Ui { .. } | Op::BindFolder { .. })) if turn.blueprint.is_none() => {
                    let reply = Reply::err(Code::BadRequest);
                    r.ops.push(op_stat(name, ui_kind(&op), &reply));
                    reply.to_line()
                }
                // 폴더 창은 AI의 답만으로 뜨지 않는다 — 카드의 버튼을 사람이 눌러야 열린다.
                Ok(Op::PickFolder { id, field }) => {
                    r.cards.push(Card::PickFolder { node_id: id, field });
                    done = true;
                    let reply = Reply::ok(json!({"status": "awaiting_user"}));
                    r.ops.push(op_stat(name, None, &reply));
                    reply.to_line()
                }
                Ok(Op::Test { .. }) if tests >= limits.tests => format!(
                    r#"{{"ok":false,"code":"TURN_TEST_LIMIT","message":"이번 턴에는 테스트를 {}번까지만 할 수 있음. 사용자에게 알리고 턴을 마칠 것"}}"#,
                    limits.tests
                ),
                Ok(op) => {
                    let test_of = match &op {
                        Op::Test { id, .. } => Some(id.clone()),
                        _ => None,
                    };
                    let finish = matches!(op, Op::Finish {});
                    let ui = ui_kind(&op);
                    if test_of.is_some() {
                        tests += 1;
                        on_stage(Stage::Testing);
                    }
                    let reply = actions.apply(op).await;
                    r.ops.push(op_stat(name, ui, &reply));
                    if let (Some(node_id), Some(run_id)) = (test_of, reply.data["runId"].as_str()) {
                        r.cards.push(Card::Test {
                            node_id,
                            run_id: run_id.into(),
                            status: reply.data["status"].as_str().unwrap_or("").into(),
                            http_sent: reply.data["httpSent"] == true,
                        });
                    }
                    if finish && reply.ok && reply.data["status"] == "awaiting_user" {
                        r.cards.push(Card::Finish {});
                        done = true;
                    }
                    // 설계서가 떴다 — pick-folder·finish처럼 사람 차례라 턴을 끝낸다.
                    if ui.is_some()
                        && reply.ok
                        && let Some(version) = reply.data["version"]
                            .as_u64()
                            .and_then(|v| u32::try_from(v).ok())
                    {
                        r.cards.push(Card::Blueprint { version });
                        done = true;
                    }
                    if reply.code == Some(Code::UserEditing) {
                        results.push(format!("{shown} => {}", reply.to_line()));
                        notice = Some(EDITING_NOTICE.into());
                        break;
                    }
                    reply.to_line()
                }
            };
            results.push(format!("{shown} => {line}"));
            if done {
                break 'turn;
            }
        }
    }
    r.say = says.join("\n");
    r
}
