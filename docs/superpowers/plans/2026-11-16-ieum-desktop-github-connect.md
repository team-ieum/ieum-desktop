# GitHub 연결 (gh CLI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 설정 > 연동에서 gh CLI로 GitHub을 연결·해제(IEUM 기록만)하고, gh가 없거나 낮으면 앱이 뒤에서 설치·업데이트한다.

**Architecture:** `ieum-local`에 stdout·stderr·종료 코드를 다 돌려주는 로그인 셸 실행기를 추가하고, `src-tauri/src/github.rs`가 그 위에서 gh 찾기·상태·로그인·설치·연결 기록을 맡는다. 화면은 `IntegrationsPane.tsx` 하나가 상태 명령과 이벤트 두 개로 그린다.

**Tech Stack:** Rust(Tauri 2, reqwest 0.13, sha2, serde_json), React 19 + TS, vitest + testing-library.

**Spec:** `docs/superpowers/specs/2026-11-16-ieum-desktop-github-connect-design.md`

## Global Constraints
- 이슈 #9, 브랜치 `feat/9-github-connect`, PR base `dev`.
- gh 최소 버전 **2.81.0**. 호스트는 `github.com`만.
- gh·brew 명령은 전부 사용자 로그인 셸(`$SHELL -lc`)로 실행.
- 로그인 명령은 정확히 `gh auth login --web --hostname github.com --git-protocol https --skip-ssh-key`, stdin 닫음.
- 상태 명령은 `gh auth status --json hosts --hostname github.com`. 종료 1 = 로그인 안 됨.
- **IEUM은 gh에서 로그아웃시키지 않는다.** 연결 해제 = 계정 폴더 `github.json` 삭제만. 토큰은 저장하지 않는다.
- 브라우저는 고정값 `https://github.com/login/device`만 연다(기존 `open_url`).
- 시간 상한: 상태·찾기 5초, 로그인 코드 대기 10초, 로그인 전체 15분, 설치·업데이트 10분.
- 앱 폴더 gh = `<app_data>/bin/gh`.
- 커밋 메시지: 한국어, 제목 `feat: …`, 본문에 `이슈 #9`. `Co-Authored-By` 넣지 않음. 워커는 커밋하지 않고 코디네이터가 커밋한다.
- Rust 파일은 perl·sed로 고치지 않는다(`cargo fmt` 어긋남). 끝에 `cargo fmt --all`.

## Review Focus
1. 사용자 셸에 `GH_TOKEN`이 있으면 `tokenSource: "GH_TOKEN"` — [연결]이 로그인 없이 바로 기록되고 "환경변수 토큰" 줄이 보여야 한다 → Task 2 `derive_env_token` 테스트.
2. gh가 PATH에 낮은 버전, 앱 폴더에 새 버전이 둘 다 있으면 앱 폴더 쪽을 써야 한다 → Task 3 `pick_gh_prefers_new_enough` 테스트.
3. 로그인 중 [취소]나 설정 화면 떠남 → gh 프로세스 그룹이 죽고 상태가 "연결 안 됨"으로 돌아와야 한다 → Task 4 `login_cancel_kills` 테스트 + Task 6 언마운트 테스트.
4. 체크섬이 안 맞는 zip → 앱 폴더에 아무것도 남지 않아야 한다 → Task 5 `install_bad_checksum_leaves_nothing` 테스트.
5. 터미널에서 gh 계정을 바꿈 → "계정 바뀜"이 뜨고 IEUM이 몰래 새 계정을 기록하지 않아야 한다 → Task 2 `derive_account_changed` 테스트.

---

### Task 1: 로그인 셸 실행기 `shell_run`

**Files:**
- Modify: `crates/ieum-local/src/ai/cli.rs` (`shell_output` 옆에 추가, `shell()`·`KillGroup` 재사용)
- Modify: `crates/ieum-local/src/ai/mod.rs` (`pub use cli::{shell_run, ShellRun, sh_quote}`)
- Test: `crates/ieum-local/tests/shell_run.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ShellRun { pub code: Option<i32>, pub stdout: String, pub stderr: String, pub timed_out: bool, pub cancelled: bool }
  pub fn shell_run(cmd: &str, limit: Duration, cancel: &AtomicBool, on_stderr_line: &mut dyn FnMut(&str)) -> std::io::Result<ShellRun>
  pub fn sh_quote(s: &str) -> String   // 기존 private → pub
  ```
  - `$SHELL -lc cmd`, cwd = temp dir, stdin null, `process_group(0)`. stdout·stderr를 각각 스레드로 읽는다(파이프 막힘 없음). stderr는 줄마다 콜백(메인 스레드에서 채널로 받아 호출). 저장하는 stdout·stderr는 각각 **마지막 64KB**만.
  - 시간 초과·`cancel`이 true → 그룹째 kill, `timed_out`/`cancelled` true, `code` None. 50ms 폴링.

