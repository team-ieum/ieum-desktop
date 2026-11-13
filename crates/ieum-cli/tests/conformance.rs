//! 적합성 묶음과 `ieum-dev` 바이너리. 실제 AI·파일 노드는 돌리지 않는다(트리거 + `branch.if`만).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

fn ieum(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ieum-dev"))
        .args(args)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance")
}

#[test]
fn all_conformance_cases_pass() {
    let dir = conformance_dir();
    let cases = fs::read_dir(&dir)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().is_dir())
        .count();
    // 계획 목록: 실행 9개 + 검증 코드 9개
    assert!(cases >= 18, "케이스 {cases}개");
    let out = ieum(&["conformance", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let out = ieum(&["conformance", dir.to_str().unwrap(), "--json"]);
    let results = lines(&out.stdout);
    assert_eq!(results.len(), cases, "{}", text(&out));
    for r in &results {
        assert_eq!(r["ok"], true, "{r}");
    }
}

/// (이름, 워크플로우) — 트리거 + 조건 노드 하나.
fn write(dir: &TempDir, name: &str, cond_config: Value, triggers: usize) -> String {
    let folder = dir.path().to_str().unwrap();
    let mut nodes: Vec<Value> = (0..triggers)
        .map(|i| json!({"id": format!("t{i}"), "type": "TRIGGER", "event": "local.folder_new_file", "label": "새 파일", "config": {"folder": folder}}))
        .collect();
    nodes.push(json!({"id": "c", "type": "CONDITION", "event": "branch.if", "label": "확장자 확인", "config": cond_config}));
    let wf = json!({"nodes": nodes, "edges": [{"id": "e", "source": "t0", "target": "c"}]});
    let path = dir.path().join(name);
    fs::write(&path, wf.to_string()).unwrap();
    path.to_str().unwrap().to_string()
}

fn legacy() -> Value {
    json!({"operator": "equals", "left": "{{nodes.t0.output.extension}}", "right": "pdf"})
}

/// 예약어 key — 실행 시 노드 실패(검증은 통과).
fn broken() -> Value {
    json!({"branches": [{"key": "else", "name": "x", "rules": [{"left": "a", "operator": "equals", "right": "a"}]}]})
}

#[test]
fn cli_exit_codes() {
    let dir = TempDir::new().unwrap();
    let ok = write(&dir, "ok.json", legacy(), 1);
    let fail = write(&dir, "fail.json", broken(), 1);
    let invalid = write(&dir, "invalid.json", legacy(), 2);
    let missing = dir.path().join("none.json");

    let out = ieum(&["run", &ok]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("확장자 확인"), "{stdout}");

    let out = ieum(&["run", &fail, "--yes"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let out = ieum(&["run", &invalid]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let out = ieum(&["run", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    let out = ieum(&["run"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    // --input 모양: 최상위·trigger·nodeOutputs가 객체가 아니면 사용법 에러
    for bad in [
        json!([1]),
        json!({"trigger": "a.pdf"}),
        json!({"nodeOutputs": []}),
    ] {
        let input = dir.path().join("bad-input.json");
        fs::write(&input, bad.to_string()).unwrap();
        let out = ieum(&["run", &ok, "--input", input.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(3), "{bad}: {}", text(&out));
    }
    let out = ieum(&["conformance", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
}

fn lines(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

#[test]
fn cli_json_lines_parse() {
    let dir = TempDir::new().unwrap();
    let ok = write(&dir, "ok.json", legacy(), 1);
    let input = dir.path().join("input.json");
    fs::write(
        &input,
        json!({"trigger": {"path": "/in/a.pdf", "name": "a.pdf", "extension": "pdf", "size": 1}})
            .to_string(),
    )
    .unwrap();

    let out = ieum(&["run", &ok, "--input", input.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let ls = lines(&out.stdout);
    assert_eq!(ls[0]["kind"], "runStarted");
    let last = ls.last().unwrap();
    assert_eq!(last["status"], "succeeded");
    assert_eq!(last["nodes"]["t0"]["output"]["name"], "a.pdf");
    assert_eq!(last["nodes"]["c"]["output"]["_branchKeys"], json!(["true"]));

    // 검증 에러 — 표준 출력 마지막 줄은 거부된 RunResult, 표준 에러는 {code, message}
    let invalid = write(&dir, "invalid.json", legacy(), 2);
    let out = ieum(&["run", &invalid, "--json"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let last = lines(&out.stdout).pop().unwrap();
    assert_eq!(last["status"], "rejected");
    assert_eq!(last["rejectReason"], json!({"kind": "INVALID"}));
    let errs = lines(&out.stderr);
    assert_eq!(errs[0]["code"], "TRIGGER_COUNT");
    assert!(errs[0]["message"].is_string());

    let out = ieum(&["run", "/없는/파일.json", "--json"]);
    assert_eq!(lines(&out.stderr)[0]["code"], "IO");
    let out = ieum(&["run", "--json"]);
    assert_eq!(lines(&out.stderr)[0]["code"], "USAGE");
}
