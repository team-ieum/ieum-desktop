//! 모델 드롭다운 목록 — 가격표·codex·agy 출력 읽기, 모델별 추론 수준.

use ieum_desktop_lib::models::{
    AGY_MODELS_LINE, ModelChoice, list, parse_agy, parse_codex, priced,
};

fn ids(v: &[ModelChoice]) -> Vec<&str> {
    v.iter().map(|m| m.id.as_str()).collect()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn priced_models_follow_table_order_per_company() {
    let claude = list("cli:claude").unwrap();
    assert_eq!(claude, list("api:anthropic").unwrap()); // 구독도 같은 표
    let claude = claude.models;
    assert!(ids(&claude).iter().all(|m| m.starts_with("claude-")));
    assert_eq!(claude[0].id, "claude-haiku-5-5"); // 표 순서 그대로
    assert!(
        ids(&list("api:openai").unwrap().models)
            .iter()
            .all(|m| m.starts_with("gpt-"))
    );
    assert!(
        ids(&list("api:gemini").unwrap().models)
            .iter()
            .all(|m| m.starts_with("gemini-"))
    );
    assert!(priced("nope-").is_empty());
    assert!(list("cli:shell").is_err());
}

/// 화면에는 읽기 쉬운 이름, 값은 원래 id. 가격표에 `name`이 있으면 그것, 없으면 id에서 만든다.
#[test]
fn priced_models_show_readable_names_and_keep_ids() {
    use ieum_desktop_lib::models::readable;
    for (id, name) in [
        ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ("claude-fable-5-2", "Claude Fable 5.2"),
        ("gpt-6-astra", "GPT-6 Astra"),
        ("gpt-6.1-sol", "GPT-6.1 Sol"),
        ("gpt-5.6-luna", "GPT-5.6 Luna"),
        ("gemini-4-argon", "Gemini 4 Argon"),
        ("gemini-3.5-flash-lite", "Gemini 3.5 Flash Lite"),
    ] {
        assert_eq!(readable(id), name, "{id}");
    }
    let claude = list("cli:claude").unwrap().models;
    assert_eq!(
        (claude[0].id.as_str(), claude[0].label.as_str()),
        ("claude-haiku-5-5", "Claude Haiku 5.5")
    );
    assert!(
        list("api:openai")
            .unwrap()
            .models
            .iter()
            .all(|m| m.label != m.id && m.label.starts_with("GPT-"))
    );
}

/// 공급원·모델마다 고를 수 있는 수준. 기본 모델의 수준은 따로.
#[test]
fn priced_efforts_per_provider() {
    let five = s(&["low", "medium", "high", "xhigh", "max"]);
    let claude = list("cli:claude").unwrap();
    assert!(claude.models.iter().all(|m| m.efforts == five));
    assert_eq!(claude.default_efforts, five); // --effort는 모델과 무관
    let openai = list("api:openai").unwrap();
    assert!(openai.models.iter().all(|m| m.efforts == five));
    assert!(openai.default_efforts.is_empty()); // 기본 모델이 없다
    let gemini = list("api:gemini").unwrap();
    let of = |id: &str| {
        gemini
            .models
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .efforts
            .clone()
    };
    assert_eq!(of("gemini-4-argon"), s(&["low", "medium", "high"]));
    assert_eq!(
        of("gemini-3.5-flash-lite"),
        s(&["minimal", "low", "medium", "high"])
    );
    assert!(gemini.default_efforts.is_empty());
}

#[test]
fn codex_keeps_only_listed_models_in_order() {
    let json = r#"{"models":[
        {"slug":"gpt-a","display_name":"GPT A","visibility":"list","priority":5,
         "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"ultra"}]},
        {"slug":"hidden","display_name":"H","visibility":"hide","priority":0,
         "supported_reasoning_levels":[{"effort":"minimal"}]},
        {"slug":"gpt-b","visibility":"list","priority":2,
         "supported_reasoning_levels":[{"effort":"medium"},{"effort":"max"}]}
    ]}"#;
    let l = parse_codex(json).unwrap();
    assert_eq!(
        l.models,
        vec![
            ModelChoice {
                id: "gpt-a".into(),
                label: "GPT A".into(),
                efforts: s(&["low", "high"]), // ultra(자동 위임)는 뺀다
            },
            ModelChoice {
                id: "gpt-b".into(),
                label: "gpt-b".into(),
                efforts: s(&["medium", "max"]),
            },
        ]
    );
    // 기본 모델 = 보이는 모델 중 priority가 가장 낮은 것(숨긴 모델은 아님).
    assert_eq!(l.default_efforts, s(&["medium", "max"]));
    assert!(parse_codex("not json").is_err());
    assert!(parse_codex("{}").is_err());
}

#[test]
fn agy_groups_effort_suffixes_and_skips_the_banner() {
    let out = "Fetching available models...\n\
        gemini-4-argon-medium\tGemini 4 Argon (Medium)\n\
        gemini-4-argon-high\tGemini 4 Argon (High)\n\
        gemini-pro-orchestrator-g1-pro\tOrchestrator (Argon)\n\
        gemini-4-argon-low\tGemini 4 Argon (Low)\n\
        gemini-4-hydrogen-xhigh\tGemini 4 Hydrogen (XHigh)\n\
        gpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n\
        claude-x\t\n\n";
    assert_eq!(
        parse_agy(out),
        vec![
            ModelChoice {
                id: "gemini-4-argon".into(),
                label: "Gemini 4 Argon".into(),
                efforts: s(&["low", "medium", "high"]),
            },
            ModelChoice {
                // 접미사 없는 모델은 이름의 괄호도 그대로, 수준 없음.
                id: "gemini-pro-orchestrator-g1-pro".into(),
                label: "Orchestrator (Argon)".into(),
                efforts: vec![],
            },
            ModelChoice {
                id: "gemini-4-hydrogen".into(),
                label: "Gemini 4 Hydrogen".into(),
                efforts: s(&["xhigh"]),
            },
            ModelChoice {
                id: "gpt-oss-120b".into(),
                label: "GPT-OSS 120B".into(),
                efforts: s(&["medium"]),
            },
            ModelChoice {
                id: "claude-x".into(),
                label: "claude-x".into(),
                efforts: vec![],
            },
        ]
    );
}

#[test]
fn agy_models_line_turns_off_auto_update() {
    // 다른 agy 경로와 같다 — 목록 읽기가 자동 업데이트를 부르지 않게.
    assert!(AGY_MODELS_LINE.contains("AGY_CLI_DISABLE_AUTO_UPDATE=true"));
}
