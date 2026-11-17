# 스케줄·수동 실행 트리거 (1b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. (이 레포는 /orca-sdd로 실행 — 워커는 커밋하지 않고 코디네이터가 커밋한다.)

**Goal:** 데스크톱에서 `schedule.*` 5개·`manual.run` 트리거를 돌리고, 놓친 차례는 2시간 유예 안이면 깨어난 뒤 한 번 실행한다.

**Architecture:** cron 계산·차례 판정은 `ieum-engine`의 순수 함수(`schedule.rs`)로 두고 BE 대조표로 고정한다. 앱 감독자(`supervisor.rs`)는 기존 30초 재확인 루프에서 그 판정을 불러 큐에 넣고, 차례 기록(`last_slot`)을 계정 저장소에 둔다. 트리거 종류는 `TriggerKind` 하나로 모아 폴더 가정을 걷어낸다.

**Tech Stack:** Rust(ieum-engine·ieum-local·src-tauri, chrono·chrono-tz 기존 + cron 크레이트 신규), React 19 + TS, vitest.

**Spec:** `docs/superpowers/specs/2026-11-16-ieum-desktop-schedule-triggers-design.md`

## Global Constraints
- 이슈 #7, 브랜치 `feat/7-schedule-triggers`, PR base `dev`.
- 시간대 **Asia/Seoul** 고정(Mac 시간대와 무관). 유예 **2시간**. 루프 간격은 기존 `Deps.recheck`(운영 30초).
- 칸→cron, 5필드→Quartz 규칙은 BE `ScheduleCron.of`·`CronConverter.toQuartz`와 같다(설계 "정한 값" 절 그대로).
- 트리거 출력: schedule = `{"triggeredAt": "YYYY-MM-DDTHH:MM:SS"}`(Asia/Seoul 로컬, 오프셋 없음, 실제 실행 시각) + `schedule.every`만 `"cron": <원문>`. manual = `{}`.
- 정적 검증: `config.cron`만 `CRON`(field `config.cron`). hour·weekday·day 범위 밖은 기존 choices 검사(`CHOICE`).
- 채팅 빌더 메뉴에 `schedule.every` 없음.
- 커밋: 한국어 `feat: …`/`fix: …`, 본문 `이슈 #7`, Co-Authored-By 없음. Rust는 perl·sed로 고치지 않는다, `cargo fmt --all`.
- 새 Tauri 명령·인자 변경은 `src-tauri/build.rs` COMMANDS와 `capabilities/default.json`에도(`tests/command_registry.rs`가 잡는다).

## Review Focus
1. Mac이 3시간 자는 동안 hourly 차례 3개를 놓침 → 깬 뒤 **정확히 한 번**(가장 최근 차례가 2시간 안이므로), 5시간 자서 모든 차례가 2시간 밖이면 0번 → Task 1 `decide_*` 테스트 + Task 3 `missed_*` 테스트.
2. 실행 직후 앱 재시작 → 같은 차례 다시 안 돎(`last_slot` 저장) → Task 3 `restart_no_duplicate`.
3. `schedule.every`의 `0 9 * * 5`(Unix 금요일)가 금요일 9시 KST에 돎(목요일 아님) → Task 1 `every_unix_weekday_friday`.
4. Mac 시간대가 서울이 아니어도(`TZ=America/New_York`) 9시 KST에 돎 → Task 1은 고정 시각(UTC 인스턴트) 입력만 쓰므로 Task 1 `daily_is_kst_not_local`.
5. 폴더 워크플로우 [실행]은 여전히 파일을 요구, 스케줄·수동 워크플로우 [실행]은 파일 없이 → Task 4 `run_now_by_kind`.

---

### Task 1: 엔진 스케줄 계산 (`schedule.rs`)

**Files:**
- Create: `crates/ieum-engine/src/schedule.rs`, `crates/ieum-engine/tests/schedule.rs`
- Modify: `crates/ieum-engine/src/lib.rs`(`pub mod schedule;`), `crates/ieum-engine/Cargo.toml`(cron 크레이트)

