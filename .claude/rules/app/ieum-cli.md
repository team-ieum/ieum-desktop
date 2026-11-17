---
paths:
  - "crates/ieum-cli/src/bin/**"
  - "crates/ieum-local/src/builder/**"
  - "src-tauri/src/builder_host.rs"
---
- `ieum` CLI는 앱 소켓(`IEUM_SOCKET`·`IEUM_TOKEN`)에 요청 한 줄을 보내는 얇은 클라이언트 — 편집 로직은 `ieum-local::builder`에, 앱 쪽 연결은 `builder_host.rs`에 둔다
