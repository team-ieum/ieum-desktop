//! 동기 `#[tauri::command]`는 메인 스레드에서 돈다. 키체인 읽기·쓰기는 접근 허용 창이
//! 뜨면 답할 때까지 멈추므로, 동기 명령에서 하면 화면 전체가 굳는다(배포 빌드 빈 화면).
//! 그래서 `Keys`를 받는 명령은 모두 `async`여야 한다.

#[test]
fn keychain_commands_are_async() {
    let src = include_str!("../src/commands.rs");
    let sync: Vec<&str> = src
        .split("#[tauri::command]")
        .skip(1)
        .map(|chunk| chunk.split('{').next().unwrap_or(""))
        .filter(|sig| {
            (sig.contains("Keys>") || sig.contains("PlanUsageClient>")) && !sig.contains("async fn")
        })
        .collect();
    assert!(sync.is_empty(), "키체인을 쓰는 동기 명령: {sync:#?}");
}
