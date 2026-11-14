# 기본 계정 바꾸기·한도 도달 계정 권하기 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 사용량 팝오버에서 계정을 눌러 기본 계정을 바꾸고, 기본 계정이 한도에 닿으면 같은 회사 다른 계정을 권한다(앱 안 띠 + macOS 알림). 구독 조회는 Rust 백그라운드 감시로 옮겨 5분/2분 주기로 줄이고, Claude 모델별 창·Antigravity 그룹별 창을 보여 준다.

**Architecture:** Rust `limit_watch.rs`가 판정(순수 함수)과 감시 루프를 맡고, 결과를 `plans-updated`·`account-suggest` 이벤트로 화면에 보낸다. 화면은 이벤트로 `usePlanStore`를 채우고, 확인 창 하나(`SwitchAccountDialog`)를 팝오버와 알림 띠가 같이 쓴다.

**Tech Stack:** Tauri 2, Rust(tokio, reqwest, serde_json, tauri-plugin-notification), React 19 + TS + Zustand + Tailwind 4, vitest, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-13-ieum-desktop-account-switch-design.md`

## Global Constraints

- 계정 전환·권하기 대상은 **Claude·Codex만**. Antigravity는 표시(창 라벨)만.
- 자동 전환 금지 — 언제나 사용자가 [바꾸기].
- 감시 주기: 판정 대상 창 최댓값 **≥ 80%인 기본 계정이 하나라도 있으면 120초, 아니면 300초**. 백엔드 캐시 TTL 60초·429 대기(60초~15분)는 그대로.
- 한도 판정 대상 창(`gating`): Claude `five_hour`·`seven_day`, Codex `session`·`weekly`·`monthly`. 모델 전용(`seven_day_opus`·`seven_day_model`)은 표시만.
- 한도 도달 = 판정 대상 창 `usedPct >= 100`. 권할 수 있는 계정 = 판정 대상 창이 모두 `< 100`.
- 같은 제안은 키 `(회사, 기본 계정 id, 창 kind, 그 창 resetsAt)`당 한 번(메모리). "나중에"도 같은 키로 기억.
- **백그라운드(감시 루프)는 키체인 허용 창을 띄우지 않는다**: 이번 실행에서 토큰을 한 번이라도 `Found`로 읽은 Claude 계정·Antigravity 로그인만 조회.
- 알림 문구에 이메일 금지 — 계정 이름표(`label`, 기본 계정 `system`은 "이 Mac의 로그인")만.
- 새 앱 명령은 `src-tauri/build.rs` `COMMANDS`와 `src-tauri/capabilities/default.json` `allow-<명령>` 둘 다에 넣는다.
- 새 설정 `notifyAccountSuggest`(Rust `notify_account_suggest`) 기본 true — `Settings`는 struct 수준 `#[serde(default)]`라 `Default`에서 true로 두면 옛 설정도 true.
- 문구는 `src/i18n.ts` ko·en 둘 다. 커밋 메시지 `fix: …`/`feat: …` 한국어, `Co-Authored-By` 없음.

## Review Focus

1. **로그아웃·계정 전환 중 감시 루프** — 이음 로그인이 없으면 조회하지 않고, 다른 이음 계정으로 바뀌면 그 계정의 자격 목록으로 다음 회차부터 조회. (Task 4 테스트: 자격을 못 읽으면 `tick`이 조회 함수를 부르지 않는다)
2. **기본 계정이 바뀐 직후 같은 제안 재등장** — 사용자가 [바꾸기]로 전환하면 새 기본 계정이 판정 대상이 되고, 옛 키로는 다시 권하지 않는다. (Task 2 테스트 `suggest_not_repeated_for_same_key`)
3. **다른 계정이 아직 키체인 허용 전** — 후보에서 빠지고 권하기 자체가 없다(허용 창 안 뜸). (Task 3 테스트 `quiet_get_skips_unread_account`)
4. **창이 초기화돼 다시 100%** — `resetsAt`이 달라졌으므로 새 키로 한 번 더 권한다. (Task 2 테스트 `suggest_again_after_reset`)
5. **알림 꺼짐** — `notifyAccountSuggest=false`면 띠·시스템 알림 둘 다 없음, 하지만 `plans-updated`는 계속. (Task 4 테스트 `route_suggestion_respects_toggle`)

