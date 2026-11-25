//! finish·확인 대기·사람의 [켜기]. 켜는 길은 `confirm_finish`뿐이고 `Op`로는 올 수 없다.

use std::fs;
use std::path::Path;

use ieum_engine::{Node, ValidationError, Workflow, references};
use serde::Serialize;
use serde_json::{Value, json};

use super::edit::{AI_EVENT, FOLDER_FIELDS};
use super::{
    Builder, ChatState, Code, Doc, FinishAction, FinishAi, FinishButton, GithubTarget, HttpLine,
    HttpSend, NodeState, PendingFinish, Reply, SaveError, clear_slot_paths, node_state,
    workflow_hash,
};
use crate::github::GITHUB_WRITE_EVENTS;

/// 완료까지 남은 것 한 줄.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Remaining {
    #[serde(skip_serializing_if = "Option::is_none")]
    node_id: Option<String>,
    /// `UNTESTED` | `TEST_FAILED` | `INVALID` | `FOLDER_MISSING` | `AI_NOT_CONNECTED`
    reason: &'static str,
}

fn at(node: &Node, reason: &'static str) -> Remaining {
    Remaining {
        node_id: Some(node.id.clone()),
        reason,
    }
}

fn text<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.config
        .get(key)?
        .as_str()
        .filter(|s| !s.trim().is_empty())
}

const HTTP_EVENT: &str = "http.request";

/// 대문자, 없으면 GET.
fn http_method(n: &Node) -> String {
    text(n, "method").unwrap_or("GET").to_uppercase()
}

/// 원문(참조식 그대로).
fn http_url(n: &Node) -> String {
    n.config
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 칸 원문 — 참조식·AI가 JSON 숫자로 넣은 번호도 글로 보인다(노드가 숫자 번호를 받으므로).
fn raw(n: &Node, key: &str) -> String {
    match n.config.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(x)) => x.to_string(),
        _ => String::new(),
    }
}

fn is_github_write(n: &Node) -> bool {
    GITHUB_WRITE_EVENTS.contains(&n.event.as_str())
}

/// GitHub 쓰기 노드의 확인 줄. 값은 싣지 않고 config 전체의 참조식만 `sends`로 보인다.
fn github_line(wf: &Workflow, n: &Node) -> HttpLine {
    let summary = match n.event.as_str() {
        "github.create_issue"
        | "github.find_or_create_issue"
        | "github.create_pull_request"
        | "github.find_or_create_pull_request" => raw(n, "title"),
        "github.update_pull_request" | "github.request_reviewers" | "github.submit_pr_review" => {
            format!("#{}", raw(n, "pull_number"))
        }
        _ => format!("#{}", raw(n, "issue_number")),
    };
    let mut line = http_line_with(wf, n, n.config.values());
    line.github = Some(GithubTarget {
        event: n.event.clone(),
        repo: format!("{}/{}", raw(n, "owner"), raw(n, "repo")),
        summary,
    });
    line.has_body = !raw(n, "body").trim().is_empty();
    line
}

/// 확인 카드 한 줄. 머리말은 이름만, 참조식은 어느 노드의 어느 출력인지만 — 값은 싣지 않는다.
fn http_line(wf: &Workflow, n: &Node) -> HttpLine {
    let fields = ["url", "headers", "body"]
        .into_iter()
        .map(|k| n.config.get(k).unwrap_or(&Value::Null));
    let mut line = http_line_with(wf, n, fields);
    line.method = http_method(n);
    line.url = http_url(n);
    line.header_keys = n
        .config
        .get("headers")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    line.header_keys.sort();
    line.has_body = n.config.get("body").is_some_and(|b| !b.is_null());
    line
}

