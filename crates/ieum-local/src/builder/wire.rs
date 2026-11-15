use serde_json::{Map, Value};

use super::{Code, Op, Reply};

/// 소켓 한 줄 JSON 요청 `{"token":"…","op":"…","args":{…}}`. 응답은 `Reply::to_line()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub token: String,
    pub op: Op,
}

impl Request {
    /// JSON이 아니면 BadRequest, token이 없거나 글자가 아니면 Unauthorized, 나머지는 `Op::from_value`.
    pub fn parse(line: &str) -> Result<Request, Reply> {
        let mut v: Value = serde_json::from_str(line).map_err(|_| Reply::err(Code::BadRequest))?;
        let token = match v.as_object_mut().map(|o| o.remove("token")) {
            None => return Err(Reply::err(Code::BadRequest)),
            Some(Some(Value::String(t))) => t,
            Some(_) => return Err(Reply::err(Code::Unauthorized)),
        };
        Ok(Request {
            token,
            op: Op::from_value(v)?,
        })
    }

    pub fn to_line(&self) -> String {
        let mut o = Map::new();
        o.insert("token".into(), Value::String(self.token.clone()));
        if let Ok(Value::Object(op)) = serde_json::to_value(&self.op) {
            o.extend(op);
        }
        Value::Object(o).to_string()
    }
}