**Interfaces:**
- Produces:
  ```rust
  pub const SCHEDULE_ZONE: chrono_tz::Tz = chrono_tz::Asia::Seoul;
  pub const GRACE: chrono::Duration = chrono::Duration::hours(2);
  #[derive(Debug, PartialEq)] pub struct CronError(pub String);       // 사용자에게 보일 한국어 이유
  pub fn schedule_cron(event: &str, config: &Map<String, Value>) -> Result<String, CronError>
      // BE ScheduleCron.of: hourly/daily/weekly/monthly는 칸으로 Quartz 6필드, every는 config.cron 원문(빈 값/비문자열 → Err)
  pub fn to_quartz(cron: &str) -> Result<String, CronError>             // BE CronConverter.toQuartz
  pub struct Schedule { /* 파싱된 Quartz 식 */ }
  impl Schedule {
      pub fn parse(quartz: &str) -> Result<Schedule, CronError>
      pub fn next_after(&self, t: DateTime<Utc>) -> Option<DateTime<Utc>>   // t 초과 첫 차례
      pub fn latest_at_or_before(&self, after: DateTime<Utc>, now: DateTime<Utc>) -> Option<DateTime<Utc>> // (after, now] 마지막 차례
  }
  pub enum Decision { Nothing, Run { slot: DateTime<Utc> }, Expired { slot: DateTime<Utc> }, ClockBack }
  pub fn decide(s: &Schedule, last_slot: DateTime<Utc>, now: DateTime<Utc>) -> Decision
      // now < last_slot → ClockBack; (last_slot, now] 마지막 차례 없음 → Nothing; now - slot <= GRACE → Run, 아니면 Expired
  pub fn triggered_at(now: DateTime<Utc>) -> String                     // Asia/Seoul 로컬 "%Y-%m-%dT%H:%M:%S"
  pub fn schedule_output(event: &str, config: &Map<String, Value>, now: DateTime<Utc>) -> Map<String, Value>
  ```
- 크레이트: Quartz 문법(초 필드, `?`, `L`, `W`, `#`, 요일 이름, 7필드 연도)을 Quartz 뜻 그대로 계산해야 한다. 후보 `croner` — 단 숫자 요일이 Unix 기준(0=일)이라 Quartz 숫자(1=일)를 넘기기 전에 이름으로 바꾸는 변환을 `Schedule::parse` 안에 둔다. 아래 테스트가 크레이트 선택의 합격선이다.

- [ ] **Step 1: 실패하는 테스트** (`tests/schedule.rs`, BE 코드에서 옮긴 대조표)
  - `fields_to_cron_matches_be`: hourly `{}`→`"0 0 * * * ?"`; hourly `{"weekends":false}`→`"0 0 * ? * MON-FRI"`; hourly `{"weekends":"false"}`→같음; daily `{"hour":"9"}`→`"0 0 9 * * ?"`; daily `{"hour":"9","weekends":false}`→`"0 0 9 ? * MON-FRI"`; weekly `{"weekday":"MON","hour":"9"}`→`"0 0 9 ? * MON"`; monthly `{"day":"L","hour":"23"}`→`"0 0 23 L * ?"`; every `{"cron":"0 9 * * 5"}`→원문 `"0 9 * * 5"`.
  - `fields_out_of_range_err`: daily hour `"24"`, weekly weekday `"MONDAY"`, monthly day `"29"` → Err.
  - `to_quartz_matches_be`: `"0 9 * * *"`→`"0 0 9 * * ?"`; `"*/5 * * * *"`→`"0 */5 * * * ?"`; `"0 9 * * 1-5"`→요일 Quartz `2-6`; `"0 9 1 * 1"`→Err(일·요일 동시); `"0 0 9 ? * MON-FRI"`→그대로; 4필드→Err.
  - `every_unix_weekday_friday`: `Schedule::parse(&to_quartz("0 9 * * 5")?)` 의 `next_after(2026-11-16T00:00Z /*월 09:00 KST*/)` → `2026-11-20T00:00Z`(금 09:00 KST).
  - `daily_is_kst_not_local`: daily 9시 `next_after(2026-11-16T01:00Z)` → `2026-11-17T00:00Z`. (테스트 프로세스 TZ와 무관해야 한다 — `std::env::set_var("TZ","America/New_York")` 후에도 같은 값.)
  - `quartz_features`: `"0 0 9 L * ?"` 11월 → 30일 09:00 KST; `"0 0 9 ? * 6#3"` → 그 달 셋째 금요일; 7필드 `"0 0 9 * * ? 2027"` → 2027년 첫 날.
  - `decide_cases` (hourly): last 09:00 KST, now 09:30 → `Nothing`; now 12:10(10·11·12시 놓침) → `Run{12:00}`; last 09:00, now 14:30, 차례 10~14시 → `Run{14:00}`(마지막이 2시간 안); hourly 대신 daily 9시로 last 어제 9시, now 오늘 11:01 → `Expired{오늘 9시}`; now 오늘 11:00 → `Run`(경계 포함); now < last → `ClockBack`.
  - `output_shape`: daily → `{"triggeredAt":"2026-11-17T09:00:00"}`(now=2026-11-17T00:00Z); every → `triggeredAt` + `"cron":"0 9 * * 5"`.