---

### Task 1: 창 라벨 — Claude 모델별 주간 창, Antigravity 그룹별 창

**Files:**
- Modify: `src-tauri/src/plan_usage.rs` (`PlanWindow`, `parse`)
- Modify: `src-tauri/src/sub_usage.rs` (`parse_agy`, `parse_codex` — 생성자에 `label: None`)
- Modify: `src/api.ts` (`PlanWindow`), `src/usage/parts.tsx` (`WindowMeter` 이름)
- Test: `src-tauri/tests/plan_usage.rs`, `src-tauri/tests/sub_usage.rs`, `src/dashboard/UsageCard.test.tsx`

**Interfaces:**
- Produces: `PlanWindow { kind: &'static str, used_pct: f64, resets_at: String, #[serde(skip_serializing_if = "Option::is_none")] label: Option<String> }`. 새 kind `"seven_day_model"`. TS: `kind: … | "seven_day_model"; label?: string`.
- Produces: TS `windowName(w: PlanWindow): string` in `src/usage/parts.tsx` = `w.label ?? t(\`usage.window.${w.kind}\`)` — 다른 곳(상태 줄 툴팁 등)도 이걸 쓴다.

- [ ] **Step 1: 실패 테스트 — Claude `limits`의 Fable**

`tests/plan_usage.rs` `plan_usage_reads_model_scoped_weekly`: 응답 `sample()`에 `"limits": [{"kind":"weekly_scoped","percent":7.0,"resets_at":"2026-11-16T03:00:00+00:00","scope":{"model":{"display_name":"Fable"}}}, {"kind":"weekly","percent":3.0}]` 추가 → 창 목록 끝에 `{kind:"seven_day_model", label:Some("Fable"), used_pct:7.0, resets_at:"2026-11-16T03:00:00+00:00"}` 하나. `plan_usage_reads_legacy_fable_key`: `limits` 없이 `"seven_day_fable": {"utilization": 12.0, "resets_at": "2026-11-16T03:00:00+00:00"}` → 같은 모양 label "Fable", used 12. 둘 다 있으면 Fable 하나만.

- [ ] **Step 2: 실패 테스트 — Antigravity 그룹별**

`tests/sub_usage.rs` `antigravity_windows_per_group`: 2026-11-13 실측 모양(설계 4절) — 그룹 "Gemini Models"(weekly rf 0.9976, 5h rf 0.9953), "Claude and GPT models"(weekly 1, 5h 1), "Gemini 4 Argon"(버킷 하나 `name:"Gemini 4 Argon Weekly Limit"`, weekly rf 0) → 창 5개, 순서 = 응답 순서, label `"Gemini Models · Weekly Limit Remaining"` … Argon은 `"Gemini 4 Argon Weekly Limit"`(버킷 하나라 그룹명 생략), Argon used 100, Gemini weekly used 0. 기존 `antigravity_usage_parses_windows`의 기대값도 그룹별로 고친다.

- [ ] **Step 3: 테스트 실패 확인**

Run: `cd src-tauri && cargo test --test plan_usage --test sub_usage`
Expected: 위 3개 FAIL.

- [ ] **Step 4: 구현**

`parse`: 기존 세 키 다음에 `limits[]`에서 `kind=="weekly_scoped"`·`percent` 숫자·`resets_at` RFC 3339·`scope.model.display_name` 비지 않은 것마다 창 추가. Fable이 없으면 `seven_day_fable`→`fable_weekly`→`fable_seven_day` 순으로 `{utilization, resets_at}` 하나를 label "Fable"로. 같은 label 중복 제거. 창 키가 하나도 없을 때 `None`인 기존 규칙은 그대로(limits만 있어도 하나라도 창이 나오면 Some). `parse_agy`: 최댓값 합치기를 지우고 버킷마다 창, `disabled`·알 수 없는 window 제외.