/// 줄의 공통 부분 — 칸들의 참조식을 등장 순서·중복 없이 `sends`로.
fn http_line_with<'a>(
    wf: &Workflow,
    n: &Node,
    fields: impl Iterator<Item = &'a Value>,
) -> HttpLine {
    let mut sends: Vec<HttpSend> = Vec::new();
    for (node_id, output) in fields.flat_map(references) {
        let node_label = wf
            .nodes
            .iter()
            .find(|m| m.id == node_id)
            .and_then(|m| m.label.clone())
            .unwrap_or_default();
        let send = HttpSend {
            node_id,
            node_label,
            output,
        };
        if !sends.contains(&send) {
            sends.push(send);
        }
    }
    HttpLine {
        node_id: n.id.clone(),
        label: n.label.clone().unwrap_or_default(),
        sends,
        ..Default::default()
    }
}

/// HTTP·GitHub 쓰기 노드 — 배열 순서로. 테스트 확인 카드가 쓴다.
pub(super) fn http_lines(wf: &Workflow) -> Vec<HttpLine> {
    wf.nodes
        .iter()
        .filter_map(|n| match n.event.as_str() {
            HTTP_EVENT => Some(http_line(wf, n)),
            _ if is_github_write(n) => Some(github_line(wf, n)),
            _ => None,
        })
        .collect()
}

/// 확인 카드에 보일 바깥 동작 — 파일·HTTP·GitHub 쓰기 노드만, 배열 순서로.
fn actions(wf: &Workflow) -> Vec<FinishAction> {
    wf.nodes
        .iter()
        .filter_map(|n| {
            let target = match n.event.as_str() {
                HTTP_EVENT => Some(format!("{} {}", http_method(n), http_url(n))),
                "local.file_move" => {
                    let base = text(n, "baseFolder").unwrap_or_default();
                    Some(match text(n, "destination") {
                        Some(dest) => format!("{base}/{dest}"),
                        None => base.to_string(),
                    })
                }
                "local.file_rename" => Some(text(n, "newName").unwrap_or_default().to_string()),
                "local.file_trash" => None,
                _ if is_github_write(n) => Some(format!("{}/{}", raw(n, "owner"), raw(n, "repo"))),
                _ => return None,
            };
            Some(FinishAction {
                node_id: n.id.clone(),
                event: n.event.clone(),
                label: n.label.clone().unwrap_or_default(),
                target,
                http: match n.event.as_str() {
                    HTTP_EVENT => Some(http_line(wf, n)),
                    _ if is_github_write(n) => Some(github_line(wf, n)),
                    _ => None,
                },
            })
        })
        .collect()
}

/// 확인 카드에 보일 AI 노드 — 배열 순서로.
fn ai_nodes(wf: &Workflow) -> Vec<FinishAi> {
    wf.nodes
        .iter()
        .filter(|n| n.event == AI_EVENT)
        .map(|n| FinishAi {
            node_id: n.id.clone(),
            label: n.label.clone().unwrap_or_default(),
            provider: text(n, "provider").unwrap_or_default().to_string(),
            send_content: n.config.get("sendContent").and_then(Value::as_bool) == Some(true),
        })
        .collect()
}

fn not_ready(remaining: Vec<Remaining>, errors: Vec<ValidationError>) -> Reply {
    Reply::err_with(
        Code::NotReady,
        json!({"remaining": remaining, "errors": errors}),
    )
}