- [ ] **Step 1: 실패하는 테스트**
  - `shell_run_returns_code_and_streams`: cmd `echo out; echo e1 1>&2; echo e2 1>&2; exit 3` → `code == Some(3)`, `stdout == "out\n"`, 콜백 받은 줄 `["e1","e2"]`, `stderr`에 `e2`.
  - `shell_run_timeout_kills_group`: cmd `sleep 30 & echo $! ; wait`, limit 300ms → `timed_out`, 반환까지 2초 미만, stdout의 pid가 `kill -0` 실패(죽음).
  - `shell_run_cancel`: 다른 스레드에서 200ms 뒤 `cancel.store(true)`, cmd `sleep 30` → `cancelled`, 2초 미만.
  - `shell_run_keeps_tail`: 200KB stdout → `stdout.len() <= 65536` 이고 마지막 줄 보존.
- [ ] **Step 2:** `cargo test -p ieum-local --test shell_run` → 컴파일 실패(FAIL).
- [ ] **Step 3:** 위 시그니처대로 구현.
- [ ] **Step 4:** 같은 명령 → PASS. `cargo test -p ieum-local` 전체 PASS.
- [ ] **Step 5: Commit** `feat: 로그인 셸 명령의 stdout·stderr·종료 코드를 받는 실행기 추가`

---

### Task 2: gh 출력 읽기·상태 판정 (순수 함수)

**Files:**
- Create: `src-tauri/src/github.rs` (이 태스크는 순수 함수만), `src-tauri/src/lib.rs`에 `pub mod github;`
- Test: `src-tauri/tests/github.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const MIN_GH: (u32, u32, u32) = (2, 81, 0);
  pub fn parse_gh_version(line: &str) -> Option<(u32, u32, u32)>        // "gh version 2.103.3 (2026-10-26)"
  pub fn parse_device_code(line: &str) -> Option<String>                // 줄 안 첫 [A-Z0-9]{4}-[A-Z0-9]{4} (앞뒤가 영숫자가 아님)
  #[derive(Deserialize)] pub struct GhAccount { pub login: String, pub active: bool, pub state: String, #[serde(rename="tokenSource")] pub token_source: String }
  pub fn parse_auth_status(json: &str) -> Result<Vec<GhAccount>, String> // hosts["github.com"] 배열, 없으면 빈 Vec
  pub enum GhAuth { LoggedOut, Accounts(Vec<GhAccount>) }               // 종료 1 → LoggedOut
  #[derive(Serialize, Clone, Debug, PartialEq)] #[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
  pub enum GithubStatus {
      NotInstalled,
      Outdated { version: String },
      NotConnected,
      NeedsLogin { recorded: String },                       // 기록 있음 + gh 로그아웃 또는 state≠"success"
      AccountChanged { recorded: String, current: String },
      Connected { login: String, env_token: bool },          // env_token = tokenSource가 GH_TOKEN|GITHUB_TOKEN
  }
  pub fn derive_status(recorded: Option<&str>, auth: &GhAuth) -> GithubStatus  // gh 설치·버전 통과 뒤 단계
  ```
