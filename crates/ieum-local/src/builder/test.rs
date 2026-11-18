//! test 동작 — 트리거부터 대상 노드까지만 실제 파일로 실행하고, AI에게는 상태·출력 키와 타입·오류 코드만 준다.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ieum_engine::schedule::schedule_output;
use ieum_engine::{NodeStatus, RunStatus, Workflow, upstream_ids};
use serde_json::{Map, Value, json};

use super::finish::http_lines;
use super::{
    Builder, Code, Doc, HttpConfirm, HttpLine, Reply, TestEntry, TestStatus, TestTrigger,
    node_hash, workflow_hash,
};

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

pub(super) fn type_name(v: &Value) -> &'static str {
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
        // 검증을 통과했으니 트리거는 하나다. 폴더 트리거면 그 폴더 칸이 차 있다.
        // 스케줄·수동 트리거는 파일 없이 트리거 출력을 만든다 — `file`이 넘어와도 무시한다.
        let trigger = sub
            .nodes
            .iter()
            .find(|n| {
                n.r#type
                    .as_deref()
                    .is_some_and(|t| t.eq_ignore_ascii_case("TRIGGER"))
            })
            .ok_or(Code::FolderMissing)?;
        let trigger = match trigger.event.as_str() {
            e if e.starts_with("schedule.") => TestTrigger::Output(schedule_output(
                e,
                &trigger.config,
                SystemTime::now().into(),
            )),
            "manual.run" => TestTrigger::Output(Map::new()),
            _ => {
                let folder = trigger
                    .config
                    .get("folder")
                    .and_then(Value::as_str)
                    .ok_or(Code::FolderMissing)?;
                TestTrigger::File(pick_sample(Path::new(folder), file.map(Path::new))?)
            }
        };
        // HTTP 요청은 되돌릴 수 없고 참조식으로 값이 밖에 나갈 수 있다 — GET이어도 사람이 먼저 본다.
        let lines = http_lines(&sub);
        let sent = !lines.is_empty();
        let sub = match sent {
            false => sub,
            true => match self.host.confirm_http(id, &lines).await {
                HttpConfirm::Cancelled => return Err(Code::TestDeclined.into()),
                HttpConfirm::Skip => return self.skip_http(id, &sub, &lines),
                HttpConfirm::Send => {
                    // 기다리는 사이(최대 5분) 캔버스가 고쳤을 수 있다 — 사람이 본 그래프와 같을 때만 보낸다.
                    let now = self.load(id)?;
                    if workflow_hash(&now.workflow) != workflow_hash(&doc.workflow) {
                        return Err(Code::Stale.into());
                    }
                    if self.host.user_editing(id) {
                        return Err(Code::UserEditing.into());
                    }
                    subgraph_to(&now.workflow, node_id).ok_or(Code::NodeNotFound)?
                }
            },
        };
        let run = self
            .host
            .run_test(id, sub.clone(), trigger)
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
                line["message"] = match code {
                    // 입력 검사 문구는 agent 고정 문구라 사용자 값이 없다 — 어느 칸인지 AI가 알게 그대로.
                    Code::GithubInput => rec.error.clone().unwrap_or_default().into(),
                    _ => code.message().into(),
                };
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
        let mut data = json!({
            "runId": run.run_id,
            "status": match (failed, target) {
                (false, Some(status)) => status,
                _ => TestStatus::Failed,
            },
            "nodes": lines,
            "undo": {"restored": run.undo.restored.len(), "failed": run.undo.skipped.len()},
        });
        if sent {
            data["httpSent"] = true.into();
        }
        let code = match first_failure {
            _ if undo_failed => Code::UndoFailed,
            Some(code) => code,
            None if target.is_none() => Code::NodeFailed,
            None => return Ok(data),
        };
        Err(Reply::err_with(code, data))
    }

    /// 사람이 [테스트 없이 넘어가기] — 요청 없이 HTTP 노드와 그 아래 노드를 건너뜀으로 적는다.
    /// HTTP보다 위 노드는 적지 않는다(따로 테스트해야 한다).
    fn skip_http(&self, id: &str, sub: &Workflow, lines: &[HttpLine]) -> Result<Value, Reply> {
        let http: HashSet<&str> = lines.iter().map(|l| l.node_id.as_str()).collect();
        let skipped: Vec<_> = sub
            .nodes
            .iter()
            .filter(|n| {
                http.contains(n.id.as_str())
                    || upstream_ids(sub, &n.id)
                        .iter()
                        .any(|u| http.contains(u.as_str()))
            })
            .collect();
        let entries: Vec<(String, TestEntry)> = skipped
            .iter()
            .map(|n| {
                let hash = node_hash(n);
                let entry = TestEntry {
                    hash,
                    status: TestStatus::Skipped,
                };
                (n.id.clone(), entry)
            })
            .collect();
        self.update_chat(id, |s| s.tests.extend(entries.clone()))?;
        let ids: Vec<&str> = skipped.iter().map(|n| n.id.as_str()).collect();
        Ok(json!({"status": "skipped_by_user", "nodes": ids}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_maps_new_codes() {
        assert_eq!(
            failure(Some("HTTP_STATUS 404")),
            (Code::HttpStatus, Some("404"))
        );
        assert_eq!(
            failure(Some("HTTP_UNREACHABLE")),
            (Code::HttpUnreachable, None)
        );
        assert_eq!(failure(Some("TOO_LARGE")), (Code::TooLarge, None));
        assert_eq!(
            failure(Some("FIELD_INVALID pattern")),
            (Code::FieldInvalid, Some("pattern"))
        );
        assert_eq!(
            failure(Some("FIELD_EMPTY url")),
            (Code::FieldEmpty, Some("url"))
        );
        assert_eq!(
            failure(Some("FIELD_INVALID webhookCredentialId")),
            (Code::FieldInvalid, Some("webhookCredentialId"))
        );
        // GitHub 노드 — AI가 칸·연결·저장소 중 무엇이 틀렸는지 가릴 수 있게.
        for (raw, want) in [
            ("GITHUB_INPUT", Code::GithubInput),
            ("GITHUB_NOT_CONNECTED", Code::GithubNotConnected),
            ("GITHUB_AUTH", Code::GithubAuth),
            ("GITHUB_UNREACHABLE", Code::GithubUnreachable),
            ("GITHUB_RATE_LIMIT", Code::GithubRateLimit),
            ("GITHUB_FORBIDDEN", Code::GithubForbidden),
            ("GITHUB_NOT_FOUND", Code::GithubNotFound),
            ("GITHUB_MOVED", Code::GithubMoved),
            ("GITHUB_GONE", Code::GithubGone),
            ("GITHUB_INVALID", Code::GithubInvalid),
            ("GITHUB_TOO_LARGE", Code::GithubTooLarge),
        ] {
            assert_eq!(failure(Some(raw)), (want, None), "{raw}");
        }
        assert_eq!(
            failure(Some("GITHUB_STATUS 429")),
            (Code::GithubStatus, Some("429"))
        );
    }
}
