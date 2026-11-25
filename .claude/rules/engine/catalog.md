---
paths:
  - "catalog/**"
  - "crates/ieum-engine/src/catalog.rs"
---
- 노드 이벤트 정의는 `crates/ieum-engine/src/catalog.rs`의 `include_str!` 목록으로 컴파일 때 박힌다 — 새 이벤트는 `catalog/`에 넣고 이 목록에도 추가
- `catalog/`는 BE·agent와 공유하는 서브모듈 — 여기서 고치지 말고 원본 저장소(team-ieum/ieum-node-catalog, 로컬 위치는 개인 설정 파일)에서 고친 뒤 서브모듈 커밋을 올린다