- [ ] **Step 1: 실패하는 테스트** (`src-tauri/tests/github.rs`)
  - `parse_version_cases`: `"gh version 2.103.3 (2026-10-26)"`→`(2,103,3)`, `"gh version 2.81.0"`→`(2,81,0)`, `"nope"`→None. `(2,80,9) < MIN_GH`, `(2,81,0) >= MIN_GH`.
  - `parse_code_from_gh_stderr`: `"! One-time code (C0FE-571F) copied to clipboard"`→`"C0FE-571F"`; `"Open this URL to continue in your web browser: https://github.com/login/device"`→None; `"XC0FE-571F"`→None.
  - `parse_status_json`: 설계 문서의 실제 JSON(hodoon, keyring) → 1개, login `hodoon`, active, state `success`. `{"hosts":{}}` → 빈 Vec. 깨진 JSON → Err.
  - `derive_not_connected`: recorded None, 로그인돼 있어도 → `NotConnected`.
  - `derive_connected`: recorded `hodoon`, 활성 hodoon success keyring → `Connected{login:"hodoon", env_token:false}`.
  - `derive_env_token`: tokenSource `GH_TOKEN` → `env_token:true`.
  - `derive_account_changed`: recorded `hodoon`, 활성 `other` → `AccountChanged{recorded:"hodoon", current:"other"}`.
  - `derive_needs_login`: recorded `hodoon` + `LoggedOut` → `NeedsLogin`; 활성 hodoon state `error` → `NeedsLogin`.
  - `status_serializes_camel`: `Connected` → `{"kind":"connected","login":"hodoon","envToken":false}`.
- [ ] **Step 2:** `cargo test -p ieum-desktop --test github` → FAIL(없는 항목).
- [ ] **Step 3:** 구현. 정규식 크레이트 없이 바이트 스캔.
- [ ] **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat: gh 버전·로그인 코드·연결 상태 읽기`

---

### Task 3: gh 찾기·상태 명령·연결 기록·해제

**Files:**
- Modify: `src-tauri/src/github.rs`, `src-tauri/src/active.rs`(접근자 2개), `src-tauri/src/lib.rs`(명령 등록·`app.manage(github::GithubJobs::default())`)
- Test: `src-tauri/tests/github.rs`

**Interfaces:**
- Consumes: Task 1 `shell_run`·`sh_quote`·`shell_output`, Task 2 전부.
- Produces:
  ```rust
  // active.rs
  pub fn app_data(&self) -> &Path
  pub async fn account_dir(&self) -> Result<PathBuf, String>   // account_root(app_data, key), 로그인 전 NOT_SIGNED_IN
  // github.rs
  pub struct GhFound { pub path: PathBuf, pub version: (u32,u32,u32) }
  pub fn pick_gh(path_gh: Option<GhFound>, app_gh: Option<GhFound>) -> Result<GhFound, GithubStatus> // 없음→NotInstalled, 둘 다 낮음→Outdated{가장 높은 버전 문자열}
  pub fn find_gh(app_data: &Path) -> Result<GhFound, GithubStatus>  // PATH: shell_output("command -v gh && gh --version") 둘째 줄; 앱: <app_data>/bin/gh --version (같은 실행기로)
  pub fn gh_auth(gh: &Path) -> Result<GhAuth, String>               // shell_run(sh_quote(gh) + " auth status --json hosts --hostname github.com", 5s); code 1 → LoggedOut, 0 → parse
  pub fn read_record(dir: &Path) -> Option<String>                  // <dir>/github.json {"login": ".."}; 없거나 깨짐 → None
  pub fn write_record(dir: &Path, login: &str) -> Result<(), String> // 임시 파일 + rename
  pub fn clear_record(dir: &Path) -> Result<(), String>             // 없으면 Ok
  #[derive(Default)] pub struct GithubJobs { /* 진행 중 작업 1개: 종류 + cancel: Arc<AtomicBool> */ }
  #[tauri::command] pub async fn github_status(active) -> Result<GithubStatus, String>
  #[tauri::command] pub async fn github_disconnect(active) -> Result<GithubStatus, String>
  ```
  - 블로킹 부분은 `tauri::async_runtime::spawn_blocking`.
- [ ] **Step 1: 실패하는 테스트**
  - `pick_gh_prefers_new_enough`: PATH `(2,60,0)`, 앱 `(2,104,0)` → 앱 쪽. PATH `(2,103,3)`, 앱 `(2,104,0)` → PATH 쪽(충분하면 PATH 우선). 둘 다 None → `NotInstalled`. PATH `(2,60,0)`만 → `Outdated{version:"2.60.0"}`.
  - `gh_auth_with_fake_gh`: tempdir에 실행 스크립트 `gh` 작성 — `#!/bin/sh` 로 `$*`에 `auth status`면 고정 JSON 출력·exit 0 → `Accounts` 1개. exit 1 버전 → `LoggedOut`. exit 2 → Err.
  - `record_roundtrip`: write `hodoon` → read `Some("hodoon")` → clear → None, clear 두 번 Ok. 깨진 파일 → None.
  - `disconnect_keeps_gh`: 가짜 gh가 호출 인자를 파일에 남기게 하고 `clear_record` 경로(명령 본문이 쓰는 함수)를 돌린 뒤 그 파일에 `logout`이 없음.