- [ ] **Step 2:** `cargo test -p ieum-engine --test schedule` → FAIL(컴파일).
- [ ] **Step 3:** 구현.
- [ ] **Step 4:** PASS, `cargo test -p ieum-engine` PASS.
- [ ] **Step 5: Commit** `feat: 스케줄 칸·cron을 클라우드와 같은 규칙으로 계산`

---

### Task 2: 카탈로그 로드 + `CRON` 검증

**Files:**
- Modify: `crates/ieum-engine/src/catalog.rs`(`embedded()`에 `schedule/{every,hourly,daily,weekly,monthly}.json`·`manual/run.json`, `stage1`의 폴더 목록에 `schedule`·`manual`), `crates/ieum-engine/src/validate.rs`(Code에 `Cron`, 직렬화 `"CRON"`)
- Test: `crates/ieum-engine/tests/` 기존 검증 테스트 파일에 추가(없으면 `tests/validate_schedule.rs`)

**Interfaces:**
- Consumes: Task 1 `schedule_cron`, `to_quartz`, `Schedule::parse`.
- Produces: `validate()`가 SCHEDULE 트리거 노드의 `config.cron`(event `schedule.every`)에 대해 `CRON` 에러(nodeId, field `config.cron`)를 낸다 — 비문자열, 필드 수 5·6·7 아님, 문자 집합 `[0-9A-Za-z*/,\-?LW#]` 밖, 또는 `to_quartz`→`Schedule::parse` 실패. 빈 값은 기존 REQUIRED 몫(CRON 안 냄).
- [ ] **Step 1: 실패하는 테스트**
  - `catalog_has_schedule_and_manual`: `Catalog::embedded()`가 6개 event를 알고 node_type TRIGGER.
  - `cron_invalid`: every `"0 9 * *"`·`"0 9 1 * 1"`·`"abc def ghi jkl mno"`·`42` → 각각 code `CRON`, field `config.cron`. `"0 9 * * 1-5"` → 에러 없음. `""` → CRON 없음.
  - `daily_hour_choice`: daily hour `"24"` → `validate_fields`가 `CHOICE`(field `config.hour`), `validate`엔 CRON 없음.
  - 기존 conformance 22개 통과 유지: `cargo run -p ieum-cli --bin ieum-dev -- conformance conformance/`.
- [ ] **Step 2:** FAIL. **Step 3:** 구현. **Step 4:** PASS + `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat: 스케줄·수동 트리거 정의를 읽고 cron을 검증`

---

### Task 3: 감독자 스케줄 루프

**Files:**
- Modify: `src-tauri/src/supervisor.rs`, `src-tauri/src/store.rs`, `src-tauri/src/lib.rs`(Deps 생성부), `src-tauri/src/notify.rs`
- Test: `src-tauri/tests/supervisor.rs`(기존 하네스에 추가), `src-tauri/tests/notify.rs`

