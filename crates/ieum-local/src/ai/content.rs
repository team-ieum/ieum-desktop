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

// ponytail: PDF는 통째로 읽어 추출한다(크기 상한 없음). 큰 PDF가 문제 되면 크기 상한을 둔다.
fn pdf_text(path: &Path) -> String {
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