- [ ] **Step 2:** FAIL 확인.
- [ ] **Step 3:** 구현. `github_status` = `find_gh` → `gh_auth` → `derive_status(read_record)`. `github_disconnect` = `clear_record` 후 상태 다시 읽기.
- [ ] **Step 4:** PASS + `cargo test --workspace` PASS.
- [ ] **Step 5: Commit** `feat: gh 찾기와 GitHub 연결 상태·해제 명령`

---

### Task 4: 연결 (이미 로그인 → 바로 기록 / 아니면 device 로그인)

**Files:**
- Modify: `src-tauri/src/github.rs`, `src-tauri/src/commands.rs`(`open_url`을 `pub(crate)`로), `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/github.rs`

**Interfaces:**
- Consumes: Task 3.
- Produces:
  ```rust
  pub const DEVICE_URL: &str = "https://github.com/login/device";
  pub const LOGIN_CMD_ARGS: &str = "auth login --web --hostname github.com --git-protocol https --skip-ssh-key";
  pub fn run_login(gh: &Path, cancel: &AtomicBool, code_wait: Duration, limit: Duration, on_code: &mut dyn FnMut(&str)) -> Result<(), String>
      // shell_run 콜백에서 parse_device_code → on_code 한 번. code_wait 안에 코드가 없으면 cancel 세우고 Err("gh 출력을 읽지 못했어요 (gh <버전>)").
      // 종료 0 → Ok, 취소 → Err("취소됨"), 그 밖 → Err(stderr 마지막 비지 않은 줄)
  #[tauri::command] pub async fn github_connect(app: AppHandle, active, jobs: State<GithubJobs>) -> Result<GithubStatus, String>
      // 1) find_gh 실패 → 그 상태 반환. 2) gh_auth 활성 계정 state=="success" → write_record(login) → 상태 반환(로그인 안 함).
      // 3) 아니면 jobs에 login 등록(이미 작업 중이면 Err("이미 진행 중이에요")) → run_login(code_wait 10s, limit 15m),
      //    코드는 app.emit("github-login-code", {code}) → 끝나면 gh_auth로 활성 login 기록 → 상태 반환.
  #[tauri::command] pub async fn github_cancel(jobs) -> Result<(), String>   // 진행 중 작업의 cancel = true
  #[tauri::command] pub async fn github_open_device() -> Result<(), String>  // open_url(DEVICE_URL)
  ```
- [ ] **Step 1: 실패하는 테스트** (가짜 gh 스크립트)
  - `login_emits_code_then_ok`: stderr에 `! One-time code (AB12-CD34) copied to clipboard`, 0.3초 뒤 exit 0 → `on_code("AB12-CD34")` 정확히 1회, Ok.
  - `login_no_code_fails_fast`: 아무것도 안 찍고 `sleep 30` → code_wait 300ms로 Err에 `gh 출력을 읽지 못했어요`, 2초 미만.
  - `login_cancel_kills`: 코드 찍고 `sleep 30`, 200ms 뒤 cancel → Err("취소됨"), 2초 미만.
  - `login_failure_reason`: 코드 찍고 stderr `error: device flow expired` 후 exit 1 → Err 문자열이 `error: device flow expired`.
  - `login_args_exact`: 가짜 gh가 받은 인자를 파일에 쓰게 해 `LOGIN_CMD_ARGS`와 같음.
