---
paths:
  - "crates/*/tests/**"
  - "src-tauri/tests/**"
---
- 테스트는 각 크레이트의 `tests/` 통합 테스트로 쓴다
- 파일은 `tempfile::TempDir` 안에서만 만든다 — 휴지통·홈·앱 데이터 폴더도 임시 폴더로 주입. 실제 사용자 폴더를 건드리지 않는다
- 외부 AI CLI는 `crates/ieum-local/tests/fixtures/fake_*.sh` 같은 가짜 스크립트로 대신한다. 빌더 테스트는 공용 `tests/builder_support/`를 쓴다
- BE 실행 의미와 맞춰야 하는 동작은 `conformance/`에 케이스를 더하거나, 묶음으로 못 하면 엔진 테스트에 BE 대조표로 고정한다
