//! 모델 드롭다운 목록 — 가격표·codex·agy 출력 읽기.

use ieum_desktop_lib::models::{ModelChoice, list, parse_agy, parse_codex, priced};

fn ids(v: &[ModelChoice]) -> Vec<&str> {
    v.iter().map(|m| m.id.as_str()).collect()
}

#[test]
fn priced_models_follow_table_order_per_company() {
    let claude = list("cli:claude").unwrap();
    assert_eq!(claude, list("api:anthropic").unwrap()); // 구독도 같은 표
    assert!(ids(&claude).iter().all(|m| m.starts_with("claude-")));
    assert_eq!(claude[0].id, "claude-haiku-5-5"); // 표 순서 그대로
    assert!(
        ids(&list("api:openai").unwrap())
            .iter()
            .all(|m| m.starts_with("gpt-"))
    );
    assert!(
        ids(&list("api:gemini").unwrap())
            .iter()
            .all(|m| m.starts_with("gemini-"))
    );
    assert!(priced("nope-").is_empty());
    assert!(list("cli:shell").is_err());
}

#[test]
fn codex_keeps_only_listed_models_in_order() {
    let json = r#"{"models":[
        {"slug":"gpt-a","display_name":"GPT A","visibility":"list"},
        {"slug":"hidden","display_name":"H","visibility":"hide"},
        {"slug":"gpt-b","visibility":"list"}
    ]}"#;
    assert_eq!(
        parse_codex(json).unwrap(),
        vec![
            ModelChoice {
                id: "gpt-a".into(),
                label: "GPT A".into()
            },
            ModelChoice {
                id: "gpt-b".into(),
                label: "gpt-b".into()
            },
        ]
    );
    assert!(parse_codex("not json").is_err());
    assert!(parse_codex("{}").is_err());
}

#[test]
fn agy_reads_tab_lines_and_skips_the_banner() {
    let out = "Fetching available models...\ngemini-4-argon-medium\tGemini 4 Argon (Medium)\nclaude-x\t\n\n";
    assert_eq!(
        parse_agy(out),
        vec![
            ModelChoice {
                id: "gemini-4-argon-medium".into(),
                label: "Gemini 4 Argon (Medium)".into()
            },
            ModelChoice {
                id: "claude-x".into(),
                label: "claude-x".into()
            },
        ]
    );
}