- [ ] **Step 5: 화면 — `windowName` 적용 + 테스트**

`WindowMeter`·`StatusBar` 툴팁의 `t(\`usage.window.${w.kind}\`)`를 `windowName(w)`로. `UsageCard.test.tsx` `dashboard_shows_window_label`: 창 `{kind:"seven_day_model", label:"Fable", usedPct:0, …}` → "Fable" 글자가 보인다. `src/i18n.ts`에 `usage.window.seven_day_model`(ko "모델별 주간", en "Model weekly") — label 없을 때 대비.

- [ ] **Step 6: 통과 확인**

Run: `cd src-tauri && cargo test --test plan_usage --test sub_usage && cd .. && npx vitest run src/dashboard`
Expected: PASS.

- [ ] **Step 7: 커밋** — `fix: 구독 사용량에 모델별·그룹별 창을 따로 보이기`

---

### Task 2: 한도 판정 (순수 함수)

**Files:**
- Create: `src-tauri/src/limit_watch.rs` (판정 부분), `src-tauri/src/lib.rs`에 `pub mod limit_watch;`
- Test: `src-tauri/tests/limit_watch.rs`

**Interfaces:**
- Consumes: `PlanUsage`, `PlanWindow` (Task 1).
- Produces:
  - `pub fn gating(kind: &str) -> bool`
  - `pub fn next_delay(defaults: &[&PlanUsage]) -> Duration` — 120초 또는 300초.
  - `pub struct Candidate { pub id: String, pub label: String, pub plan: PlanUsage }`
  - `#[derive(Debug, Clone, PartialEq, Serialize)] #[serde(rename_all = "camelCase")] pub struct Suggestion { pub company: &'static str /* "claude"|"codex" */, pub from_id: String, pub from_label: String, pub to_id: String, pub to_label: String, pub to_used_pct: f64, pub window_kind: &'static str, pub resets_at: String }`
  - `impl Suggestion { pub fn key(&self) -> (String, String, String, String) }` = (company, from_id, window_kind, resets_at)
  - `pub fn suggest(company: &'static str, default: &Candidate, others: &[Candidate]) -> Option<Suggestion>` — `default.plan.state=="ok"`이고 판정 대상 창 중 `>=100`이 있을 때만; 그 창(여럿이면 resetsAt가 가장 늦은 것)을 `window_kind`/`resets_at`로. `to` = `state=="ok"`이고 판정 대상 창이 모두 `<100`인 다른 계정 중 판정 대상 최댓값이 가장 낮은 것(`to_used_pct` = 그 최댓값, 반올림 없이).
  - `#[derive(Default)] pub struct Seen(HashSet<(String,String,String,String)>)` + `pub fn first_time(&mut self, s: &Suggestion) -> bool` (처음이면 기억하고 true).
  - `pub fn needs_others(default: &PlanUsage) -> bool` — 판정 대상 창 중 `>=100`이 있는가(다른 계정 조회 여부).

- [ ] **Step 1: 실패 테스트** — `tests/limit_watch.rs`:
  - `gating_excludes_model_windows`: `gating("seven_day_opus")`·`gating("seven_day_model")` false, `five_hour`·`seven_day`·`session`·`weekly`·`monthly` true.
  - `delay_is_2min_at_80`: 창 `five_hour 80.0` → 120초, `79.9` → 300초, 모델 창 `seven_day_model 95` 하나뿐 → 300초, 빈 목록 → 300초.
  - `suggests_lowest_other`: 기본 `five_hour 100`, 다른 A `seven_day 40`, B `five_hour 12`, C `five_hour 100` → to B, `to_used_pct 12.0`, `window_kind "five_hour"`.
  - `no_suggest_when_only_model_window_full`: 기본 `seven_day_model 100`, `five_hour 10` → None.
  - `no_suggest_without_room`: 다른 계정 전부 100 또는 `state "unavailable"` → None.
  - `suggest_not_repeated_for_same_key` / `suggest_again_after_reset`: `Seen::first_time` 같은 제안 두 번 → true,false; `resets_at`만 다르면 → true.
