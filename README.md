# ieum-desktop

```sh
npm install
npm run tauri dev
cargo test --workspace
npm test
```

## 배포 빌드

```sh
npm run tauri build   # → target/release/bundle/dmg/*.dmg
```

- 배포 산출물은 **반드시 `npm run tauri build`** 로 만든다. custom-protocol로 묶여 `dist`와 CSP가 적용된다. `cargo build --release`로 만든 바이너리는 `devUrl`(localhost:1420)을 불러오므로 배포하면 안 된다.
- 설치 안내(공증 전 "그래도 열기"): [docs/INSTALL.md](docs/INSTALL.md)

## 릴리스와 자동 업데이트

`v*` 태그를 push하면 `.github/workflows/release.yml`이 macOS arm64로 빌드해 Release **초안**에 `.dmg`·`latest.json`을 올린다. 초안을 확인하고 직접 게시한다. 앱은 `releases/latest/download/latest.json`을 보고 새 버전을 받는다.

처음 한 번, 업데이트 서명 키를 만든다(키는 커밋하지 않는다):

1. `npm run tauri signer generate -- -w ~/.tauri/ieum.key`
2. 출력된 **공개키**를 `src-tauri/tauri.conf.json`의 `plugins.updater.pubkey` 자리표시자(`REPLACE_WITH_PUBLIC_KEY_…`)에 넣는다.
3. 개인키 파일 내용과 비밀번호를 GitHub 저장소 Secrets `TAURI_SIGNING_PRIVATE_KEY`·`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`에 넣는다.

업데이트 산출물(`createUpdaterArtifacts`)은 CI에서 `src-tauri/tauri.updater.conf.json`으로만 켠다 — 로컬 빌드는 키 없이 된다. 공개키가 자리표시자인 동안 앱의 업데이트 확인은 실패를 로그로만 남긴다.
