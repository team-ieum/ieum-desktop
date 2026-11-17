---
paths:
  - "**/*.rs"
---
- 모듈 첫머리 `//!`, 항목 `///` 주석은 한국어로 "왜"를 적는다 — BE와 다르게 둔 곳은 주석에 `GAPS.md`를 가리킨다
- 라이브러리 크레이트(`ieum-engine`·`ieum-local`)의 에러는 `thiserror` 열거형. 앱에서 화면으로 넘기는 에러는 `String`
- 앱 로그는 `eprintln!("[ieum] ...")` 형식
- 앱 쪽 `Mutex`는 `.lock().unwrap_or_else(|e| e.into_inner())` — 다른 스레드의 패닉으로 앱 전체가 멈추지 않게
- 핵심 로직은 Tauri 없이 돌게 의존성을 주입한다(`supervisor`의 `Deps`, 경로 검사의 `home`·`app_data`) — 그래야 `tests/`에서 검증할 수 있다
