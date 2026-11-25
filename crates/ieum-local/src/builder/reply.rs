use serde::{Deserialize, Serialize};
use serde_json::Value;

/// AI에게 가는 오류 코드. 문구는 코드별 고정(경로·파일 이름이 섞이지 않는다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Code {
    BadRequest,
    Unauthorized,
    Internal,
    Stale,
    UserEditing,
    NodeNotFound,
    EventUnknown,
    TriggerRule,
    FieldUnknown,
    FieldLocked,
    NotFolderField,
    PickCancelled,
    FolderNotAllowed,
    EdgeInvalid,
    SaveRejected,
    Invalid,
    SampleNotAllowed,
    NotReady,
    NoSampleFile,
    FolderMissing,
    FileMissing,
    DestOccupied,
    FieldEmpty,
    FieldInvalid,
    MoveFailed,
    AiNotConnected,
    AiFailed,
    AiModelListFailed,
    AiKeyLimit,
    NodeFailed,
    UndoFailed,
    TooLarge,
    HttpStatus,
    HttpUnreachable,
    TestDeclined,
    GithubInput,
    GithubNotConnected,
    GithubAuth,
    GithubUnreachable,
    GithubRateLimit,
    GithubForbidden,
    GithubNotFound,
    GithubMoved,
    GithubGone,
    GithubInvalid,
    GithubStatus,
    GithubTooLarge,
    UiInvalid,
    UiNoSurface,
    SlotUnknown,
    SlotEmpty,
    SlotMismatch,
    BlueprintNotSubmitted,
}

