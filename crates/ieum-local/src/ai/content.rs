//! AI에 넘길 파일 내용.

use std::fs::File;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// AI에 넘기는 파일 내용 상한(글자).
pub const MAX_CHARS: usize = 50_000;
/// 텍스트 파일은 앞부분만 읽는다 — UTF-8 한 글자는 최대 4바이트.
const MAX_TEXT_BYTES: u64 = (MAX_CHARS * 4) as u64;
/// PDF 추출 시간 상한.
const PDF_TIMEOUT: Duration = Duration::from_secs(20);
/// 이보다 큰 PDF는 추출하지 않는다.
pub const MAX_PDF_BYTES: u64 = 20 * 1024 * 1024;

/// 내용을 못 읽은 이유. 노드는 파일 이름만으로 진행하고 사용자에게 한 줄 알린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// PDF가 크기 상한을 넘음.
    TooBig,
    /// 다른 PDF를 추출하는 중(전역 1개).
    Busy,
    /// 추출이 시간 상한을 넘음.
    Timeout,
    /// 없음·읽기 실패·바이너리·텍스트 없는 PDF·일반 파일 아님(FIFO 등).
    Unreadable,
    /// 하드 링크(링크 수 > 1) — 허용 폴더 밖 파일의 다른 이름일 수 있다.
    HardLink,
}

impl Skip {
    pub fn notice(self) -> &'static str {
        match self {
            Skip::TooBig => "파일이 커서 내용 없이 이름으로만 판단해요.",
            Skip::Busy => "다른 PDF를 읽는 중이라 내용 없이 이름으로만 판단해요.",
            Skip::Timeout => "파일 내용을 읽는 데 너무 오래 걸려 이름으로만 판단해요.",
            Skip::Unreadable => "파일 내용을 읽지 못해 이름으로만 판단해요.",
            Skip::HardLink => {
                "다른 곳의 파일과 이어진 파일(하드 링크)이라 내용 없이 이름으로만 판단해요."
            }
        }
    }
}

/// UTF-8 텍스트 파일은 내용, PDF는 추출한 텍스트, 그 밖(바이너리·읽기 실패)은 `""`. 최대 50,000자.
pub fn file_text(path: &Path) -> String {
    read_file_text(path).unwrap_or_default()
}

/// `file_text`와 같되 못 읽은 이유를 돌려준다.
pub fn read_file_text(path: &Path) -> Result<String, Skip> {
    let is_pdf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let text = if is_pdf {
        pdf_text(path)?
    } else {
        utf8_text(path)?
    };
    Ok(text.chars().take(MAX_CHARS).collect())
}

/// macOS `<fcntl.h>` 값.
const O_NONBLOCK: i32 = 0x0004;
const O_NOFOLLOW: i32 = 0x0100;

/// 링크를 따라가지 않고 멈추지 않게 연 뒤(fstat) 일반 파일·링크 수 1만 돌려준다.
/// FIFO 열기는 O_NONBLOCK이라 바로 돌아오고 종류 검사에서 걸린다.
fn open_regular(path: &Path) -> Result<(File, u64), Skip> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK | O_NOFOLLOW)
        .open(path)
        .map_err(|_| Skip::Unreadable)?;
    let m = f.metadata().map_err(|_| Skip::Unreadable)?;
    if !m.file_type().is_file() {
        return Err(Skip::Unreadable);
    }
    if m.nlink() > 1 {
        return Err(Skip::HardLink);
    }
    Ok((f, m.len()))
}

fn utf8_text(path: &Path) -> Result<String, Skip> {
    let mut buf = Vec::new();
    let (f, _) = open_regular(path)?;
    if f.take(MAX_TEXT_BYTES).read_to_end(&mut buf).is_err() || buf.contains(&0) {
        return Err(Skip::Unreadable);
    }
    match std::str::from_utf8(&buf) {
        Ok(s) => Ok(s.to_string()),
        // 상한에서 잘린 마지막 글자만 버린다. 중간에 깨진 바이트가 있으면 바이너리로 본다.
        Err(e) if e.error_len().is_none() => {
            Ok(String::from_utf8_lossy(&buf[..e.valid_up_to()]).into_owned())
        }
        Err(_) => Err(Skip::Unreadable),
    }
}

/// `read_file_text`를 별도 스레드에서 돌리고 시간 상한을 건다.
/// 취소는 호출하는 쪽 `select!`가 이 future를 버리는 것으로 한다.
// ponytail: 시간 초과·취소 뒤에도 추출 스레드는 끝까지 돈다(압축 폭탄이면 코어 1개·폭탄 1개 분량 메모리).
// 동시 추출을 1개로 묶어 누적은 막았다. 업그레이드 = 별도 프로세스에서 추출하고 kill·RLIMIT_AS로 상한.
pub async fn file_text_bounded(path: &Path) -> Result<String, Skip> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let path = path.to_path_buf();
    // spawn_blocking이 아니라 분리된 스레드: 런타임 종료가 끝나지 않는 추출을 기다리지 않게.
    std::thread::spawn(move || {
        let _ = tx.send(read_file_text(&path));
    });
    match tokio::time::timeout(PDF_TIMEOUT, rx).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err(Skip::Unreadable), // 스레드가 답 없이 끝남
        Err(_) => Err(Skip::Timeout),
    }
}

/// 전역 PDF 추출 1개. 끝나지 않는 폭탄 스레드가 쌓이지 않게.
static PDF_BUSY: AtomicBool = AtomicBool::new(false);

struct PdfSlot;

impl PdfSlot {
    fn take() -> Option<PdfSlot> {
        PDF_BUSY
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| PdfSlot)
    }
}

impl Drop for PdfSlot {
    fn drop(&mut self) {
        PDF_BUSY.store(false, Ordering::Release);
    }
}

fn pdf_text(path: &Path) -> Result<String, Skip> {
    let (f, len) = open_regular(path)?;
    if len > MAX_PDF_BYTES {
        return Err(Skip::TooBig);
    }
    let _slot = PdfSlot::take().ok_or(Skip::Busy)?;
    let mut bytes = Vec::new();
    f.take(MAX_PDF_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|_| Skip::Unreadable)?;
    // pdf-extract는 깨진 PDF에서 panic할 수 있다.
    catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(&bytes)
    }))
    .ok()
    .and_then(Result::ok)
    .filter(|t| !t.trim().is_empty())
    .ok_or(Skip::Unreadable)
}
