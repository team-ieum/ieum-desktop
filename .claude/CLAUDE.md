# Commands
- 처음 받은 뒤: `git submodule update --init` — `catalog/`가 없으면 엔진이 컴파일되지 않는다
- 개발 실행: `npm run tauri dev` (Vite 1420 포트 고정)
- 러스트 테스트 파일 하나: `cargo test -p ieum-engine --test render` (앱 크레이트 이름은 `ieum-desktop`)
- 프론트 테스트 파일 하나: `npx vitest run src/editor/Canvas.test.tsx`
- 타입 검사: `npx tsc`
- BE 대조 검사: `cargo run -p ieum-cli --bin ieum-dev -- conformance conformance/`

# Conventions
- 주석·에러 메시지는 한국어
- 영역별 규칙은 `.claude/rules/`에 있다(해당 파일을 다룰 때만 로드)
- 릴리스 서명 키·업데이트 설정은 `README.md` "릴리스와 자동 업데이트" 참고

# 개인 설정 (없으면 무시)
- @~/.claude/ieum-desktop.md

# Gotchas
- 배포 빌드는 `npm run tauri:build`만 — `cargo build --release` 바이너리는 localhost:1420을 불러온다. `tauri:build`는 `ieum` CLI도 먼저 빌드해 번들에 넣는다
