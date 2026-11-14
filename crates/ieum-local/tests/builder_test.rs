//! Builder test 동작 — 대상까지 자르기·샘플 고르기·결과 숨기기·상태 기록.

mod builder_support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use builder_support::MemHost;
use ieum_engine::{NodeRecord, NodeStatus, RunStatus, Workflow};
use ieum_local::builder::*;
use ieum_local::{FileOp, FileOpKind, UndoReport, UndoSkip};
use serde_json::{Value, json};
use tempfile::TempDir;

const W: &str = "11111111-1111-4111-8111-111111111111";

async fn run(b: &Builder, op: Value) -> Reply {
    b.apply(W, Op::from_value(op).unwrap(), None).await
}

/// t → a → m + 곁가지 t → x
fn wf(watch: &Path, base: &Path) -> Value {
    json!({
        "nodes": [
            {"id": "t", "type": "TRIGGER", "event": "local.folder_new_file", "config": {"folder": watch}},
            {"id": "a", "type": "AI", "event": "local.ai", "config": {"provider": "cli:claude", "prompt": "p", "outputFields": [{"key": "folder", "type": "text"}]}},
            {"id": "m", "type": "ACTION", "event": "local.file_move", "config": {"path": "{{nodes.t.output.path}}", "baseFolder": base, "destination": "{{nodes.a.output.folder}}"}},
            {"id": "x", "type": "ACTION", "event": "local.file_trash", "config": {"path": "{{nodes.t.output.path}}"}}
        ],
        "edges": [{"source": "t", "target": "a"}, {"source": "a", "target": "m"}, {"source": "t", "target": "x"}]
    })
}

fn rec(
    status: NodeStatus,
    output: Option<Value>,
    error: Option<&str>,
    code: Option<&str>,
) -> NodeRecord {
    NodeRecord {
        status,
        output: output.map(|o| o.as_object().unwrap().clone()),
        error: error.map(String::from),
        usage: None,
        error_code: code.map(String::from),
    }
}

/// t·a·m 모두 성공, 되돌릴 것 없음.
fn ok_run() -> TestRun {
    let ok = |output: Value| rec(NodeStatus::Succeeded, Some(output), None, None);
    let nodes: BTreeMap<String, NodeRecord> = [
        (
            "t",
            ok(json!({"path": "/h/in/비밀보고서.pdf", "name": "비밀보고서.pdf", "extension": "pdf", "size": 10})),
        ),
        ("a", ok(json!({"folder": "기밀"}))),
        (
            "m",
            ok(json!({"path": "/h/out/기밀/비밀보고서.pdf", "name": "비밀보고서.pdf"})),
        ),
    ]
    .into_iter()
    .map(|(id, r)| (id.to_string(), r))
    .collect();
    TestRun {
        run_id: "run-1".into(),
        status: RunStatus::Succeeded,
        nodes,
        undo: UndoReport::default(),
    }
}

/// MemHost + Builder + watch 경로(안에 `비밀보고서.pdf`). 마지막은 임시 폴더를 살려 두는 손잡이.
fn setup() -> (Arc<MemHost>, Builder, PathBuf, TempDir) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let (watch, base) = (root.join("watch"), root.join("base"));
    std::fs::create_dir_all(&watch).unwrap();
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(watch.join("비밀보고서.pdf"), "pdf").unwrap();
    let h = MemHost::with(W, wf(&watch, &base));
    let b = Builder::new(h.clone()).unwrap();
    (h, b, watch, t)
}

fn test_op(id: &str) -> Value {
    json!({"op": "test", "args": {"id": id}})
}

#[test]
fn subgraph_keeps_only_target_and_ancestors() {
    let w: Workflow = serde_json::from_value(wf(Path::new("/h/in"), Path::new("/h/out"))).unwrap();
    let s = subgraph_to(&w, "m").unwrap();
    assert_eq!(
        s.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["t", "a", "m"]
    );
    assert_eq!(s.edges.len(), 2);
    assert_eq!(subgraph_to(&w, "t").unwrap().nodes.len(), 1);
    assert!(subgraph_to(&w, "zz").is_none());
}