- [ ] **Step 2: 실패 확인** — `cd src-tauri && cargo test --test limit_watch` → FAIL(모듈 없음).
- [ ] **Step 3: 구현** — 위 시그니처 그대로.
- [ ] **Step 4: 통과 확인** — 같은 명령 PASS.
- [ ] **Step 5: 커밋** — `feat: 구독 한도 도달 판정과 권할 계정 고르기`

---

### Task 3: 허용 창 없는 조회 (`get_quiet`)

**Files:**
- Modify: `src-tauri/src/plan_usage.rs` (`PlanUsageClient`), `src-tauri/src/sub_usage.rs` (`SubscriptionUsage`, `AntigravityUsageClient`)
- Test: `src-tauri/tests/plan_usage.rs`, `src-tauri/tests/sub_usage.rs`

**Interfaces:**
- Produces:
  - `PlanUsageClient::get_quiet(&self) -> Option<PlanUsage>` — 이번 실행에서 토큰을 `Found`로 읽은 적이 없으면 `None`(토큰 출처를 부르지 않는다), 있으면 `Some(self.get().await)`. `fetch`에서 `Found`일 때 `AtomicBool` 표시.
  - `AntigravityUsageClient::get_quiet(&self) -> Option<PlanUsage>` — 로그인 읽기 기준 같은 규칙.
  - `SubscriptionUsage::get_quiet(&self, kind: &str, dir: Option<PathBuf>) -> Result<Option<PlanUsage>, String>` — claude는 위 규칙(+ 계정 이메일 채우기 그대로), codex는 키체인을 안 쓰므로 `Some(get)`.

- [ ] **Step 1: 실패 테스트**
  - `tests/plan_usage.rs` `quiet_get_skips_unread_account`: `counting(...)` 토큰 출처로 만든 클라이언트 → `get_quiet()` = None, 토큰 호출 0회. 이어서 `get()` 한 번 → `get_quiet()` = Some(ok), 이후 TTL 안이면 토큰 호출 1회 그대로.
  - `quiet_get_after_denied_stays_none`: 첫 `get()`이 Denied → `get_quiet()` None.
  - `tests/sub_usage.rs` `antigravity_quiet_get_needs_prior_login`: 같은 모양, `agy` 실행 0회 확인. `codex_quiet_get_reads_file`: `get_quiet("codex", None)` → Some.
- [ ] **Step 2: 실패 확인** — `cargo test --test plan_usage --test sub_usage` FAIL.
- [ ] **Step 3: 구현**
- [ ] **Step 4: 통과 확인**
- [ ] **Step 5: 커밋** — `fix: 백그라운드 구독 조회가 키체인 허용 창을 띄우지 않게`

---

### Task 4: 감시 루프·제안 전달·설정 스위치 (Rust)

**Files:**
- Modify: `src-tauri/src/limit_watch.rs` (루프·전달), `src-tauri/src/notify.rs` (문구·`Target::Suggest`), `src-tauri/src/store.rs` (`notify_account_suggest`), `src-tauri/src/commands.rs` (`take_account_suggestion`), `src-tauri/src/lib.rs` (spawn·manage·invoke_handler), `src-tauri/build.rs`, `src-tauri/capabilities/default.json`
- Test: `src-tauri/tests/limit_watch.rs`, `src-tauri/tests/notify.rs`