**Interfaces:**
- Consumes: Task 1 전부, Task 2 카탈로그.
- Produces:
  ```rust
  // supervisor.rs
  pub enum TriggerKind { Folder(PathBuf), Schedule { event: String, quartz: Result<String, String> }, Manual, Unsupported }
  pub fn trigger_kind(wf: &StoredWorkflow) -> TriggerKind          // trigger_folder 대체(첫 TRIGGER 노드 event로 판정)
  // Deps 새 필드
  pub now: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>          // 운영 = Utc::now, 테스트 = 가짜 시계
  // Supervisor
  fn schedule_tick(&self)                                         // recheck 루프·sync 끝에서 호출
  // Job.folder 대신
  enum JobSource { Watch(PathBuf), Schedule { slot: DateTime<Utc> }, Manual, Test }
  // WorkflowEntry 새 필드 (camelCase)
  pub next_run: Option<String>                                    // Asia/Seoul "YYYY-MM-DDTHH:MM:SS", 꺼짐·비스케줄이면 None
  // store.rs
  pub enum RunTrigger { Watch, Manual, Test, Schedule }           // 직렬화 "schedule"
  StoredRun 새 필드 `#[serde(default)] pub scheduled_for: Option<String>`  // 스케줄 실행의 차례(KST 로컬)
  pub fn get_schedule_slots(&self) -> Result<HashMap<String, DateTime<Utc>>, String>   // 계정 저장소 schedule.json
  pub fn set_schedule_slot(&self, workflow_id: &str, slot: DateTime<Utc>) -> Result<(), String>
  ```
- 동작(설계 "스케줄 루프"·"겹침"·"잘못된 스케줄"·"계정" 절 그대로):
  - 켜진 Schedule 워크플로우마다 `last_slot` 없으면 `now` 저장 후 끝. 있으면 `decide`: `Run` → 그 워크플로우 줄이 비어 있고(실행 중·대기 없음) 실행 → `push(Job{source: Schedule{slot}, trigger: schedule_output(..now)})`, 아니면 건너뜀(eprintln 로그만); `Run`·`Expired` 모두 `set_schedule_slot(slot)`; `ClockBack` → `set_schedule_slot(now)`.
  - 저장 때 트리거 event·칸이 바뀌었거나, 꺼짐→켜짐이면 `set_schedule_slot(now)`.
  - `quartz`가 Err이면 돌지 않고 `WatchState::WatchError` + `watch_message = "스케줄을 계산하지 못했어요: <이유>"`(기존 `emit_status` 경로).
  - `run_one`: `RunTrigger`는 `JobSource`로(Schedule → `RunTrigger::Schedule`, `scheduled_for` 기록). Watch 늦은 트리거 버리기는 Watch만.
  - notify: `Schedule` 끝 알림은 `Watch`와 같은 설정(`notify_watch_end`)·조건을 따른다. 대시보드 집계도 Watch와 같게(Test만 제외 유지).
  - `QueueDropped`의 `trigger["path"]`는 없으면 빈 문자열(현재 동작 유지).
- [ ] **Step 1: 실패하는 테스트** (가짜 시계 `Arc<Mutex<DateTime<Utc>>>` + 가짜 AI 하네스, `recheck`는 짧게, `schedule_tick`은 테스트에서 직접 부를 수 있게 `pub(crate)` 대신 `#[doc(hidden)] pub fn tick_for_test`를 허용)
  - `first_enable_no_backfill`: hourly 켬, 시계 10:30 → tick → 실행 0, slot=10:30.
  - `runs_on_slot`: 이어서 11:00:10 → tick → 실행 1(`RunTrigger::Schedule`, `scheduled_for` "…T11:00:00", 출력 `triggeredAt` "…T11:00:10").
  - `missed_within_grace_once`: 시계 14:30으로 점프 → tick → 실행 정확히 1, 두 번째 tick → 0.
  - `missed_beyond_grace_none`: daily 9시, slot 어제 9시, 시계 오늘 11:01 → 0 실행, 다음 tick도 0, 내일 9시 → 1.
  - `restart_no_duplicate`: 실행 뒤 Supervisor를 stop/start(같은 store) 같은 시각 tick → 0.
  - `overlap_skips`: 가짜 AI가 첫 실행을 막아 둔 채 다음 차례 tick → 큐에 안 들어감, slot은 넘어감.
  - `reenable_starts_now`: 끔 → 시계 +5시간 → 켬 → tick → 0.
  - `bad_cron_watch_error`: every `"0 9 1 * 1"` 켬 → `list_workflows`의 watch `watch_error`, 메시지 접두 "스케줄을 계산하지 못했어요".
  - `next_run_listed`: daily 9시 켜짐 → `next_run` "…T09:00:00".
  - `notify_schedule_like_watch` (`tests/notify.rs`): Schedule 끝은 `notify_watch_end` false면 안 울림, true면 울림.