impl Code {
    pub fn message(self) -> &'static str {
        use Code::*;
        match self {
            BadRequest => "명령이나 인자가 올바르지 않음",
            Unauthorized => "이 세션에서는 쓸 수 없음. IEUM 앱의 터미널에서 실행할 것",
            Internal => "앱 내부 오류. 사용자에게 앱 화면을 확인해 달라고 할 것",
            Stale => "워크플로우가 방금 바뀌었음. status로 다시 본 뒤 같은 명령을 다시 실행할 것",
            UserEditing => "사용자가 캔버스에서 고치는 중입니다. 저장한 뒤 다시 시도하세요.",
            NodeNotFound => "그 id의 노드가 없음",
            EventUnknown => "쓸 수 없는 노드 종류. events로 목록을 볼 것",
            TriggerRule => "첫 노드는 트리거 하나여야 해요(폴더에 새 파일·스케줄·수동 실행).",
            FieldUnknown => "그 노드에 없는 칸. fields로 칸을 볼 것",
            FieldLocked => {
                "set으로 채울 수 없는 칸. 폴더 칸은 pick-folder, AI 연결 칸은 사용자가 캔버스에서 고름"
            }
            NotFolderField => "폴더 칸이 아님. pick-folder는 폴더 칸에만 씀",
            PickCancelled => "사용자가 폴더를 고르지 않음",
            FolderNotAllowed => {
                "쓸 수 없는 폴더나 경로. 홈 아래 일반 폴더만 되고 기준 폴더 밖으로 나갈 수 없음"
            }
            EdgeInvalid => {
                "연결할 수 없음. 없는 연결이거나 자기 자신·트리거로 들어가는 연결·순환·갈래 값 오류"
            }
            SaveRejected => "저장 검사에서 거부됨",
            Invalid => "검증 에러가 남아 있음. errors를 볼 것",
            SampleNotAllowed => "테스트 파일은 감시 폴더 바로 아래의 일반 파일이어야 함",
            NotReady => "아직 완료할 수 없음. remaining을 볼 것",
            NoSampleFile => {
                "감시 폴더에 테스트할 파일이 없음. 사용자에게 파일을 그 폴더에 넣어 달라고 할 것"
            }
            FolderMissing => "폴더가 없거나 열 수 없음",
            FileMissing => "옮길 파일이 없음",
            DestOccupied => "목적지에 같은 이름의 파일이 너무 많음",
            FieldEmpty => "필수 칸이 비어 있음",
            FieldInvalid => "칸 값의 모양이 맞지 않음",
            MoveFailed => "파일을 옮기지 못함",
            AiNotConnected => "AI 연결(키·계정)이 없음. 사용자가 캔버스에서 골라야 함",
            AiFailed => "AI 호출이 실패함",
            AiModelListFailed => {
                "Antigravity 모델 목록을 읽지 못함. 사용자가 agy 로그인·설치 상태를 확인해야 함"
            }
            AiKeyLimit => {
                "API 키가 이번 달 한도를 넘어 호출하지 않음. 사용자가 설정 › AI에서 한도를 올리거나 다른 키를 골라야 함"
            }
            NodeFailed => "노드 실행이 실패함. 자세한 내용은 사용자 화면에만 보임",
            UndoFailed => "테스트가 옮긴 파일 일부를 되돌리지 못함. 경로는 사용자 화면에 표시됨",
            TooLarge => "입력이나 결과가 허용 크기를 넘음",
            HttpStatus => "요청은 갔지만 서버가 오류 상태를 돌려줌. field가 상태 코드",
            HttpUnreachable => "요청을 보내지 못함(연결·시간 초과·리다이렉트)",
            TestDeclined => {
                "사용자가 확인하지 않아 테스트를 돌리지 않음. 다시 시도하지 말고 사용자에게 알릴 것"
            }
            GithubInput => {
                "GitHub 노드 칸 값이 올바르지 않음. 노드 줄의 message가 어느 칸인지 알려 줌"
            }
            GithubNotConnected => {
                "GitHub 연결이 없거나 gh 계정이 연결한 계정과 다름. 사용자가 설정 > 연동에서 확인해야 함"
            }
            GithubAuth => "GitHub 로그인이 만료됨. 사용자가 설정 > 연동에서 다시 연결해야 함",
            GithubUnreachable => {
                "GitHub에 닿지 못함(연결·시간 초과·서버 오류). 잠시 뒤 다시 시도할 것"
            }
            GithubRateLimit => "GitHub 요청 한도에 걸림. 잠시 뒤 다시 시도할 것",
            GithubForbidden => {
                "이 저장소·조직을 읽거나 쓸 권한이 없음. owner/repo·org와 사용자의 권한을 확인할 것"
            }
            GithubNotFound => {
                "저장소나 이슈가 없음(또는 볼 권한이 없음). owner/repo·이슈 번호를 확인할 것"
            }
            GithubMoved => "저장소 주소가 바뀌었음. owner/repo를 확인할 것",
            GithubGone => "이 저장소는 이슈를 쓰지 않음",
            GithubInvalid => {
                "GitHub가 칸 값을 받지 않음(422). 라벨·담당자·마일스톤 등 칸 값을 확인할 것"
            }
            GithubStatus => "GitHub가 요청을 거절함. field가 상태 코드",
            GithubTooLarge => "GitHub 응답이 허용 크기를 넘음",
            UiInvalid => "설계서 메시지가 규칙에 맞지 않음. rule·at을 보고 고쳐 다시 낼 것",
            UiNoSurface => "고칠 설계서가 없음. createSurface로 새로 띄울 것",
            SlotUnknown => "설계서에 그 이름의 앱 전용 칸이 없음",
            SlotEmpty => "앱 전용 칸이 비어 있음(사용자가 아직 고르지 않았거나 이미 씀)",
            SlotMismatch => "앱 전용 칸의 대상(노드 종류·칸)이 이 노드와 다름",
            BlueprintNotSubmitted => {
                "제출된 설계서가 없음. 사용자가 [이대로 만들기]를 누른 뒤에만 쓸 수 있음"
            }
        }
    }

    /// `ieum` CLI 종료 코드: 1 실패 / 2 검증·거부 / 3 사용법.
    pub fn exit_code(self) -> i32 {
        use Code::*;
        match self {
            BadRequest | Unauthorized => 3,
            Internal | NoSampleFile | FolderMissing | FileMissing | DestOccupied | FieldEmpty
            | FieldInvalid | MoveFailed | AiNotConnected | AiFailed | AiModelListFailed
            | AiKeyLimit | NodeFailed | UndoFailed | TooLarge | HttpStatus | HttpUnreachable
            | GithubInput | GithubNotConnected | GithubAuth | GithubUnreachable
            | GithubRateLimit | GithubForbidden | GithubNotFound | GithubMoved | GithubGone
            | GithubInvalid | GithubStatus | GithubTooLarge => 1,
            Stale | UserEditing | NodeNotFound | EventUnknown | TriggerRule | FieldUnknown
            | FieldLocked | NotFolderField | PickCancelled | FolderNotAllowed | EdgeInvalid
            | SaveRejected | Invalid | SampleNotAllowed | NotReady | TestDeclined => 2,
            UiInvalid
            | UiNoSurface
            | SlotUnknown
            | SlotEmpty
            | SlotMismatch
            | BlueprintNotSubmitted => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<Code>,
    /// 항상 `code.message()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl Reply {
    pub fn ok(data: impl Serialize) -> Reply {
        Reply {
            ok: true,
            code: None,
            message: None,
            data: serde_json::to_value(data).unwrap_or(Value::Null),
        }
    }

    pub fn err(code: Code) -> Reply {
        Reply::err_with(code, ())
    }

    pub fn err_with(code: Code, data: impl Serialize) -> Reply {
        Reply {
            ok: false,
            code: Some(code),
            message: Some(code.message().into()),
            data: serde_json::to_value(data).unwrap_or(Value::Null),
        }
    }

    /// ok → 0, 아니면 `code.exit_code()`.
    pub fn exit_code(&self) -> i32 {
        match (self.ok, self.code) {
            (true, _) => 0,
            (false, Some(c)) => c.exit_code(),
            (false, None) => 1,
        }
    }

    /// 줄바꿈 없는 JSON 한 줄.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("Reply는 항상 직렬화된다")
    }
}