**Interfaces:**
- Consumes: Task 2 판정 함수, Task 3 `get_quiet`, `Active::credentials()`, `AccountCredentials::list()`·`cli_dir(kind, id)`.
- Produces:
  - 이벤트 `plans-updated`: `{ [planKey]: PlanUsage }` (planKey = `claude:<id>`·`codex:<id>`·`antigravity`, `get_quiet`이 None인 키는 싣지 않는다).
  - 이벤트 `account-suggest`: `Suggestion`(camelCase).
  - `pub struct PendingSuggest(pub Mutex<Option<Suggestion>>)` (app state).
  - 명령 `take_account_suggestion() -> Option<Suggestion>` — 대기 중 제안을 꺼내고 비운다.
  - `notify::Target::Suggest` → `open-target` payload `{"kind":"suggest"}`.
  - `pub fn suggest_notice(s: &Suggestion, lang: Lang) -> Notice` — ko 제목 `"{회사} 한도에 닿았어요"`(회사 = "Claude"|"Codex"), 본문 `"{to_label}({N}% 사용)으로 바꿔서 쓸까요?"`(N = `to_used_pct` 반올림 정수); en 제목 `"{Company} limit reached"`, 본문 `"Switch to {to_label} ({N}% used)?"`.
  - `pub enum Delivery { Banner, System, Drop }` + `pub fn route_suggestion(enabled: bool, focused: bool) -> Delivery` (꺼짐 → Drop, 앞 → Banner, 뒤 → System).
  - `pub fn spawn(app: AppHandle)` — 루프: 자격 못 읽으면(로그아웃) 300초 쉬고 다음. 회사마다 기본 계정 `get_quiet` → `needs_others`면 다른 계정도 `get_quiet` → `suggest` → `Seen::first_time` → `route_suggestion` → Banner: `emit("account-suggest")`, System: `PendingSuggest`에 넣고 `notify::deliver(suggest_notice)`. Antigravity `get_quiet`. `plans-updated` emit. `sleep(next_delay(기본 계정 plan들))`. 계정 이름표: `label`, id `system`이면 ko "이 Mac의 로그인"/en "This Mac's login".

- [ ] **Step 1: 실패 테스트**
  - `tests/notify.rs` `suggest_notice_ko_en`: `to_label "회사"`, `to_used_pct 12.4`, company claude → ko `("Claude 한도에 닿았어요", "회사(12% 사용)으로 바꿔서 쓸까요?")`, en `("Claude limit reached", "Switch to 회사 (12% used)?")`; 본문에 `@` 없음.
  - `tests/limit_watch.rs` `route_suggestion_respects_toggle`: `(false, true)`·`(false,false)` → Drop, `(true,true)` → Banner, `(true,false)` → System.
  - `tests/settings_screen.rs`(또는 store 테스트 위치) `account_suggest_defaults_on`: `{}` 역직렬화 → `notify_account_suggest == true`.
- [ ] **Step 2: 실패 확인** — `cargo test --test notify --test limit_watch --test settings_screen` FAIL.
- [ ] **Step 3: 구현** — 위 Interfaces 그대로. 루프 본체는 `tauri::async_runtime::spawn`, `setup`에서 `app.manage(PendingSuggest::default())` 뒤 `limit_watch::spawn(app.handle().clone())`. `listen_clicks`는 그대로 두고 `Target::Suggest` payload만 추가. 새 명령 `take_account_suggestion`을 `build.rs` `COMMANDS`·`capabilities/default.json` `allow-take-account-suggestion`·`invoke_handler`에.
- [ ] **Step 4: 통과 확인** — `cargo test && cargo clippy --all-targets -q` 출력 없음.
- [ ] **Step 5: 커밋** — `feat: 구독 한도 감시를 백그라운드로 옮기고 다른 계정 권하기`

---

### Task 5: 화면이 감시 결과를 받기

**Files:**
- Modify: `src/api.ts` (`onPlansUpdated`, `onAccountSuggest`, `takeAccountSuggestion`, `Suggestion` 타입, `OpenTarget`에 `{kind:"suggest"}`, `Settings.notifyAccountSuggest`), `src/stores/plans.ts`, `src/shell/StatusBar.tsx`
- Test: `src/dashboard/UsageCard.test.tsx` (StatusBar도 여기서 렌더)