- [ ] **Step 2:** FAIL. **Step 3:** 구현. **Step 4:** PASS + `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat: 30초 루프에서 스케줄 차례를 돌리고 놓친 차례는 2시간 안이면 한 번`

---

### Task 4: 수동 실행·노드 테스트를 트리거 종류별로

**Files:**
- Modify: `src-tauri/src/supervisor.rs`(`run_now`, `run_test`), `src-tauri/src/commands.rs`(`run_now` 인자), `src-tauri/src/builder_host.rs`, `crates/ieum-local/src/builder/test.rs`(+ Host 트레이트 정의 위치), `crates/ieum-local/src/paths.rs`(`user_folders` 비폴더 트리거)
- Test: `src-tauri/tests/supervisor.rs`, `crates/ieum-local/tests/` 빌더 테스트 파일

**Interfaces:**
- Consumes: Task 3 `trigger_kind`, `JobSource`; Task 1 `schedule_output`.
- Produces:
  ```rust
  pub enum TestTrigger { File(PathBuf), Output(Map<String, Value>) }
  pub async fn run_now(&self, workflow_id: &str, file: Option<&Path>) -> Result<String, String>
      // Folder → file 필수(없으면 Err "파일을 골라 주세요"), Schedule → schedule_output(now), Manual → {}; Unsupported → Err
  pub async fn run_test(&self, workflow_id: &str, wf: Workflow, trigger: TestTrigger) -> Result<TestRun, String>
  // 빌더 Host 트레이트의 run_test도 TestTrigger를 받는다.
  // builder test.rs: 폴더 트리거면 기존 pick_sample, 스케줄·수동이면 TestTrigger::Output(schedule_output 또는 {}) — FolderMissing 안 냄.
  // commands.rs: run_now(id: String, file_path: Option<String>)
  ```
- [ ] **Step 1: 실패하는 테스트**
  - `run_now_by_kind`: 폴더 워크플로우 `run_now(id, None)` → Err "파일을 골라 주세요"; 파일 있으면 기존대로. daily 워크플로우 `run_now(id, None)` → 실행 기록 `RunTrigger::Manual`, 트리거 출력에 `triggeredAt`. manual.run → 출력 `{}`.
  - `node_test_schedule_no_folder`(빌더): schedule.daily → AI 노드 그래프 테스트가 `FolderMissing` 없이 돈다, 트리거 출력 `triggeredAt` 있음.
  - `user_folders_non_folder`: 스케줄 워크플로우의 허용 폴더 목록은 다른 노드(파일 이동 등)에서만 온다(트리거 폴더 없음).
- [ ] **Step 2:** FAIL. **Step 3:** 구현. **Step 4:** PASS + `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat: 스케줄·수동 워크플로우는 파일 없이 실행·테스트`

---

### Task 5: 채팅 빌더 트리거 규칙·안내

**Files:**
- Modify: `crates/ieum-local/src/builder/{finish.rs,edit.rs,reply.rs,guide.rs}`, `crates/ieum-local/src/chat/prompt.rs`
- Test: 해당 빌더 테스트 파일

**Interfaces:**
- Produces:
  - `TriggerRule` 문구: "첫 노드는 트리거 하나여야 해요(폴더에 새 파일·스케줄·수동 실행)."
  - 빌더가 고를 수 있는 트리거: `local.folder_new_file`, `schedule.hourly|daily|weekly|monthly`, `manual.run`. **`schedule.every`는 빌더 메뉴·안내에 없다**(편집기 전용) — 빌더가 `schedule.every`를 넣으면 `EventUnknown`과 같은 거절.
  - `PendingFinish.folder`는 `Option`(폴더 트리거만). 수동 워크플로우 완료 확인은 버튼 종류 `save`(화면이 [저장]으로 그림), 그 밖은 기존 `enable`. 저장 시 `enabled = true`.
  - `FOLDER_FIELDS`는 폴더 트리거에만 적용.
  - 안내문(`guide.rs`·`prompt.rs`): 스케줄 칸(hour 문자열 "0"~"23", weekday MON~SUN, day "1"~"28"|"L", weekends 불리언, Asia/Seoul)과 수동 실행 설명. "첫 노드는 항상 local.folder_new_file" 문장 삭제.