impl Builder {
    /// 완료 조건. `finish`와 `confirm_finish`가 같이 쓴다. 비어 있으면 통과.
    async fn readiness(
        &self,
        doc: &Doc,
        state: &ChatState,
    ) -> (Vec<Remaining>, Vec<ValidationError>) {
        let nodes = &doc.workflow.nodes;
        let mut remaining = Vec::new();
        let errors = self.errors(&doc.workflow);
        if !errors.is_empty() {
            remaining.push(Remaining {
                node_id: None,
                reason: "INVALID",
            });
        }
        for n in nodes {
            match node_state(n, state) {
                NodeState::Untested => remaining.push(at(n, "UNTESTED")),
                NodeState::Failed => remaining.push(at(n, "TEST_FAILED")),
                _ => {}
            }
        }
        // 폴더가 정책 안이고 실제로 열리는가.
        for n in nodes {
            for (_, key) in FOLDER_FIELDS.iter().filter(|(event, _)| *event == n.event) {
                let Some(folder) = text(n, key).map(Path::new) else {
                    continue; // 빈 칸은 검증 에러(REQUIRED) 몫
                };
                if self.host.check_folder(folder).is_err() || fs::read_dir(folder).is_err() {
                    remaining.push(at(n, "FOLDER_MISSING"));
                }
            }
        }
        for n in nodes.iter().filter(|n| n.event == AI_EVENT) {
            let provider = text(n, "provider").unwrap_or_default();
            // OpenAI는 기본 모델이 없다.
            let no_model = provider == "api:openai" && text(n, "model").is_none();
            if no_model || !self.host.ai_ready(provider, text(n, "credential")).await {
                remaining.push(at(n, "AI_NOT_CONNECTED"));
            }
        }
        (remaining, errors)
    }

    /// 완료 요청 — 통과하면 확인 대기를 남기고 사람을 부른다. `enabled`는 건드리지 않는다.
    pub(super) async fn finish(
        &self,
        id: &str,
        doc: &Doc,
        state: &ChatState,
    ) -> Result<Value, Reply> {
        let (remaining, errors) = self.readiness(doc, state).await;
        if !remaining.is_empty() {
            return Err(not_ready(remaining, errors));
        }
        let wf = &doc.workflow;
        let folder = wf
            .nodes
            .iter()
            .filter(|n| n.event == "local.folder_new_file")
            .find_map(|n| text(n, "folder"));
        let manual = wf.nodes.iter().any(|n| n.event == "manual.run");
        let pending = PendingFinish {
            hash: workflow_hash(wf),
            folder: folder.map(str::to_string),
            button: match manual {
                true => FinishButton::Save,
                false => FinishButton::Enable,
            },
            actions: actions(wf),
            ai: ai_nodes(wf),
        };
        self.update_chat(id, |s| s.pending_finish = Some(pending.clone()))?;
        self.host.finish_pending(id, &doc.name);
        Ok(json!({"status": "awaiting_user"}))
    }

    /// 저장본과 hash가 같은 확인 대기. 달라졌으면 None — 지우지는 않는다(읽기만).
    /// 잠금 없이 불리는 `pending()`이 두 읽기 사이에 생긴 새 대기를 지우지 않게. 낡은 대기는 commit·confirm이 지운다.
    pub(super) fn pending_of(&self, id: &str, doc: &Doc) -> Option<PendingFinish> {
        let pending = self.host.load_chat(id).ok()?.state.pending_finish?;
        (pending.hash == workflow_hash(&doc.workflow)).then_some(pending)
    }