**Interfaces:**
- Produces: `api.onPlansUpdated(cb: (p: Record<string, PlanUsage>) => void): Promise<UnlistenFn>`, `api.onAccountSuggest(cb: (s: Suggestion) => void)`, `api.takeAccountSuggestion(): Promise<Suggestion | null>`, TS `Suggestion { company: "claude" | "codex"; fromId; fromLabel; toId; toLabel; toUsedPct: number; windowKind: string; resetsAt: string }`.
- `plans.ts`: 모듈 수준에서 한 번 `api.onPlansUpdated`로 `usePlanStore` 병합(`useDefaultPlans` 첫 호출 때 구독, 중복 구독 금지).

- [ ] **Step 1: 실패 테스트** — `plans_update_from_watch_event`: `onPlansUpdated`를 목으로 잡아 콜백에 `{"claude:system": OK}` 전달 → 대시보드 구독 칸에 OK 창이 보인다. `status_bar_no_subscription_polling`: 가짜 타이머로 60초 진행 → `subscriptionUsage` 추가 호출 0, `usageSummary` 1회 추가.
- [ ] **Step 2: 실패 확인** — `npx vitest run src/dashboard` FAIL.
- [ ] **Step 3: 구현** — StatusBar 60초 타이머·`visibilitychange`에서 `refresh()` 제거(`loadToday`만).
- [ ] **Step 4: 통과 확인** — PASS.
- [ ] **Step 5: 커밋** — `fix: 구독 사용량은 백그라운드 감시 결과로 받기`

---

### Task 6: 확인 창 + 팝오버 계정 버튼

**Files:**
- Create: `src/usage/SwitchAccount.tsx`
- Modify: `src/shell/UsagePopover.tsx`, `src/i18n.ts`
- Test: `src/shell/UsagePopover.test.tsx` (새 파일)

**Interfaces:**
- Consumes: `api.setDefaultCredential`, `useCredentials.getState().load()`, `useDefaultPlans(...).refreshNow`.
- Produces: `export function SwitchAccountDialog({ kind, fromName, to, onClose }: { kind: "claude" | "codex"; fromName: string; to: { id: string; name: string; usedPct: number | null }; onClose: (switched: boolean) => void })` — `role="dialog"` `aria-modal`, Esc·[취소] = `onClose(false)`, [바꾸기] → `setDefaultCredential(kind, to.id)` → `load()` → `onClose(true)`. 실패하면 창 안에 오류 문구(`settings.error` 계열 재사용) 남기고 닫지 않는다.
- 문구(ko/en, `usage.switch.*`): 제목 "기본 {회사} 계정을 바꿀까요?" / "Change the default {company} account?", 줄 "{from} → {to} ({N}% 사용)", Claude 안내 "계정을 '기본'으로 둔 AI 노드는 이제 이 계정을 써요. 계정을 직접 고른 노드는 그대로예요." / "AI nodes set to “Default” will use this account. Nodes with a chosen account stay as they are.", Codex 안내 "IEUM에서 Codex 구독을 쓸 때 이 계정을 써요." / "IEUM will use this account for Codex.", 버튼 "취소"/"바꾸기", "Cancel"/"Switch".

- [ ] **Step 1: 실패 테스트** (`UsagePopover.test.tsx`)
  - `popover_account_click_opens_confirm`: Claude 계정 2개(system 기본 100%, `abcd` "회사" 12%) → 패널에서 "회사" 버튼 클릭 → 다이얼로그에 "이 Mac의 로그인 → 회사 (12% 사용)"·Claude 안내 → [바꾸기] → `setDefaultCredential("claude","abcd")` 1회.
  - `default_account_not_clickable`: 기본 줄은 `aria-disabled="true"`, 클릭해도 다이얼로그 없음.
  - `codex_confirm_text`: Codex 계정 → Codex 안내 문구.
  - `hover_switch_waits_150ms`: Claude 줄 hover → 패널 Claude, Codex 줄 hover 후 100ms 안에 패널로 mouseenter → 패널 여전히 Claude; 150ms 지나면 Codex.
