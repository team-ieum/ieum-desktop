---
name: release
description: IEUM 데스크톱 새 버전을 배포한다. 버전 올림 → dev→main PR → main에 v태그 push → CI가 만든 Release 초안 확인까지. 인자로 새 버전(예: 0.1.2)을 받는다.
disable-model-invocation: true
---
새 버전: $ARGUMENTS (비었으면 지금 버전을 보여 주고 물어본다)

push·PR 생성·머지·태그 push·게시는 매번 사용자에게 확인받고 진행한다. 어느 단계든 실패하면 멈추고 보고한다.

## 1. 준비
- `dev` 브랜치, 깨끗한 작업 트리인지 확인. 아니면 멈추고 묻는다
- `git pull --rebase`
- 검사 전부 통과: `cargo fmt --check`, `cargo clippy --workspace`, `cargo test --workspace`, `npm test`, `npx tsc`, `cargo run -p ieum-cli --bin ieum-dev -- conformance conformance/`
- `src-tauri/tauri.conf.json`의 `plugins.updater.pubkey`가 `REPLACE_WITH_PUBLIC_KEY`로 시작하면 멈춘다 — 업데이트 서명이 안 된다(README "릴리스와 자동 업데이트")

## 2. 버전 올림
- `chore/v<버전>` 브랜치를 `dev`에서 딴다
- 세 곳의 `version`을 고친다: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
- 잠금 파일 갱신: `npm install --package-lock-only --ignore-scripts`, `cargo check -p ieum-desktop`
- `git diff --stat`이 정확히 5개 파일(위 3개 + `package-lock.json`·`Cargo.lock`)인지 확인
- 커밋 `chore: 버전 <버전>` → push → `dev`로 PR(제목 `[chore] 버전 <버전>`) → 머지 커밋으로 머지

## 3. 배포 PR (dev → main)
- 지난 태그 이후 `dev`에 머지된 PR을 모은다: `git log --first-parent --format='%s' v<지난 버전>..origin/dev`
- 제목: `[feat] <주요 변경 요약> (<버전>)` (예: `[feat] 화면 개편·여러 계정·캔버스 채팅·업데이트 알림 (0.1.1)`)
- 본문 `## 무엇`: 사용자에게 보이는 변경만 PR당 한 줄, `(#PR번호)`를 단다
- 머지 커밋으로 머지(squash 금지)

## 4. 태그
- `git checkout main && git pull --rebase` 후 HEAD가 방금 머지한 커밋인지 확인
- `git tag v<버전> && git push origin v<버전>` — 태그는 `main`에만 붙인다
- 로컬에서 `cargo build --release`나 `npm run tauri:build`로 배포물을 만들지 않는다. 배포물은 CI만 만든다

## 5. 초안 확인
- `gh run list --workflow release.yml --limit 1`로 실행을 찾아 `gh run watch <id>`로 끝까지 본다
- `gh release view v<버전>`: 초안(draft)이고 `.dmg`·`latest.json`이 올라왔는지 확인
- 게시는 사용자가 GitHub에서 직접 한다(또는 명시적으로 요청하면 `gh release edit v<버전> --draft=false`). 게시해야 앱 자동 업데이트가 새 버전을 받는다
- 끝나면 `dev`로 돌아가 `git pull --rebase`
