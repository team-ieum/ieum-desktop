//! 채팅 패널 대화 루프. Tauri·Store를 모른다 — 저장과 이벤트 송신은 앱(`src-tauri/src/chat.rs`)이 한다.
//! Builder는 `Actions` 이음매 하나로만 부른다.

mod prompt;

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::ai::{
    AiError, AiRequest, AiRunner, AiUsage, OutputField, build_output_schema,
    parse_structured_output,
};
use crate::builder::{AiDefault, ChatMessage, Code, Op, Reply};

pub use prompt::INSTRUCTION;

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
    },
    PickFolder {
        node_id: String,
        field: String,
    },
    Finish {},
}

pub struct Turn<'a> {
    pub text: &'a str,
    pub history: &'a [ChatMessage],
    pub ai: &'a AiDefault,
    pub now: &'a str,
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub say: String,
    pub cards: Vec<Card>,
    /// 호출마다 한 건(실패한 턴도 쓴 만큼).
    pub usage: Vec<AiUsage>,
    pub error: Option<String>,
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

/// 답 글자 → (say, actions). 모양이 틀리면 None — 이유에는 AI가 낸 값이 섞여 있어 프롬프트에 싣지 않는다.
fn parse(text: &str, fields: &[OutputField]) -> Option<(String, Vec<Value>)> {
    let out = parse_structured_output(text, fields).ok()?;
    let say = out["say"].as_str().unwrap_or("").trim().to_string();
    let raw = out["actions"].as_str().unwrap_or("").trim();
    let actions = if raw.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(raw).ok()?
    };
    Some((say, actions))
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
    };
    let mut results: Vec<String> = Vec::new();
    let mut notice: Option<String> = None;
    let (mut calls, mut tests, mut bad) = (0, 0, 0);

    'turn: loop {
        let left = limits.time.saturating_sub(started.elapsed());
        if left.is_zero() || calls >= limits.calls {
            says.push(CUT_SHORT.into());
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
            ),
            model: turn.ai.model.clone(),
            schema: Some(schema.clone()),
            key_name: None,
            config_dir: None,
            system: Some(INSTRUCTION.into()),
        };
        calls += 1;
        let ask = runner.ask(
            &turn.ai.provider,
            turn.ai.credential.as_deref(),
            req,
            &|_: &str| {},
        );
        let answer = match tokio::time::timeout(left, ask).await {
            Err(_) => {
                says.push(CUT_SHORT.into());
                break;
            }
            Ok(Err(AiError(m))) => {
                r.error = Some(m);
                break;
            }
            Ok(Ok(a)) => a,
        };
        r.usage.extend(answer.usage);
        let Some((say, todo)) = parse(&answer.text, &fields) else {
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
            let mut done = false;
            let line = match Op::from_value(action) {
                Err(reply) => reply.to_line(),
                // 폴더 창은 AI의 답만으로 뜨지 않는다 — 카드의 버튼을 사람이 눌러야 열린다.
                Ok(Op::PickFolder { id, field }) => {
                    r.cards.push(Card::PickFolder { node_id: id, field });
                    done = true;
                    r#"{"ok":true,"data":{"status":"awaiting_user"}}"#.to_string()
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
                    if test_of.is_some() {
                        tests += 1;
                        on_stage(Stage::Testing);
                    }
                    let reply = actions.apply(op).await;
                    if let (Some(node_id), Some(run_id)) = (test_of, reply.data["runId"].as_str()) {
                        r.cards.push(Card::Test {
                            node_id,
                            run_id: run_id.into(),
                            status: reply.data["status"].as_str().unwrap_or("").into(),
                        });
                    }
                    if finish && reply.ok && reply.data["status"] == "awaiting_user" {
                        r.cards.push(Card::Finish {});
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
