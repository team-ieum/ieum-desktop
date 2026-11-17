use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// 데스크톱이 러너를 가진 GitHub 이슈 노드. 카탈로그엔 이 밖에도 있지만 싣지 않는다.
pub const GITHUB_EVENTS: [&str; 7] = [
    "github.create_issue",
    "github.update_issue",
    "github.find_issue",
    "github.find_or_create_issue",
    "github.list_issues",
    "github.add_labels",
    "github.create_comment",
];

const GITHUB_INPUTS: &str = include_str!("../../ieum-local/github_inputs.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Trigger,
    Ai,
    Action,
    Condition,
    Transform,
    Http,
}

#[derive(Debug, Clone)]
pub struct EventDef {
    pub event: String,
    pub node_type: NodeType,
    /// 정의 JSON의 `title`·`description`(사람이 읽는 이름과 설명).
    pub title: String,
    pub description: String,
    pub output_keys: Option<BTreeSet<String>>,
    pub output_dynamic: bool,
    pub outputs_from: Option<String>,
    pub input_fields: Vec<InputField>,
}

/// 칸의 선택지 하나.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// 입력 칸 정의 중 정적 검증·채팅 빌더가 보는 부분.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputField {
    pub key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
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
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    output_fields: Option<Vec<RawField>>,
    #[serde(default)]
    output_dynamic: bool,
    #[serde(default)]
    outputs_from: Option<String>,
    #[serde(default)]
    input_fields: Vec<InputField>,
    /// 카탈로그의 칸 설명(제목·선택지 등). 필수·기본값은 없어 데스크톱 표와 합친다.
    #[serde(default)]
    fields: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
struct RawField {
    key: String,
}

impl Catalog {
    pub fn load(files: &[(String, PathBuf)]) -> Result<Catalog, CatalogError> {
        Self::load_with(files, GITHUB_INPUTS)
    }

    fn load_with(files: &[(String, PathBuf)], table: &str) -> Result<Catalog, CatalogError> {
        let mut sources = Vec::new();
        for (event, path) in files {
            let text = std::fs::read_to_string(path).map_err(|source| CatalogError::Io {
                path: path.clone(),
                source,
            })?;
            sources.push((event.clone(), path.clone(), text));
        }
        Self::parse(sources, &github_table(table)?)
    }

    /// 실행 파일에 묶은 노드 정의(`stage1`과 같은 파일들).
    /// 배포된 CLI·앱은 이걸 쓴다. `stage1`(디스크)은 테스트·개발용.
    pub fn embedded() -> Result<Catalog, CatalogError> {
        macro_rules! src {
            ($event:expr, $rel:expr) => {
                (
                    $event.to_string(),
                    PathBuf::from($rel),
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../", $rel)).to_string(),
                )
            };
        }
        let mut sources = vec![
            src!("branch.if", "catalog/events/branch/if.json"),
            src!("local.ai", "crates/ieum-local/defs/local.ai.json"),
            src!(
                "local.file_move",
                "crates/ieum-local/defs/local.file_move.json"
            ),
            src!(
                "local.file_rename",
                "crates/ieum-local/defs/local.file_rename.json"
            ),
            src!(
                "local.file_trash",
                "crates/ieum-local/defs/local.file_trash.json"
            ),
            src!(
                "local.folder_new_file",
                "crates/ieum-local/defs/local.folder_new_file.json"
            ),
            src!("schedule.every", "catalog/events/schedule/every.json"),
            src!("schedule.hourly", "catalog/events/schedule/hourly.json"),
            src!("schedule.daily", "catalog/events/schedule/daily.json"),
            src!("schedule.weekly", "catalog/events/schedule/weekly.json"),
            src!("schedule.monthly", "catalog/events/schedule/monthly.json"),
            src!("manual.run", "catalog/events/manual/run.json"),
            src!("transform.map", "catalog/events/transform/map.json"),
            src!("http.request", "catalog/events/http/request.json"),
            src!(
                "formatter.date_add",
                "catalog/events/formatter/date_add.json"
            ),
            src!(
                "formatter.date_compare",
                "catalog/events/formatter/date_compare.json"
            ),
            src!(
                "formatter.date_format",
                "catalog/events/formatter/date_format.json"
            ),
            src!(
                "formatter.list_pick",
                "catalog/events/formatter/list_pick.json"
            ),
            src!(
                "formatter.list_to_text",
                "catalog/events/formatter/list_to_text.json"
            ),
            src!(
                "formatter.lookup_table",
                "catalog/events/formatter/lookup_table.json"
            ),
            src!(
                "formatter.number_currency",
                "catalog/events/formatter/number_currency.json"
            ),
            src!(
                "formatter.number_format",
                "catalog/events/formatter/number_format.json"
            ),
            src!(
                "formatter.number_math",
                "catalog/events/formatter/number_math.json"
            ),
            src!(
                "formatter.number_phone",
                "catalog/events/formatter/number_phone.json"
            ),
            src!(
                "formatter.text_case",
                "catalog/events/formatter/text_case.json"
            ),
            src!(
                "formatter.text_default",
                "catalog/events/formatter/text_default.json"
            ),
            src!(
                "formatter.text_extract",
                "catalog/events/formatter/text_extract.json"
            ),
            src!(
                "formatter.text_extract_pattern",
                "catalog/events/formatter/text_extract_pattern.json"
            ),
            src!(
                "formatter.text_length",
                "catalog/events/formatter/text_length.json"
            ),
            src!(
                "formatter.text_markdown_to_html",
                "catalog/events/formatter/text_markdown_to_html.json"
            ),
            src!(
                "formatter.text_replace",
                "catalog/events/formatter/text_replace.json"
            ),
            src!(
                "formatter.text_split",
                "catalog/events/formatter/text_split.json"
            ),
            src!(
                "formatter.text_strip_html",
                "catalog/events/formatter/text_strip_html.json"
            ),
            src!(
                "formatter.text_trim",
                "catalog/events/formatter/text_trim.json"
            ),
            src!(
                "formatter.text_truncate",
                "catalog/events/formatter/text_truncate.json"
            ),
            src!(
                "formatter.text_url_encode",
                "catalog/events/formatter/text_url_encode.json"
            ),
        ];
        macro_rules! github {
            ($($name:literal),*) => {
                $(sources.push(src!(
                    concat!("github.", $name),
                    concat!("catalog/events/github/", $name, ".json")
                ));)*
            };
        }
        github!(
            "create_issue",
            "update_issue",
            "find_issue",
            "find_or_create_issue",
            "list_issues",
            "add_labels",
            "create_comment"
        );
        Self::parse(sources, &github_table(GITHUB_INPUTS)?)
    }