- [ ] **Step 2:** FAIL. **Step 3:** 구현. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat: gh 로그인 코드로 GitHub 연결`

---

### Task 5: gh 설치·업데이트

**Files:**
- Modify: `src-tauri/src/github.rs`, `src-tauri/src/lib.rs`
- Test: `src-tauri/tests/github.rs`

**Interfaces:**
- Consumes: Task 1, 3.
- Produces:
  ```rust
  pub fn release_assets(release_json: &str, arch: &str) -> Result<(String /*tag*/, String /*zip url*/, String /*zip name*/, String /*checksums url*/), String>
      // arch: "aarch64"→"arm64", "x86_64"→"amd64"; 이름 gh_<tag에서 v뺀>_macOS_<arch>.zip, gh_<..>_checksums.txt
  pub fn checksum_for(checksums_txt: &str, file: &str) -> Option<String>   // "<sha256>  <file>" 줄
  pub fn install_from_zip(zip: &[u8], expected_sha256: &str, app_data: &Path) -> Result<PathBuf, String>
      // sha 불일치 → Err, 아무것도 안 씀. 맞으면 임시 폴더에 쓰고 /usr/bin/ditto -x -k 로 풀어 */bin/gh 를
      // <app_data>/bin/gh.tmp → chmod 755 → rename <app_data>/bin/gh. 임시 폴더는 항상 지움.
  pub enum InstallRoute { BrewInstall, BrewUpgrade, Release }
  pub fn choose_route(brew_prefix: Option<&Path>, current_gh: Option<&Path>) -> InstallRoute
      // brew 없음 → Release. gh 없음 → BrewInstall. gh가 brew_prefix 아래 → BrewUpgrade. 그 밖 → Release(기존 gh는 둔다)
  #[tauri::command] pub async fn github_install(app, active, jobs) -> Result<GithubStatus, String>
      // jobs에 install 등록, 10분·취소. brew 경로는 shell_run("brew install gh" | "brew upgrade gh"), stderr·stdout 줄은 app.emit("github-install-progress", {line}).
      // Release 경로는 reqwest(User-Agent "ieum-desktop")로 https://api.github.com/repos/cli/cli/releases/latest → zip·checksums 받기 → install_from_zip.
      // 끝나면 find_gh 로 2.81.0 이상인지 확인, 아니면 Err("설치했지만 gh가 2.81.0보다 낮아요"). 반환 = github_status 결과.
  ```
  - brew prefix는 `shell_output("brew --prefix", 5s)`.
- [ ] **Step 1: 실패하는 테스트**
  - `release_assets_pick_arch`: v2.104.0 자산 목록 JSON(설계 문서의 이름 4개 + browser_download_url) → aarch64는 `gh_2.104.0_macOS_arm64.zip`, x86_64는 amd64, 없는 arch → Err.
  - `checksum_lookup`: 두 줄 텍스트에서 이름으로 찾음, 없으면 None.
  - `install_bad_checksum_leaves_nothing`: 아무 바이트 + 틀린 sha → Err, `<app_data>/bin` 없음.
  - `install_from_zip_places_executable`: 테스트에서 `ditto -c -k --keepParent`로 `gh_9.9.9_macOS_arm64/bin/gh`(`#!/bin/sh\necho gh version 9.9.9`) zip을 만들고 올바른 sha로 호출 → `<app_data>/bin/gh` 실행 가능, `--version` 출력 일치, `gh.tmp` 없음.
  - `choose_route_cases`: 위 네 경우.
- [ ] **Step 2:** FAIL. **Step 3:** 구현. **Step 4:** PASS.
- [ ] **Step 5: Commit** `feat: gh가 없거나 낮으면 brew 또는 공식 릴리스로 설치`

---

### Task 6: 설정 > 연동 화면

**Files:**
- Create: `src/screens/settings/IntegrationsPane.tsx`, `src/screens/settings/IntegrationsPane.test.tsx`
- Modify: `src/api.ts`, `src/screens/settings/SettingsShell.tsx`(`SettingsSection`에 `"integrations"`, `SECTIONS`에 묶음 `settings.groupIntegrations`·메뉴 아이콘 lucide `Plug`·`texts`·`panes`. lucide 1.71엔 GitHub 아이콘이 없으니 줄 안 GitHub 마크는 IntegrationsPane 안 인라인 SVG), `src/i18n.ts`(ko·en)

