//! `ieum` 명령이 닿는 유닉스 소켓. 연결 하나 = 요청 한 줄 → 응답 한 줄.
//! 토큰이 계정과 워크플로우를 정한다 — 요청에 실린 id는 보지 않는다.
//! 같은 워크플로우의 줄 세우기는 `Builder::apply`가 한다. 여기는 연결을 동시에 받기만 한다.

use std::collections::HashMap;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ieum_local::builder::{Code, Op, Reply, Request};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const ENV_SOCKET: &str = "IEUM_SOCKET";
pub const ENV_TOKEN: &str = "IEUM_TOKEN";
/// 요청 한 줄 상한(바이트).
pub const MAX_LINE: usize = 1 << 20;
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// macOS `sun_path`는 104바이트(끝 NUL 포함).
const MAX_SOCKET_PATH: usize = 103;
const SOCKET_FILE: &str = "ieum.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub account: String,
    pub workflow_id: String,
}

pub type Handler =
    Arc<dyn Fn(Session, Op) -> Pin<Box<dyn Future<Output = Reply> + Send>> + Send + Sync>;

/// `root/ieum.sock`. 103바이트를 넘으면 `tmp/ieum-<account>/ieum.sock`.
pub fn socket_path(account_root: &Path, tmp: &Path, account: &str) -> PathBuf {
    let path = account_root.join(SOCKET_FILE);
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        tmp.join(format!("ieum-{account}")).join(SOCKET_FILE)
    } else {
        path
    }
}

struct Open {
    account: String,
    path: PathBuf,
    /// 토큰 → 워크플로우 id.
    tokens: HashMap<String, String>,
    cancel: CancellationToken,
}

type State = Arc<Mutex<Option<Open>>>;

fn lock(state: &State) -> MutexGuard<'_, Option<Open>> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

fn close_open(open: Option<Open>) {
    if let Some(o) = open {
        o.cancel.cancel();
        let _ = std::fs::remove_file(&o.path);
    }
}

#[derive(Clone)]
pub struct Channel {
    handler: Handler,
    tmp: PathBuf,
    state: State,
}

impl Channel {
    /// `tmp` = 계정 폴더 경로가 너무 길 때 소켓을 둘 곳(앱은 `std::env::temp_dir()`).
    pub fn new(handler: Handler, tmp: PathBuf) -> Channel {
        Channel {
            handler,
            tmp,
            state: Arc::default(),
        }
    }

    /// 그 계정의 소켓을 연다. 같은 계정으로 이미 열려 있으면 그대로, 다른 계정이면 `close` 뒤 새로 연다.
    pub fn open(&self, account: &str, account_root: &Path) -> Result<PathBuf, String> {
        let mut state = lock(&self.state);
        if let Some(o) = state.as_ref()
            && o.account == account
        {
            return Ok(o.path.clone());
        }
        close_open(state.take());
        let path = socket_path(account_root, &self.tmp, account);
        let dir = path.parent().ok_or("소켓 폴더가 없어요")?;
        let io = |e: std::io::Error| format!("소켓을 열지 못했어요: {e}");
        std::fs::create_dir_all(dir).map_err(io)?;
        // 임시 폴더 쪽은 모두가 쓰는 /tmp일 수 있다 — 남의 폴더면 권한을 못 바꿔 여기서 멈춘다.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
            _ => {}
        }
        let listener = std::os::unix::net::UnixListener::bind(&path).map_err(io)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
        listener.set_nonblocking(true).map_err(io)?;
        let cancel = CancellationToken::new();
        tauri::async_runtime::spawn(accept(
            listener,
            cancel.clone(),
            self.handler.clone(),
            self.state.clone(),
        ));
        *state = Some(Open {
            account: account.to_string(),
            path: path.clone(),
            tokens: HashMap::new(),
            cancel,
        });
        Ok(path)
    }

    /// 열린 계정에 묶인 새 토큰(64자 hex). 열려 있지 않으면 Err.
    pub fn issue(&self, workflow_id: &str) -> Result<String, String> {
        let mut state = lock(&self.state);
        let open = state.as_mut().ok_or("소켓이 열려 있지 않아요")?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        open.tokens.insert(token.clone(), workflow_id.to_string());
        Ok(token)
    }

    pub fn revoke(&self, token: &str) {
        if let Some(o) = lock(&self.state).as_mut() {
            o.tokens.remove(token);
        }
    }

    pub fn revoke_workflow(&self, workflow_id: &str) {
        if let Some(o) = lock(&self.state).as_mut() {
            o.tokens.retain(|_, w| w != workflow_id);
        }
    }

    /// 토큰 전부 폐기 + 받기 중단 + 소켓 파일 삭제. 여러 번 불러도 된다.
    pub fn close(&self) {
        close_open(lock(&self.state).take());
    }
}

async fn accept(
    listener: std::os::unix::net::UnixListener,
    cancel: CancellationToken,
    handler: Handler,
    state: State,
) {
    let listener = match tokio::net::UnixListener::from_std(listener) {
        Ok(l) => l,
        Err(e) => return eprintln!("[ieum] 소켓을 받지 못했어요: {e}"),
    };
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            conn = listener.accept() => match conn {
                Ok((stream, _)) => {
                    tokio::spawn(serve(stream, handler.clone(), state.clone()));
                }
                // 파일 수 한도 같은 일시 오류 — 바쁘게 돌지 않는다.
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            },
        }
    }
}

async fn serve(stream: UnixStream, handler: Handler, state: State) {
    let (read, mut write) = stream.into_split();
    let reply = match read_line(read).await {
        Some(line) => answer(&line, &handler, &state).await,
        None => Reply::err(Code::BadRequest),
    };
    // 클라이언트가 끊겼어도 동작은 이미 끝났다.
    let _ = write
        .write_all(format!("{}\n", reply.to_line()).as_bytes())
        .await;
}

/// `READ_TIMEOUT` 안에 `\n`까지. 너무 길거나 늦거나 UTF-8이 아니면 None.
async fn read_line(read: tokio::net::unix::OwnedReadHalf) -> Option<String> {
    let mut buf = Vec::new();
    let mut limited = BufReader::new(read).take(MAX_LINE as u64 + 1);
    tokio::time::timeout(READ_TIMEOUT, limited.read_until(b'\n', &mut buf))
        .await
        .ok()?
        .ok()?;
    if buf.pop() != Some(b'\n') {
        return None;
    }
    String::from_utf8(buf).ok()
}

async fn answer(line: &str, handler: &Handler, state: &State) -> Reply {
    let req = match Request::parse(line) {
        Ok(r) => r,
        Err(reply) => return reply,
    };
    let session = lock(state).as_ref().and_then(|o| {
        Some(Session {
            account: o.account.clone(),
            workflow_id: o.tokens.get(&req.token)?.clone(),
        })
    });
    match session {
        Some(s) => handler(s, req.op).await,
        None => Reply::err(Code::Unauthorized),
    }
}
