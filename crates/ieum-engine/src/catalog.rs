use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Trigger,
    Ai,
    Action,
    Condition,
}

#[derive(Debug, Clone)]
pub struct EventDef {
    pub event: String,
    pub node_type: NodeType,
    pub output_keys: Option<BTreeSet<String>>,
    pub output_dynamic: bool,
    pub outputs_from: Option<String>,
    pub input_fields: Vec<InputField>,
}

/// 입력 칸 정의 중 정적 검증이 보는 부분.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputField {
    pub key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// `false`면 참조식을 넣을 수 없다. 없으면 허용.
    #[serde(default, rename = "ref")]
    pub ref_allowed: Option<bool>,
    #[serde(default)]
    pub list: bool,
    #[serde(default)]
    pub fields_source: Option<String>,
    #[serde(default)]
    pub children: Vec<InputField>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("{path}: 읽기 실패: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: JSON 형식 오류: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{event}: 지원하지 않는 node_type `{node_type}`")]
    UnknownNodeType { event: String, node_type: String },
}

#[derive(Debug, Default)]
pub struct Catalog {
    events: HashMap<String, EventDef>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEvent {
    #[serde(rename = "node_type")]
    node_type: String,
    #[serde(default)]
    output_fields: Option<Vec<RawField>>,
    #[serde(default)]
    output_dynamic: bool,
    #[serde(default)]
    outputs_from: Option<String>,
    #[serde(default)]
    input_fields: Vec<InputField>,
}

#[derive(Deserialize)]
struct RawField {
    key: String,
}

impl Catalog {
    pub fn load(files: &[(String, PathBuf)]) -> Result<Catalog, CatalogError> {
        let mut events = HashMap::new();
        for (event, path) in files {
            let text = std::fs::read_to_string(path).map_err(|source| CatalogError::Io {
                path: path.clone(),
                source,
            })?;
            let raw: RawEvent =
                serde_json::from_str(&text).map_err(|source| CatalogError::Json {
                    path: path.clone(),
                    source,
                })?;
            let node_type = match raw.node_type.as_str() {
                "TRIGGER" => NodeType::Trigger,
                "AI" => NodeType::Ai,
                "ACTION" => NodeType::Action,
                "CONDITION" => NodeType::Condition,
                other => {
                    return Err(CatalogError::UnknownNodeType {
                        event: event.clone(),
                        node_type: other.to_string(),
                    });
                }
            };
            let mut output_keys: Option<BTreeSet<String>> = raw
                .output_fields
                .map(|fields| fields.into_iter().map(|f| f.key).collect());
            // SCHEMA.md: AI 항목은 공통 출력(output)이 앞에 붙는다.
            if node_type == NodeType::Ai {
                output_keys
                    .get_or_insert_with(BTreeSet::new)
                    .insert("output".to_string());
            }
            events.insert(
                event.clone(),
                EventDef {
                    event: event.clone(),
                    node_type,
                    output_keys,
                    output_dynamic: raw.output_dynamic,
                    outputs_from: raw.outputs_from,
                    input_fields: raw.input_fields,
                },
            );
        }
        Ok(Catalog { events })
    }

    pub fn get(&self, event: &str) -> Option<&EventDef> {
        self.events.get(event)
    }

    /// `catalog/events/branch/if.json` + `crates/ieum-local/defs/*.json`(파일 이름 = event).
    pub fn stage1(repo_root: &Path) -> Result<Catalog, CatalogError> {
        let mut files = vec![(
            "branch.if".to_string(),
            repo_root.join("catalog/events/branch/if.json"),
        )];
        let defs = repo_root.join("crates/ieum-local/defs");
        let entries = std::fs::read_dir(&defs).map_err(|source| CatalogError::Io {
            path: defs.clone(),
            source,
        })?;
        for entry in entries {
            let path = entry
                .map_err(|source| CatalogError::Io {
                    path: defs.clone(),
                    source,
                })?
                .path();
            if path.extension().is_some_and(|e| e == "json") {
                let event = path.file_stem().unwrap().to_string_lossy().into_owned();
                files.push((event, path));
            }
        }
        Catalog::load(&files)
    }
}
