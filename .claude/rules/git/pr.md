# PR
- 대상: 작업 브랜치 → `dev`. 배포(`dev` → `main`·태그)는 `/release` 스킬로만 한다
- 제목: `[<유형>] <한국어 설명> (#<이슈번호>)` (예: `[feat] 스케줄·수동 실행 트리거를 데스크톱에서 (#7)`)
- 본문:
  - `Closes #<이슈번호>`
  - `## 무엇` — 바뀐 동작과 이유. 클라우드(BE)와 다르게 둔 점은 `conformance/GAPS.md` 위치로 안내
  - `## 확인` — 돌린 검사와 결과: `cargo test --workspace`·`npm test` 통과 수, 적합성 묶음, `cargo fmt --check`·`cargo clippy`·`npx tsc`, 앱에서 직접 확인한 것
  - `## 남은 것`(있으면) — 이번에 안 고친 문제
- 설계·계획 문서가 있으면 `docs/superpowers/specs/`·`plans/` 경로를 단다

# 머지
- PR은 머지 커밋으로 합친다(squash·rebase 머지 안 함) — 작업 커밋 이력을 남긴다
