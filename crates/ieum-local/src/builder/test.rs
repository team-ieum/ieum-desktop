//! test 동작 — 트리거부터 대상 노드까지만 실제 파일로 실행하고, AI에게는 상태·출력 키와 타입·오류 코드만 준다.

use std::fs;
use std::path::{Path, PathBuf};

use ieum_engine::{NodeStatus, RunStatus, Workflow, upstream_ids};
use serde_json::{Map, Value, json};

use super::{Builder, Code, Doc, Reply, TestEntry, TestStatus, node_hash};

/// 대상 + 조상만 남긴 그래프(노드는 원래 순서, 엣지는 양 끝이 다 남은 것만). 없는 id면 None.
pub fn subgraph_to(wf: &Workflow, id: &str) -> Option<Workflow> {
    wf.nodes.iter().find(|n| n.id == id)?;
    let mut keep = upstream_ids(wf, id);
    keep.insert(id.to_string());
    let mut sub = wf.clone();
    sub.nodes.retain(|n| keep.contains(&n.id));
    sub.edges
        .retain(|e| keep.contains(&e.source) && keep.contains(&e.target));
    Some(sub)
}

/// 트리거 입력 파일. `file`이 있으면 그 파일(감시 폴더 **바로 아래**의 일반 파일, 심볼릭 링크 아님 — 아니면 SampleNotAllowed),
/// 없으면 폴더 바로 아래 일반 파일 중 가장 최근에 바뀐 것(이름이 `.`으로 시작하거나 `Icon\r`인 것은 뺀다 — 감시 규칙과 같다).
/// 폴더를 못 열면 FolderMissing, 파일이 없으면 NoSampleFile. 실제 경로(canonicalize)를 돌려준다.
pub fn pick_sample(folder: &Path, file: Option<&Path>) -> Result<PathBuf, Code> {
    let dir = folder.canonicalize().map_err(|_| Code::FolderMissing)?;
    if let Some(file) = file {
        let plain = fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_file());
        let real = file.canonicalize().map_err(|_| Code::SampleNotAllowed)?;
        if !plain || real.parent() != Some(&dir) {
            return Err(Code::SampleNotAllowed);
        }
        return Ok(real);
    }
    fs::read_dir(&dir)
        .map_err(|_| Code::FolderMissing)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name != "Icon\r"
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
        .ok_or(Code::NoSampleFile)
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `<CODE>` 또는 `<CODE> <칸 key>` → (코드, 칸). 모르는 글자는 그대로 흘리지 않는다.
fn failure(error_code: Option<&str>) -> (Code, Option<&str>) {
    let Some((head, rest)) = error_code.map(|c| c.split_once(' ').unwrap_or((c, ""))) else {
        return (Code::NodeFailed, None);
    };
    let Ok(code) = serde_json::from_value::<Code>(Value::String(head.to_string())) else {
        return (Code::NodeFailed, None);
    };
    let safe = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (code, safe.then_some(rest))
}

impl Builder {
    pub(super) async fn test(
        &self,
        id: &str,
        doc: &Doc,
        node_id: &str,
        file: Option<&str>,
    ) -> Result<Value, Reply> {
        let sub = subgraph_to(&doc.workflow, node_id).ok_or(Code::NodeNotFound)?;
        let errors = self.errors(&sub);
        if !errors.is_empty() {
            return Err(Reply::err_with(Code::Invalid, json!({"errors": errors})));
        }
        // 검증을 통과했으니 트리거는 하나이고 폴더 칸이 차 있다.
        let folder = sub
            .nodes
            .iter()
            .filter(|n| n.event == "local.folder_new_file")
            .find_map(|n| n.config.get("folder")?.as_str())
            .ok_or(Code::FolderMissing)?;
        let sample = pick_sample(Path::new(folder), file.map(Path::new))?;
        let run = self
            .host
            .run_test(id, sub.clone(), &sample)
            .await
            .map_err(|_| Code::Internal)?;
        if run.status == RunStatus::Rejected {
            return Err(Code::Invalid.into());
        }

        // 되돌리기가 하나라도 실패하면 노드가 성공했어도 테스트는 실패다.
        let undo_failed = !run.undo.skipped.is_empty();
        let mut lines = Vec::new();
        let mut entries = Vec::new();
        let mut first_failure = None;
        let mut target = None;
        for node in &sub.nodes {
            let Some(rec) = run.nodes.get(&node.id) else {
                continue;
            };
            let status = match rec.status {
                NodeStatus::NotRun => continue,
                NodeStatus::Succeeded => TestStatus::Success,
                NodeStatus::Skipped => TestStatus::Skipped,
                NodeStatus::Failed => TestStatus::Failed,
            };
            // 값은 싣지 않는다 — 출력 키와 타입 이름만.
            let outputs: Map<String, Value> = rec
                .output
                .iter()
                .flatten()
                .map(|(k, v)| (k.clone(), type_name(v).into()))
                .collect();
            let mut line = json!({"id": node.id, "status": status, "outputs": outputs});
            if status == TestStatus::Failed {
                // `rec.error` 원문에는 경로가 섞인다 — 코드와 고정 문구만.
                let (code, field) = failure(rec.error_code.as_deref());
                line["code"] = json!(code);
                line["message"] = code.message().into();
                if let Some(field) = field {
                    line["field"] = field.into();
                }
                first_failure.get_or_insert(code);
            }
            lines.push(line);
            let is_target = node.id == node_id;
            if is_target {
                target = Some(status);
            }
            let status = match is_target && undo_failed {
                true => TestStatus::Failed,
                false => status,
            };
            let hash = node_hash(node);
            entries.push((node.id.clone(), TestEntry { hash, status }));
        }
        self.update_chat(id, |s| s.tests.extend(entries.clone()))?;

        let failed = undo_failed || first_failure.is_some() || target.is_none();
        let data = json!({
            "runId": run.run_id,
            "status": match (failed, target) {
                (false, Some(status)) => status,
                _ => TestStatus::Failed,
            },
            "nodes": lines,
            "undo": {"restored": run.undo.restored.len(), "failed": run.undo.skipped.len()},
        });
        let code = match first_failure {
            _ if undo_failed => Code::UndoFailed,
            Some(code) => code,
            None if target.is_none() => Code::NodeFailed,
            None => return Ok(data),
        };
        Err(Reply::err_with(code, data))
    }
}
