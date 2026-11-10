//! AI에 넘길 파일 내용.

use std::fs::File;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

/// AI에 넘기는 파일 내용 상한(글자).
pub const MAX_CHARS: usize = 50_000;
/// 텍스트 파일은 앞부분만 읽는다 — UTF-8 한 글자는 최대 4바이트.
const MAX_TEXT_BYTES: u64 = (MAX_CHARS * 4) as u64;

/// UTF-8 텍스트 파일은 내용, PDF는 추출한 텍스트, 그 밖(바이너리·읽기 실패)은 `""`. 최대 50,000자.
pub fn file_text(path: &Path) -> String {
    let is_pdf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let text = if is_pdf {
        pdf_text(path)
    } else {
        utf8_text(path)
    };
    text.chars().take(MAX_CHARS).collect()
}

fn utf8_text(path: &Path) -> String {
    let mut buf = Vec::new();
    let Ok(f) = File::open(path) else {
        return String::new();
    };
    if f.take(MAX_TEXT_BYTES).read_to_end(&mut buf).is_err() || buf.contains(&0) {
        return String::new();
    }
    match std::str::from_utf8(&buf) {
        Ok(s) => s.to_string(),
        // 상한에서 잘린 마지막 글자만 버린다. 중간에 깨진 바이트가 있으면 바이너리로 본다.
        Err(e) if e.error_len().is_none() => {
            String::from_utf8_lossy(&buf[..e.valid_up_to()]).into_owned()
        }
        Err(_) => String::new(),
    }
}

/// `file_text`를 별도 스레드에서 돌리고 시간 상한을 건다. 넘으면 `""`(노드는 파일 이름만으로 진행).
/// 취소는 호출하는 쪽 `select!`가 이 future를 버리는 것으로 한다.
// ponytail: 시간 초과·취소 뒤에도 추출 스레드는 끝까지 돈다(압축 폭탄이면 CPU·메모리를 계속 씀).
// 업그레이드 = 별도 프로세스에서 추출하고 kill·RLIMIT_AS로 상한.
pub async fn file_text_bounded(path: &Path) -> String {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let path = path.to_path_buf();
    // spawn_blocking이 아니라 분리된 스레드: 런타임 종료가 끝나지 않는 추출을 기다리지 않게.
    std::thread::spawn(move || {
        let _ = tx.send(file_text(&path));
    });
    tokio::time::timeout(PDF_TIMEOUT, rx)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
}

/// PDF 추출 시간 상한.
const PDF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// 이보다 큰 PDF는 추출하지 않는다.
pub const MAX_PDF_BYTES: u64 = 20 * 1024 * 1024;

fn pdf_text(path: &Path) -> String {
    if std::fs::metadata(path).map_or(true, |m| m.len() > MAX_PDF_BYTES) {
        return String::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    // pdf-extract는 깨진 PDF에서 panic할 수 있다.
    catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(&bytes)
    }))
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default()
}