**Interfaces:**
- Consumes: Task 3~5 명령·이벤트.
- Produces (`api.ts`):
  ```ts
  export type GithubStatus =
    | { kind: "notInstalled" } | { kind: "outdated"; version: string } | { kind: "notConnected" }
    | { kind: "needsLogin"; recorded: string } | { kind: "accountChanged"; recorded: string; current: string }
    | { kind: "connected"; login: string; envToken: boolean };
  githubStatus: () => invoke<GithubStatus>("github_status"),
  githubConnect: () => invoke<GithubStatus>("github_connect"),
  githubDisconnect: () => invoke<GithubStatus>("github_disconnect"),
  githubInstall: () => invoke<GithubStatus>("github_install"),
  githubCancel: () => invoke<void>("github_cancel"),
  githubOpenDevice: () => invoke<void>("github_open_device"),
  onGithubLoginCode: (cb: (p: { code: string }) => void) => listen("github-login-code", …),
  onGithubInstallProgress: (cb: (p: { line: string }) => void) => listen("github-install-progress", …),
  export function integrationsTexts(): string[]   // 검색용 문구(IntegrationsPane.tsx에서 export)
  ```
- 화면 문구(ko, 그대로): 설명 "PR·이슈 노드는 gh CLI로 실행돼요." / 미설치 "GitHub을 쓰려면 gh CLI가 필요해요. 설치할까요?" [설치] [다시 확인] / 낮음 "gh {version}는 너무 오래됐어요(2.81.0 이상 필요)." [업데이트] [다시 확인] / 미연결 [연결] / 코드 카드 "일회용 코드" {code} [복사] [브라우저 열기] "브라우저에서 코드를 입력하면 자동으로 연결돼요." [취소] / 연결됨 {login} 배지 "연결됨" [연결 해제] / 환경변수 "셸 환경변수 토큰을 쓰는 중이에요." / 계정 바뀜 "gh 계정이 {current}(으)로 바뀌었어요." [이 계정으로 연결] [연결 해제] / 다시 로그인 "gh 로그인이 필요해요." [연결] / 진행 중 "설치하는 중…" [취소], 실패 시 이유 한 줄 + [다시 시도] + 진행 줄 마지막 20줄 펼쳐 보기. en은 같은 뜻으로.
- [ ] **Step 1: 실패하는 테스트** (`api` 모킹, 기존 `SettingsShell.test.tsx` 방식)
  - `shows_install_when_missing`: `githubStatus`→notInstalled → 안내 문구·[설치]; 클릭 → `githubInstall` 호출, 진행 중 [취소] 보임.
  - `connect_shows_code_card`: notConnected → [연결] 클릭 → `githubConnect`가 대기 중일 때 `onGithubLoginCode` 콜백으로 `AB12-CD34` → 카드에 코드; [브라우저 열기] → `githubOpenDevice`; 연결 promise가 connected로 끝나면 `hodoon`·"연결됨".
  - `disconnect_calls_api_only`: connected → [연결 해제] → `githubDisconnect` 1회, 확인 카드 없음.
  - `env_token_line`: connected envToken true → 환경변수 문구.
  - `account_changed`: accountChanged → 문구에 `other`, [이 계정으로 연결] → `githubConnect`.
  - `unmount_cancels`: 연결 진행 중 언마운트 → `githubCancel` 1회.
  - `settings_menu_has_integrations`: `SettingsShell` initial `integrations` 렌더 → 메뉴 "연동" 보임, 검색 "gh" 로 메뉴 남음.
- [ ] **Step 2:** `npx vitest run src/screens/settings/IntegrationsPane.test.tsx` → FAIL.
- [ ] **Step 3:** 구현. 색·카드 모양은 `AiPane`의 줄·배지 스타일 재사용.
- [ ] **Step 4:** PASS + `npm test`·`npx tsc --noEmit` PASS.
- [ ] **Step 5: Commit** `feat: 설정에 연동 메뉴와 GitHub 연결 줄 추가`

---

### Task 7: 전체 확인 + 수동 끝-끝

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm test`, `npx tsc --noEmit` 모두 PASS.
- [ ] `find crates src-tauri -name 'zz_probe*' -delete`, `git status`로 임시 파일 없음 확인.
- [ ] 수동(사용자와): 설치된 IEUM 종료 → `npm run tauri dev` → 설정 > 연동: (a) 지금 Mac(gh 2.103.3, hodoon 로그인)에서 [연결] → 코드 없이 연결됨 (b) [연결 해제] 후 터미널 `gh auth status` 여전히 로그인 (c) 로그인 카드 흐름은 `GH_CONFIG_DIR`을 임시 폴더로 둔 셸 설정으로 확인 또는 사용자 판단으로 생략.
