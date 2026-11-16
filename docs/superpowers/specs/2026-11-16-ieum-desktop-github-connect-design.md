# IEUM Desktop — GitHub 연결 (gh CLI) 설계

2026-11-16. 상태: 승인됨. 최종 리뷰 반영(같은 날).

## 왜
사용자가 데스크톱에서 GitHub 워크플로우를 직접 돌려 보고 싶다. 그 첫 단계로 **로그인(연결)만** 먼저 만든다. GitHub 노드 23개 실행은 다음 이슈.

11-15 결정(연결 = 데스크톱 자체 연결 + 교환 서버)에서 GitHub만 예외로, 교환 서버 없이 사용자 Mac의 `gh` CLI 로그인을 쓴다. 순서도 바뀐다: 1b(스케줄·수동 트리거)보다 먼저.

## 목표
- 설정에 새 메뉴 **연동**을 두고, 그 안에 GitHub 줄 하나. 구조는 Orca 설정 > 연동, 색은 이음.
- gh가 없으면 안내 후 물어보고, [설치]를 누르면 앱이 뒤에서 설치한다.
- gh가 낮은 버전이면 같은 방식으로 업데이트한다.
- [연결]을 누르면 앱이 뒤에서 `gh auth login --web`을 돌리고 일회용 코드를 카드로 보여 준다. 로그인이 끝나면 저절로 "연결됨".

## 하지 않는 것
- GitHub 노드 실행(다음 이슈). 이번엔 엔진·카탈로그를 건드리지 않는다.
- GitLab·Bitbucket 등 다른 서비스. 화면은 줄을 더 붙일 수 있는 모양이면 충분하다.
- GitHub Enterprise 호스트. `github.com`만.
- IEUM 자체 토큰 저장. 토큰은 gh가 자기 키체인에 갖고, IEUM은 쓸 때마다 `gh auth token`으로 읽는다(노드 실행 이슈에서).
- gh 여러 계정 전환 UI. 활성 계정 하나만 보여 준다.

## 실패로 치는 것
- 사용자 터미널의 gh·git 설정을 몰래 바꾸는 것(로그아웃, git credential helper 변경 등). **IEUM은 gh에서 로그아웃시키지 않는다.** 바뀌는 게 있으면 화면에 먼저 말한다.
- 설치가 확인 없이 시작되는 것.

## 고른 방식과 버린 방식
| 결정 | 고른 것 | 버린 것 |
|---|---|---|
| 로그인 화면 | 앱이 gh를 뒤에서 돌리고 코드 카드 (사용자 선택) | 앱 안 터미널에 `gh auth login` 띄우기(투박), 안내만(앱 밖으로 나감) |
| 설치 | Homebrew 있으면 `brew install gh`, 없으면 GitHub 공식 릴리스 zip을 받아 앱 폴더에 (사용자 선택) | brew만(없는 사용자 막힘), 항상 앱 폴더(이미 깔린 gh와 두 벌) |

## 확인한 사실 (gh 2.103.3, 이 Mac)
- 터미널 없이(stdin 닫음) `gh auth login --web --hostname github.com --git-protocol https --skip-ssh-key` 실행 시 **stderr**에:
  ```
  ! One-time code (C0FE-571F) copied to clipboard
  Open this URL to continue in your web browser: https://github.com/login/device
  ```
  Enter를 기다리지 않고, 브라우저를 직접 열지 않는다. 코드는 gh가 클립보드에 복사한다. 그 뒤 브라우저 승인까지 폴링하며 기다리다 끝나면 종료 코드 0.
- `gh auth status --json hosts` → `{"hosts":{"github.com":[{"state":"success","active":true,"login":"hodoon","tokenSource":"keyring","scopes":"gist, read:org, repo, workflow",…}]}}`. 로그인 안 됐으면 `{"hosts":{}}`, **종료 코드 0**(최종 리뷰 실측 — 처음 적은 "종료 1"은 틀림). 토큰 무효·네트워크 오류도 종료 0, 활성 항목 `state:"error"` + `error` 문자열(401은 `Bad credentials`, 네트워크는 `proxyconnect …` 등).
- 기능 도입 버전: `auth status --json` **2.81.0**, `--skip-ssh-key` 2.48.0, `auth token` 2.17.0. → **최소 버전 2.81.0**.
- 최신 릴리스 v2.104.0 파일: `gh_<v>_macOS_arm64.zip`, `gh_<v>_macOS_amd64.zip`, `gh_<v>_checksums.txt`(sha256), `…universal.pkg`.