- [ ] **Step 1: 실패하는 테스트**: 빌더가 `schedule.daily` 첫 노드 그래프를 받아들임; `schedule.every`는 거절; `manual.run` 워크플로우 finish 확인 버튼 종류 `save`; 폴더 워크플로우는 여전히 `enable`·폴더 필수; 프롬프트 문자열에 `schedule.every` 없음·`schedule.daily` 있음.
- [ ] **Step 2:** FAIL. **Step 3:** 구현. **Step 4:** PASS + `cargo test --workspace`.
- [ ] **Step 5: Commit** `feat: 채팅 빌더가 스케줄·수동 트리거를 만든다`

---

### Task 6: 화면

**Files:**
- Modify: `src/nodes/defs.ts`(6개 event), `src/list/WorkflowCard.tsx`, `src/editor/useTestRun.ts`, `src/api.ts`(`runNow(id, filePath?: string)`, `WorkflowEntry.nextRun`, `RunTrigger`에 `"schedule"`, 기록 `scheduledFor`), 실행 기록 화면(`src/screens/History.tsx`·`RunView.tsx`), 채팅 완료 확인 카드(버튼 종류 `save`), `src/i18n.ts`
- Test: 각 화면 `*.test.tsx`

**Interfaces:**
- Consumes: Task 3 `nextRun`·`RunTrigger::Schedule`·`scheduledFor`, Task 4 `run_now` 인자, Task 5 finish 버튼 종류.
- Produces(문구 ko, en 같은 뜻):
  - 카드 요약: hourly "매시 정각"(weekends false면 "평일 매시 정각"), daily "매일 {h}시"/"평일 매일 {h}시", weekly "매주 {요일} {h}시", monthly "매월 {d}일 {h}시"/day L "매월 마지막 날 {h}시", every "cron {원문}". 시각은 카탈로그 choice 이름(예: "오전 9시"). 아래 줄 "다음 {M}/{D} {HH}:{MM}"(nextRun 있을 때).
  - manual.run 카드: 켜기 스위치 없음, [실행]만.
  - [실행]: 폴더 → 파일 고르기(지금 그대로), 스케줄·수동 → 바로 `runNow(id)`.
  - 기록: trigger schedule은 "스케줄", `scheduledFor`와 시작 시각이 1분 넘게 다르면 "예정 {HH:MM}, 늦게 실행".
  - 완료 확인 카드: 버튼 종류 save면 [저장].
- [ ] **Step 1: 실패하는 테스트**: 카드 요약 문구 6종; manual 카드 스위치 없음; 스케줄 카드 [실행]이 파일 선택 없이 `runNow(id)` 호출; 폴더 카드는 파일 선택; 기록 "예정 09:00, 늦게 실행"; 완료 카드 [저장].
- [ ] **Step 2:** `npx vitest run` 해당 파일 FAIL. **Step 3:** 구현. **Step 4:** `npm test`·`npx tsc --noEmit` PASS.
- [ ] **Step 5: Commit** `feat: 스케줄·수동 워크플로우 카드·실행·기록 화면`

---

### Task 7: 문서·전체 확인·수동 확인
- [ ] `conformance/GAPS.md`에 클라우드와 다른 점: 놓친 차례 2시간 유예 실행(클라우드는 건너뜀), 앱이 켜져 있을 때만, 활성 계정만.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, conformance, `npm test`, `npx tsc --noEmit`.
- [ ] `find crates src-tauri -name 'zz_probe*' -delete`, `git status` 깨끗.
- [ ] 수동(사용자와): 설치된 IEUM 종료 → `npm run tauri dev` → schedule.every `*/2 * * * *` 워크플로우 켜고 2분 안에 실행 기록 생김, [실행] 파일 없이 돎, manual.run 카드 스위치 없음.