    /// (event, 에러에 쓸 경로, JSON 본문).
    fn parse(
        sources: Vec<(String, PathBuf, String)>,
        inputs: &Map<String, Value>,
    ) -> Result<Catalog, CatalogError> {
        let mut events = HashMap::new();
        for (event, path, text) in &sources {
            let raw: RawEvent =
                serde_json::from_str(text).map_err(|source| CatalogError::Json {
                    path: path.clone(),
                    source,
                })?;
            let node_type = match raw.node_type.as_str() {
                "TRIGGER" => NodeType::Trigger,
                "AI" => NodeType::Ai,
                "ACTION" => NodeType::Action,
                "CONDITION" => NodeType::Condition,
                "TRANSFORM" => NodeType::Transform,
                "HTTP" => NodeType::Http,
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
            let input_fields = if raw.input_fields.is_empty() {
                merge_fields(path, inputs.get(event), raw.fields.as_ref())?
            } else {
                raw.input_fields
            };
            events.insert(
                event.clone(),
                EventDef {
                    event: event.clone(),
                    node_type,
                    title: raw.title,
                    description: raw.description,
                    output_keys,
                    output_dynamic: raw.output_dynamic,
                    outputs_from: raw.outputs_from,
                    input_fields,
                },
            );
        }
        Ok(Catalog { events })
    }

    pub fn get(&self, event: &str) -> Option<&EventDef> {
        self.events.get(event)
    }

    /// 모든 event — 트리거 먼저, 그다음 event 이름순.
    pub fn events(&self) -> Vec<&EventDef> {
        let mut all: Vec<&EventDef> = self.events.values().collect();
        all.sort_by_key(|e| (e.node_type != NodeType::Trigger, &e.event));
        all
    }

    /// `catalog/events/branch/if.json` + `crates/ieum-local/defs/*.json`(파일 이름 = event)
    /// + `catalog/events/{transform,formatter,http,schedule,manual}/*.json`(event = `<폴더>.<파일 이름>`).
    pub fn stage1(repo_root: &Path) -> Result<Catalog, CatalogError> {
        let mut files = vec![(
            "branch.if".to_string(),
            repo_root.join("catalog/events/branch/if.json"),
        )];
        json_files(&repo_root.join("crates/ieum-local/defs"), "", &mut files)?;
        for dir in ["transform", "formatter", "http", "schedule", "manual"] {
            let prefix = format!("{dir}.");
            json_files(
                &repo_root.join("catalog/events").join(dir),
                &prefix,
                &mut files,
            )?;
        }
        for event in GITHUB_EVENTS {
            let name = event.trim_start_matches("github.");
            files.push((
                event.to_string(),
                repo_root.join(format!("catalog/events/github/{name}.json")),
            ));
        }
        let table_path = repo_root.join("crates/ieum-local/github_inputs.json");
        let table = std::fs::read_to_string(&table_path).map_err(|source| CatalogError::Io {
            path: table_path.clone(),
            source,
        })?;
        Self::load_with(&files, &table)
    }
}

fn github_table(text: &str) -> Result<Map<String, Value>, CatalogError> {
    serde_json::from_str(text).map_err(|source| CatalogError::Json {
        path: PathBuf::from("crates/ieum-local/github_inputs.json"),
        source,
    })
}

/// 칸 하나 = `{key, type: string}` ← 데스크톱 표 항목 ← 카탈로그 `fields[key]`(뒤가 앞을 덮는다).
fn merge_fields(
    path: &Path,
    rows: Option<&Value>,
    fields: Option<&Map<String, Value>>,
) -> Result<Vec<InputField>, CatalogError> {
    let Some(rows) = rows.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    rows.iter()
        .map(|row| {
            let mut merged = json!({"type": "string"}).as_object().cloned().unwrap();
            if let Some(row) = row.as_object() {
                merged.extend(row.clone());
            }
            let key = merged.get("key").and_then(Value::as_str).unwrap_or("");
            if let Some(over) = fields.and_then(|f| f.get(key)).and_then(Value::as_object) {
                merged.extend(over.clone());
            }
            serde_json::from_value(Value::Object(merged)).map_err(|source| CatalogError::Json {
                path: path.to_path_buf(),
                source,
            })
        })
        .collect()
}

fn json_files(
    dir: &Path,
    prefix: &str,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), CatalogError> {
    let io = |source| CatalogError::Io {
        path: dir.to_path_buf(),
        source,
    };
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.extension().is_some_and(|e| e == "json") {
            let stem = path.file_stem().unwrap().to_string_lossy();
            files.push((format!("{prefix}{stem}"), path));
        }
    }
    Ok(())
}