#[test]
fn sample_is_latest_plain_file_directly_in_folder() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().canonicalize().unwrap();
    assert_eq!(pick_sample(&dir, None), Err(Code::NoSampleFile));
    assert_eq!(
        pick_sample(&dir.join("없음"), None),
        Err(Code::FolderMissing)
    );
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for (name, age) in [
        ("old.txt", Some(old)),
        ("new.txt", None),
        (".DS_Store", None),
    ] {
        let f = std::fs::File::create(dir.join(name)).unwrap();
        if let Some(a) = age {
            f.set_modified(a).unwrap();
        }
    }
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/inner.txt"), "x").unwrap();
    std::os::unix::fs::symlink(dir.join("new.txt"), dir.join("link.txt")).unwrap();
    assert_eq!(pick_sample(&dir, None), Ok(dir.join("new.txt")));
    assert_eq!(
        pick_sample(&dir, Some(&dir.join("old.txt"))),
        Ok(dir.join("old.txt"))
    );
    for bad in ["sub/inner.txt", "link.txt", "sub", "없는.txt"] {
        assert_eq!(
            pick_sample(&dir, Some(&dir.join(bad))),
            Err(Code::SampleNotAllowed),
            "{bad}"
        );
    }
    let other = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(
        pick_sample(&dir, Some(other.path())),
        Err(Code::SampleNotAllowed)
    );
}

#[tokio::test]
async fn test_runs_pruned_graph_and_records_ancestors() {
    let (h, b, watch, _t) = setup();
    h.push_test_run(Ok(ok_run()));
    let r = run(&b, test_op("m")).await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.data["status"], "success");
    let (ran, sample) = h.tested().remove(0);
    // 곁가지 x는 돌지 않는다
    assert_eq!(
        ran.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["t", "a", "m"]
    );
    assert_eq!(sample, watch.join("비밀보고서.pdf"));
    let s = run(&b, json!({"op": "status"})).await;
    let states: Vec<_> = s.data["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["id"].as_str().unwrap().to_string(),
                n["state"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("t", "success"),
            ("a", "success"),
            ("m", "success"),
            ("x", "passed")
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
    );
}

#[tokio::test]
async fn test_reply_hides_runtime_data() {
    let (h, b, watch, _t) = setup();
    let mut run_ = ok_run();
    let leak = format!(
        "파일을 찾을 수 없어요: {}: No such file",
        watch.join("비밀보고서.pdf").display()
    );
    run_.nodes.insert(
        "m".into(),
        rec(NodeStatus::Failed, None, Some(&leak), Some("FILE_MISSING")),
    );
    run_.status = RunStatus::Failed;
    h.push_test_run(Ok(run_));
    let r = run(&b, test_op("m")).await;
    assert_eq!(
        (r.ok, r.code, r.message.as_deref()),
        (false, Some(Code::FileMissing), Some("옮길 파일이 없음"))
    );
    assert_eq!(
        r.data["nodes"][0],
        json!({"id": "t", "status": "success", "outputs": {"path": "string", "name": "string", "extension": "string", "size": "number"}})
    );
    assert_eq!(r.data["nodes"][1]["outputs"], json!({"folder": "string"}));
    assert_eq!(
        r.data["nodes"][2],
        json!({"id": "m", "status": "failed", "outputs": {}, "code": "FILE_MISSING", "message": "옮길 파일이 없음"})
    );
    let line = r.to_line();
    for secret in ["비밀보고서", "기밀", "No such file", "찾을 수 없어요"] {
        assert!(
            !line.contains(secret),
            "{secret} 이(가) 응답에 샜다: {line}"
        );
    }
    // 상태: 조상은 통과, 대상은 실패.
    let st = h.load_chat(W).unwrap().state.tests;
    assert_eq!(
        (st["t"].status, st["a"].status, st["m"].status),
        (TestStatus::Success, TestStatus::Success, TestStatus::Failed)
    );
}

#[tokio::test]
async fn failure_codes_parse_field_and_fall_back() {
    let (h, b, _w, _t) = setup();
    for (code, want, field) in [
        (Some("FIELD_EMPTY path"), Code::FieldEmpty, json!("path")),
        (None, Code::NodeFailed, json!(null)),
        // 모르는 코드는 그대로 흘리지 않는다
        (Some("../etc/passwd 비밀"), Code::NodeFailed, json!(null)),
        (Some("FIELD_EMPTY ../비밀"), Code::FieldEmpty, json!(null)),
    ] {
        let mut run_ = ok_run();
        run_.nodes.insert(
            "m".into(),
            rec(NodeStatus::Failed, None, Some("원문"), code),
        );
        h.push_test_run(Ok(run_));
        let r = run(&b, test_op("m")).await;
        assert_eq!(r.code, Some(want), "{code:?}");
        assert_eq!(
            r.data["nodes"][2]
                .get("field")
                .cloned()
                .unwrap_or(json!(null)),
            field
        );
        assert!(!r.to_line().contains("비밀") && !r.to_line().contains("원문"));
    }
}

