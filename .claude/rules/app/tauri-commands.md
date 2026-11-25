---
paths:
  - "src-tauri/src/**/*.rs"
  - "src-tauri/build.rs"
  - "src-tauri/capabilities/**"
  - "src/api.ts"
---
- Tauri 명령을 추가하면 3곳 모두 등록: `src-tauri/src/lib.rs`의 `generate_handler![...]`, `src-tauri/build.rs`의 `COMMANDS`, `src-tauri/capabilities/default.json`의 `allow-<명령>` — 하나라도 빠지면 화면에서 "not allowed"로 실패한다(`src-tauri/tests/command_registry.rs`가 검사)
- 키체인(`Keys`)을 받는 명령은 반드시 `async` — 동기 명령은 메인 스레드에서 돌아 키체인 허용 창이 뜨면 화면 전체가 굳는다(`src-tauri/tests/commands_off_main.rs`가 검사)
- `src/api.ts`의 타입은 러스트 직렬화(camelCase)와 1:1로 맞춘다