## 동작

### 실행 환경 (설계 결정)
- **gh·brew 명령은 전부 사용자 로그인 셸(`$SHELL -lc`)로 실행**한다. 터미널과 같은 환경이라 `GH_CONFIG_DIR`·`XDG_CONFIG_HOME`이 있어도 앱과 터미널이 같은 로그인을 본다.
- 기존 `shell_output`(`crates/ieum-local/src/ai/cli.rs:934`)은 stderr를 버리고 종료 코드≠0이면 `None`이라 찾기에만 쓴다. 상태·로그인·설치에는 stdout·stderr·종료 코드를 모두 돌려주고 stderr를 줄 단위로 흘려 주는 새 실행기를 둔다(시간 상한·프로세스 그룹 종료는 `shell_output`과 같은 방식).

### gh 찾기
1. 사용자 로그인 셸 PATH: `command -v gh && gh --version`. 첫 줄은 경로, **둘째 줄** `gh version X.Y.Z (…)`에서 버전(`detect_clis`와 같은 읽기).
2. 없으면 앱 폴더 `~/Library/Application Support/<앱 id>/bin/gh`.
- 둘 다 있으면 PATH 쪽이 2.81.0 이상일 때 PATH 쪽, 아니면 앱 폴더 쪽(이것도 2.81.0 이상일 때).
- 두 gh는 같은 설정 폴더(`~/.config/gh`)와 키체인을 쓰므로 로그인은 공유된다.

### IEUM 연결 기록 (사용자 결정: 해제는 IEUM에서만)
- 연결 여부는 IEUM이 따로 기록한다: 계정 폴더 설정 파일에 `github: { login }`(토큰 아님). 없으면 연결 안 됨.
- [연결] = 그 시점 gh 활성 계정 `login`을 기록. [연결 해제] = 기록만 지운다. gh 로그인은 그대로.
- gh 활성 계정이 기록과 다르면(터미널에서 계정을 바꾼 경우) "gh 계정이 <새 login>(으)로 바뀌었어요" + [이 계정으로 연결]. 몰래 따라가지 않는다.

### 상태 (화면 배지)
| 상태 | 조건 | 줄에 보이는 것 |
|---|---|---|
| 설치되지 않음 | gh 없음 | "GitHub을 쓰려면 gh CLI가 필요해요. 설치할까요?" [설치] [다시 확인] |
| 업데이트 필요 | gh < 2.81.0 | "gh X.Y.Z는 너무 오래됐어요(2.81.0 이상 필요)." [업데이트] [다시 확인] |
| 설치 중 / 업데이트 중 | 진행 중 | 진행 표시, [취소] |
| 연결 안 됨 | IEUM 기록 없음 | [연결] (gh 로그인 여부는 버튼을 누른 뒤 갈린다) |
| 코드 입력 대기 | 로그인 프로세스 진행 중 | 코드 카드 |
| 연결됨 | IEUM 기록 있음 + gh 활성 계정이 같은 login, `state: success` | 계정 이름(`login`), [연결 해제] |
| 계정 바뀜 | IEUM 기록 login ≠ gh 활성 login | 위 문구 + [이 계정으로 연결] [연결 해제] |
| (환경변수 토큰 표시) | 활성 항목 `tokenSource`가 `GH_TOKEN`·`GITHUB_TOKEN` | 위 상태에 "셸 환경변수 토큰을 쓰는 중이에요." 한 줄 추가. gh 로그인 단계는 건너뛴다 |
| 닿지 못함 | 활성 항목 state≠success이고 `error`에 `401`·`Bad credentials`가 없음(네트워크 문제) | "GitHub에 닿지 못했어요." + 오류 한 줄 [다시 확인]. [연결]은 로그인을 돌리지 않는다 |
| 연결 문제 | IEUM 기록 있음 + gh 로그아웃됨 또는 state≠success(토큰 만료 등) | "gh 로그인이 필요해요" + "연결하면 터미널의 gh 로그인도 새 로그인으로 바뀌어요." [연결] |

상태 명령은 `gh auth status --active --json hosts --hostname github.com`(상한 10초). 상태는 연동 메뉴를 열 때와 [다시 확인]·작업 완료 후에 다시 읽는다. 주기적 폴링은 하지 않는다.

