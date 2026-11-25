//! `generate_handler!`에 등록한 명령이 `build.rs` COMMANDS와 capabilities 허용 목록에도 있는지.
//! 하나라도 빠지면 화면에서 "<명령> not allowed. Command not found"로 실패한다(단위 테스트로는 안 보인다).

use std::fs;
use std::path::Path;

fn read(rel: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

/// `generate_handler![ a::b, c::d ]` 안의 마지막 경로 조각들.
fn handler_commands() -> Vec<String> {
    let lib = read("src/lib.rs");
    let start =
        lib.find("generate_handler![").expect("generate_handler") + "generate_handler![".len();
    let end = start + lib[start..].find(']').unwrap();
    lib[start..end]
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with("//"))
        .map(|s| s.rsplit("::").next().unwrap().to_string())
        .collect()
}

#[test]
fn every_handler_command_is_allowed() {
    let build = read("build.rs");
    let caps = read("capabilities/default.json");
    let cmds = handler_commands();
    assert!(cmds.len() > 10, "명령 목록을 못 읽었다: {cmds:?}");
    let mut missing = Vec::new();
    for c in &cmds {
        if !build.contains(&format!("\"{c}\"")) {
            missing.push(format!("build.rs COMMANDS: {c}"));
        }
        let allow = format!("\"allow-{}\"", c.replace('_', "-"));
        if !caps.contains(&allow) {
            missing.push(format!("capabilities/default.json: {allow}"));
        }
    }
    assert!(
        missing.is_empty(),
        "허용 목록에 없는 명령:\n{}",
        missing.join("\n")
    );
}