    /// 사람의 [켜기]/[아직]. 잠금은 호출자(`confirm_finish`)가 잡는다.
    pub(super) async fn confirm(&self, id: &str, accept: bool) -> Result<(), Reply> {
        if !accept {
            self.update_chat(id, |s| s.pending_finish = None)?;
            return Ok(());
        }
        let mut doc = self.load(id)?;
        let waiting = self.pending_of(id, &doc).is_some();
        let state = self.update_chat(id, |s| s.pending_finish = None)?;
        // 대기 뒤에 바뀌었거나 그사이 조건이 깨졌으면 켜지 않는다 — 다시 finish해야 한다.
        let (remaining, errors) = self.readiness(&doc, &state).await;
        if !waiting || !remaining.is_empty() {
            return Err(not_ready(remaining, errors));
        }
        doc.enabled = true;
        self.host.save(id, doc).map_err(|e| match e {
            SaveError::Stale => Code::Stale,
            SaveError::Rejected(_) => Code::SaveRejected,
        })?;
        // 다 만들었다 — 쓰지 않고 남은 슬롯 경로를 들고 있지 않는다. 켜기는 이미 저장됐으니
        // 이 갱신이 실패해도 확인 결과는 바꾸지 않는다.
        if state.blueprint.is_some() {
            let _ = self.update_chat(id, |s| {
                if let Some(b) = s.blueprint.as_mut() {
                    clear_slot_paths(b);
                }
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_lines_uppercase_method_and_default_get() {
        let wf: Workflow = serde_json::from_value(json!({
            "nodes": [
                {"id": "a", "event": "http.request", "config": {"method": "post", "url": "https://x/{{nodes.t.output.name}}"}},
                {"id": "f", "event": "formatter.text_trim", "config": {}},
                {"id": "b", "event": "http.request", "label": "조회", "config": {}}
            ],
            "edges": []
        }))
        .unwrap();
        let lines = http_lines(&wf);
        let got: Vec<_> = lines
            .iter()
            .map(|l| {
                (
                    l.node_id.as_str(),
                    l.label.as_str(),
                    l.method.as_str(),
                    l.url.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("a", "", "POST", "https://x/{{nodes.t.output.name}}"),
                ("b", "조회", "GET", "")
            ]
        );
    }

    #[test]
    fn github_write_nodes_become_confirm_lines() {
        let wf: Workflow = serde_json::from_value(json!({
            "nodes": [
                {"id": "c", "event": "github.create_issue", "config": {"owner": "a", "repo": "b", "title": "{{nodes.t.output.name}}", "body": "x"}},
                {"id": "r", "event": "github.find_issue", "config": {"owner": "a", "repo": "b", "title": "x"}},
                {"id": "m", "event": "github.add_labels", "config": {"owner": "a", "repo": "b", "issue_number": "{{nodes.t.output.no}}", "labels": "bug"}},
                {"id": "u", "event": "github.update_issue", "config": {"owner": "a", "repo": "b", "issue_number": 12, "state": "closed"}},
                {"id": "p", "event": "github.create_pull_request", "config": {"owner": "a", "repo": "b", "title": "T", "head": "h", "base": "main"}},
                {"id": "s", "event": "github.submit_pr_review", "config": {"owner": "a", "repo": "b", "pull_number": 7}},
                {"id": "f", "event": "github.find_pull_request", "config": {"owner": "a", "repo": "b"}},
                {"id": "g", "event": "github.get_pr_files", "config": {"owner": "a", "repo": "b", "pull_number": 7}}
            ],
            "edges": []
        }))
        .unwrap();
        let lines = http_lines(&wf);
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[3].github.as_ref().unwrap().summary, "T");
        assert_eq!(lines[4].github.as_ref().unwrap().summary, "#7");
        let c = &lines[0];
        assert_eq!(
            c.github,
            Some(GithubTarget {
                event: "github.create_issue".into(),
                repo: "a/b".into(),
                summary: "{{nodes.t.output.name}}".into()
            })
        );
        assert_eq!(
            c.sends,
            [HttpSend {
                node_id: "t".into(),
                node_label: "".into(),
                output: "name".into()
            }]
        );
        assert!(c.has_body && c.method.is_empty() && c.url.is_empty() && c.header_keys.is_empty());
        let m = &lines[1];
        assert_eq!(m.github.as_ref().unwrap().summary, "#{{nodes.t.output.no}}");
        assert!(!m.has_body);
        // 참조식·AI가 번호를 JSON 숫자로 넣어도 어느 이슈인지 보인다.
        let u = lines[2].github.as_ref().unwrap();
        assert_eq!(
            (u.event.as_str(), u.summary.as_str()),
            ("github.update_issue", "#12")
        );
        let acts = actions(&wf);
        assert_eq!(acts.len(), 5);
        assert_eq!(acts[0].target.as_deref(), Some("a/b"));
        assert!(acts[0].http.is_some());
    }
}