### 설치·업데이트 ([설치]/[업데이트] = 사용자가 이미 확인한 것)
- `brew`가 로그인 셸 PATH에 있으면: `brew install gh` / `brew upgrade gh`. 단, 업데이트 대상 gh가 brew 것이 아니면(경로가 brew prefix 밖) 아래 공식 릴리스 경로로 앱 폴더에 새 gh를 둔다(기존 gh는 건드리지 않음).
- 아니면 공식 릴리스: GitHub API `repos/cli/cli/releases/latest`로 태그 → 아키텍처(`aarch64`→arm64, `x86_64`→amd64) zip과 checksums.txt 받기 → sha256 대조 → 안의 `bin/gh`만 앱 폴더 `bin/gh`로(임시 파일에 쓰고 rename, 실행 권한) → `gh --version` 2.81.0 이상인지 확인.
- 체크섬 불일치·버전 미달은 실패, 받은 파일은 지운다.
- 시간 상한 10분, [취소]는 프로세스 그룹 종료.
- 실패 시 줄에 한 줄 이유 + [다시 시도]. brew 출력 꼬리 몇 줄은 펼쳐 보기로.

### 연결 ([연결])
- **gh가 이미 로그인돼 있으면(활성 계정 `state: success`) 코드 없이 바로 그 계정을 기록**하고 연결됨. 다시 로그인시키지 않는다(gh 전역 로그인을 건드리지 않으려고).
- gh가 로그인 안 됐거나 토큰이 무효일 때만 아래 로그인을 돌린다(사용자 결정: 터미널에서 직접 로그인한 것과 같은 결과):
- `gh auth login --web --hostname github.com --skip-ssh-key --scopes workflow`, gh에 github.com 계정 항목이 없을 때만 `--git-protocol https`를 덧붙인다(있으면 사용자의 기존 프로토콜 유지). 이 명령을 stdin 닫고 실행, stderr를 줄 단위로 읽는다.
- `[A-Z0-9]{4}-[A-Z0-9]{4}` 코드와 `https://` 주소를 찾으면 카드 표시:
  ```
  일회용 코드  C0FE-571F  [복사]  [브라우저 열기]
  브라우저에서 코드를 입력하면 자동으로 연결돼요.  [취소]
  ```
  [브라우저 열기]는 Rust가 고정값 `https://github.com/login/device`를 `/usr/bin/open`으로 연다(`open_url`의 "주소는 화면이 아닌 고정값에서" 규칙). gh 출력의 주소는 쓰지 않고, 코드만 읽는다.
- 종료 0 → 상태 다시 읽기 → 활성 login 기록 → 연결됨. 0이 아니면 stderr 마지막 줄을 이유로 보여 주고 "연결 안 됨".
- 코드가 10초 안에 안 나오면 실패(gh 출력 형식이 바뀐 경우) — 이유: "gh 출력을 읽지 못했어요. gh 버전 X". 원문 stderr는 펼쳐 보기로.
- 시간 상한 15분(device code 만료), [취소]·메뉴 떠남은 프로세스 종료(설치는 메뉴를 떠나도 계속). 동시에 하나만. 진행 작업은 `github_job`으로 조회해, 메뉴에 다시 들어오면 코드 카드·[취소]를 이어 보여 준다. 앱 종료 때 진행 작업을 취소한다.
- gh 로그인이 필요한 경우에만 gh를 돌리므로 gh의 "다시 로그인할까요?" 질문은 생기지 않는다.

### 연결 해제 ([연결 해제])
- IEUM 기록만 지운다. gh 로그아웃 없음, 확인 카드 없음(되돌리기 = [연결] 한 번).

## 코드 위치 (제안)
- Rust: `src-tauri/src/github.rs` 새 파일 — 찾기·상태·설치·로그인·연결 기록, Tauri 명령 + 진행 이벤트. gh 찾기는 `ieum_local::ai::shell_output` 재사용. 다운로드는 이미 쓰는 `reqwest`, 체크섬은 `sha2`, 앱 폴더는 `app_data_dir`(`com.ieum.desktop`). zip 풀기(`zip` 크레이트 직접 추가 또는 `/usr/bin/ditto`)는 빌더 몫.
- 화면: `src/screens/settings/IntegrationsPane.tsx` 새 파일, `SettingsShell` `SECTIONS`에 `integrations` 추가, 검색 문구(`texts`) 포함, i18n ko·en.

## 빌더에게 남긴 것
- 진행 이벤트 이름·모양, 상태 타입 이름.
- 메뉴 아이콘·묶음 위치(Orca처럼 "연동" 묶음 권장).
- 테스트: 가짜 gh 스크립트(stderr 문구·종료 코드 흉내)로 파싱·상태 전이·시간 상한 확인. 실제 설치·로그인은 수동 확인.
