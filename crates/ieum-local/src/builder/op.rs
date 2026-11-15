use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Code, Reply};

/// AI가 닿는 유일한 길. 켜는 동작은 없다(켜기는 `Builder::confirm_finish`뿐).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "args",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Op {
    Status {},
    Events {},
    Finish {},
    Guide {},
    Outline {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default)]
        steps: Vec<String>,
    },
    Add {
        event: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    Fields {
        id: String,
    },
    Set {
        id: String,
        values: Map<String, Value>,
    },
    Connect {
        from: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    Disconnect {
        from: String,
        to: String,
    },
    Remove {
        id: String,
    },
    Rename {
        id: String,
        label: String,
    },
    PickFolder {
        id: String,
        field: String,
    },
    Test {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
}

impl Op {
    /// `{"op":..,"args":{..}}`. `args`가 없으면 `{}`로 본다. 모르는 op·모양 오류는 `Reply::err(Code::BadRequest)`.
    pub fn from_value(mut v: Value) -> Result<Op, Reply> {
        if let Some(o) = v.as_object_mut() {
            o.entry("args").or_insert_with(|| Value::Object(Map::new()));
        }
        serde_json::from_value(v).map_err(|_| Reply::err(Code::BadRequest))
    }

    /// 저장본을 고치는 동작인가(outline·add·set·connect·disconnect·remove·rename·pick-folder).
    pub fn mutates(&self) -> bool {
        matches!(
            self,
            Op::Outline { .. }
                | Op::Add { .. }
                | Op::Set { .. }
                | Op::Connect { .. }
                | Op::Disconnect { .. }
                | Op::Remove { .. }
                | Op::Rename { .. }
                | Op::PickFolder { .. }
        )
    }
}