#[tokio::test]
async fn undo_skip_makes_test_failed() {
    let (h, b, watch, _t) = setup();
    let mut run_ = ok_run();
    let op = FileOp {
        kind: FileOpKind::Move,
        from: watch.join("비밀보고서.pdf"),
        to: PathBuf::from("/h/out/기밀/비밀보고서.pdf"),
        file_id: None,
    };
    run_.undo.skipped.push((op, UndoSkip::OriginalOccupied));
    h.push_test_run(Ok(run_));
    let r = run(&b, test_op("m")).await;
    assert_eq!((r.ok, r.code), (false, Some(Code::UndoFailed)));
    assert_eq!(r.data["undo"], json!({"restored": 0, "failed": 1}));
    assert!(!r.to_line().contains("비밀보고서"));
    let st = h.load_chat(W).unwrap().state.tests;
    assert_eq!(
        (st["t"].status, st["m"].status),
        (TestStatus::Success, TestStatus::Failed)
    );
}

#[tokio::test]
async fn skipped_target_passes_and_config_change_resets() {
    let (h, b, _w, _t) = setup();
    let mut run_ = ok_run();
    run_.nodes
        .insert("m".into(), rec(NodeStatus::Skipped, None, None, None));
    h.push_test_run(Ok(run_));
    let r = run(&b, test_op("m")).await;
    assert!(r.ok);
    assert_eq!(r.data["status"], "skipped");
    assert_eq!(
        h.load_chat(W).unwrap().state.tests["m"].status,
        TestStatus::Skipped
    );
    // 캔버스에서 칸을 바꾸면 그 노드만 untested로 돌아간다.
    h.put_doc(W, |d| {
        d.workflow.nodes[2]
            .config
            .insert("destination".into(), "다른곳".into());
    });
    let s = run(&b, json!({"op": "status"})).await;
    assert_eq!(
        (
            s.data["nodes"][0]["state"].as_str(),
            s.data["nodes"][2]["state"].as_str()
        ),
        (Some("success"), Some("untested"))
    );
}

#[tokio::test]
async fn test_refuses_before_running() {
    let (h, b, watch, _t) = setup();
    assert_eq!(run(&b, test_op("zz")).await.code, Some(Code::NodeNotFound));
    assert_eq!(
        run(
            &b,
            json!({"op": "test", "args": {"id": "m", "file": "/etc/hosts"}})
        )
        .await
        .code,
        Some(Code::SampleNotAllowed)
    );
    std::fs::remove_file(watch.join("비밀보고서.pdf")).unwrap();
    assert_eq!(run(&b, test_op("m")).await.code, Some(Code::NoSampleFile));
    h.put_doc(W, |d| {
        d.workflow.nodes[2].config.remove("baseFolder");
    });
    let r = run(&b, test_op("m")).await; // 필수 칸이 비면 실행하지 않는다
    assert_eq!(r.code, Some(Code::Invalid));
    assert_eq!(r.data["errors"][0]["code"], "REQUIRED");
    h.put_doc(W, |d| d.workflow.edges.retain(|e| e.target != "x")); // x는 트리거에서 닿지 않는다
    assert_eq!(run(&b, test_op("x")).await.code, Some(Code::Invalid));
    assert!(h.tested().is_empty());
    assert!(h.load_chat(W).unwrap().state.tests.is_empty());
    // 앱 쪽 실행이 못 돌면 INTERNAL, 상태는 그대로.
    std::fs::write(watch.join("b.txt"), "x").unwrap();
    h.push_test_run(Err("저장소 오류: /Users/비밀".into()));
    let r = run(&b, test_op("t")).await;
    assert_eq!(r.code, Some(Code::Internal));
    assert!(!r.to_line().contains("비밀"));
}