- [ ] **Step 2: 실패 확인** — `npx vitest run src/shell` FAIL.
- [ ] **Step 3: 구현** — 계정 `<li>` 안을 `<button>`으로(기본은 `aria-disabled`, onClick 없음). 줄 `onMouseEnter`는 `setTimeout(150)`로 `setFly`, 패널 `onMouseEnter`에서 타이머 취소. `onClick`(키보드 포함)은 즉시.
- [ ] **Step 4: 통과 확인** — PASS.
- [ ] **Step 5: 커밋** — `feat: 사용량 팝오버에서 기본 계정 바꾸기`

---

### Task 7: 권하기 띠 + 알림 설정 스위치

**Files:**
- Create: `src/usage/SuggestBanner.tsx`
- Modify: `src/App.tsx` (띠 자리·이벤트·포커스), `src/screens/settings/PermissionsPane.tsx` (스위치), `src/i18n.ts`
- Test: `src/App.test.tsx`, `src/screens/settings/SettingsShell.test.tsx`

**Interfaces:**
- Consumes: Task 5 `onAccountSuggest`·`takeAccountSuggestion`·`onOpenTarget({kind:"suggest"})`, Task 6 `SwitchAccountDialog`.
- Produces: `export function SuggestBanner({ s, onSwitch, onLater }: { s: Suggestion; onSwitch: () => void; onLater: () => void })` — `role="status"`, 문구 ko "{회사} 기본 계정이 한도에 닿았어요 ({창 이름} 100%, {초기화} 뒤 초기화). {to}({N}% 사용)으로 바꿔서 쓸까요?" 버튼 "바꾸기"/"나중에"; en "{Company} default account hit its limit ({window} 100%, resets in {time}). Switch to {to} ({N}% used)?" "Switch"/"Later". 창 이름은 `usage.window.<windowKind>`, 초기화는 `formatReset`.
- App: `onAccountSuggest` → 띠 표시. `window` `focus` 이벤트와 `open-target {kind:"suggest"}`에서 `takeAccountSuggestion()` → 있으면 띠. "나중에" = 띠 닫기(Rust `Seen`이 이미 기억). [바꾸기] → `SwitchAccountDialog`(fromName = `fromLabel`) → 닫히면 띠도 닫기.
- 설정 › 알림: 기존 `NotifyKey` 목록에 `notifyAccountSuggest`, 라벨 ko "구독 한도에 닿으면 다른 계정 권하기" / en "Suggest another account when a subscription hits its limit".

- [ ] **Step 1: 실패 테스트**
  - `App.test.tsx` `suggest_banner_from_event`: `onAccountSuggest` 콜백에 제안 전달 → 띠에 "회사(12% 사용)으로 바꿔서 쓸까요?" → [바꾸기] → 다이얼로그 → [바꾸기] → `setDefaultCredential("claude","abcd")` → 띠 사라짐.
  - `suggest_banner_on_focus`: `takeAccountSuggestion`이 제안 반환 → `window` focus 디스패치 → 띠 보임; [나중에] → 사라짐.
  - `SettingsShell.test.tsx` `notifications_page_has_toggles_and_test` 기대 목록 끝에 "구독 한도에 닿으면 다른 계정 권하기" 추가.
- [ ] **Step 2: 실패 확인** — `npx vitest run src/App.test.tsx src/screens/settings` FAIL.
- [ ] **Step 3: 구현**
- [ ] **Step 4: 통과 확인** — `npx tsc --noEmit -p . && npx vitest run` 전부 PASS.
- [ ] **Step 5: 커밋** — `feat: 구독 한도에 닿으면 앱 안 띠로 다른 계정 권하기`

---

### Task 8: 실제 앱 확인

- [ ] **Step 1:** `npm run tauri dev`(설치본 IEUM이 떠 있으면 먼저 종료 — 같은 앱 ID). 팝오버 → Claude → 다른 계정 클릭 → 확인 창 문구 → [취소].
- [ ] **Step 2:** 대시보드·팝오버에 Claude "Fable", Antigravity 그룹별 창(Argon 따로) 보이는지.
- [ ] **Step 3:** 설정 › 알림에 새 스위치. 결과를 사용자에게 화면으로 보여 준다(바꾸기는 사용자가 직접).
