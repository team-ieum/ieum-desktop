---
paths:
  - "crates/ieum-engine/src/**"
  - "conformance/**"
---
- 실행 의미의 정답은 BE — BE와 일부러 다르게 동작시키면 `conformance/GAPS.md`에 적고 그 차이를 고정하는 테스트를 단다
- 바꾼 뒤 `cargo run -p ieum-cli --bin ieum-dev -- conformance conformance/`로 대조 묶음을 확인
