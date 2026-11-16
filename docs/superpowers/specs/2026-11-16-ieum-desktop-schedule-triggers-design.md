# IEUM Desktop — 스케줄·수동 실행 트리거 (1b) 설계

2026-11-16. 이슈 #7. 상태: 승인됨(빌더 점검 반영).

## 왜
데스크톱 워크플로우의 트리거는 "폴더에 새 파일" 하나뿐이라, "평일 9시에 레포 감사" 같은 시간 기반 자동화를 못 만든다. 이번에 스케줄·수동 실행 트리거를 넣고 트리거 구조를 여러 종류로 넓힌다.

이 작업은 세 단계의 첫 번째다(사용자 결정 11-16): **① 스케줄러(이번) → ② GitHub 노드 + GitHub 트리거(= 스케줄 + 새 항목 확인 폴링) → ③ 도구 쓰는 AI 에이전트 노드.** 최종 그림은 Orca 자동화의 "평일 레포 감사"를 IEUM 워크플로우로 만드는 것. 그래서 이번 스케줄러는 ②의 폴링 트리거가 그대로 올라탈 수 있어야 한다.

## 목표
- 카탈로그 트리거 6개를 데스크톱에서 실행: `schedule.every`(cron), `schedule.hourly`, `schedule.daily`, `schedule.weekly`, `schedule.monthly`, `manual.run`.
- Mac이 잠들었거나 앱이 꺼져 있던 동안 놓친 차례는 **2시간 유예 안이면 깨어난 뒤 한 번** 실행.
- 수동 실행은 파일 고르기 없이 [실행] 한 번(폴더 트리거 워크플로우는 지금처럼 파일 고르기 유지).
- 트리거를 "폴더 감시 하나"로 가정한 곳을 모두 여러 종류를 받게 고친다.

## 하지 않는 것
- 별도 "사전 확인(precheck)" 개념. 트리거 뒤 `branch.if`(otherwise: stop)로 같은 효과를 낸다.
- 앱이 꺼져 있어도 도는 실행(launchd 등). 스케줄은 앱이 켜져 있을 때만(창을 닫아도 트레이에 있으면 돈다).
- GitHub 트리거·앱 노드(②), AI 에이전트(③), 기다리기·승인(1c), 웹훅, 워크플로우 옮기기.
- 카탈로그 서브모듈 올리기(이번에 쓰는 schedule·manual 정의는 고정 커밋과 최신이 같다).

## 실패로 치는 것
- 같은 차례가 두 번 도는 것(잠자기에서 깬 뒤, 앱 재시작 뒤, 시계 변경 뒤 포함).
- 유예가 지난 오래된 차례를 몰아서 여러 번 돌리는 것.
- 이음 클라우드와 같은 칸 값이 다른 시각에 도는 것(시간대·요일·주말 해석).

## 고른 방식
| 결정 | 고른 것 | 버린 것 |
|---|---|---|
| 놓친 실행 | 유예 안이면 깨어난 뒤 한 번 (사용자 선택) | 건너뛰기(클라우드와 같음, 덮은 날은 실행 없음), 워크플로우마다 고르기(데스크톱 전용 칸) |
| 스케줄 방식 | 벽시계 기준 30초 확인 루프 (사용자 선택) | 워크플로우마다 "다음 시각까지 잠자기"(macOS 잠자기 중 단조 시계가 멈춰 깬 뒤 늦거나 빠짐), launchd(앱 꺼져도 돌지만 시스템 설정 파일·백그라운드 기동 경로가 커짐) |

## 정한 값 (클라우드와 같은 뜻)
- **시간대 Asia/Seoul 고정** (BE `ScheduleCron.ZONE`). Mac의 시간대와 무관.
- **칸 → cron** (BE `ScheduleCron.of`와 같은 규칙, Quartz 6필드):
  - hourly: `0 0 * <days>`
  - daily: `0 0 <hour> <days>`
  - weekly: `0 0 <hour> ? * <weekday>` (요일은 이름 MON~SUN 그대로)
  - monthly: `0 0 <hour> <day> * ?` (day `1`~`28` 또는 `L`)
  - `<days>` = `weekends`가 **명시적 false**(불리언 false 또는 문자열 "false")일 때만 `? * MON-FRI`, 그 밖엔 `* * ?`.
  - 칸 값 허용: hour `0`~`23`, weekday `MON|TUE|WED|THU|FRI|SAT|SUN`, day `1`~`28`|`L`. 벗어나면 agent와 같게 `CHOICE`(카탈로그 choices 검사 — 기존 `validate_fields` 경로). `CRON`은 `config.cron`에만.
- **schedule.every의 cron** (BE `CronConverter.toQuartz`): 5필드(Unix, 분 시 일 월 요일)면 Quartz로 바꾼다 — 앞에 초 `0`, 일·요일 중 `*`인 쪽을 `?`, 요일 숫자는 Unix(0·7=일)→Quartz(1=일). 일·요일 둘 다 지정된 5필드는 거절. 6·7필드는 Quartz로 보고 그대로. 정적 검증 `CRON`(field `config.cron`)은 agent와 같게 필드 수(5·6·7)와 문자 집합 `[0-9A-Za-z*/,\-?LW#]`, 거기에 데스크톱은 실제로 다음 시각을 계산할 수 있는지까지 본다.
- **트리거 출력** (BE `ScheduleTriggerHandler`): `triggeredAt` = 실제 실행 시각, Asia/Seoul 로컬 ISO(`2026-11-17T09:00:00`, 오프셋 없음). `schedule.every`만 `cron`도 내는데, **사용자가 쓴 원문 그대로**(BE `ScheduleCron.of`가 every는 `config.cron`을 그대로 돌려줌). `manual.run` 출력은 빈 객체.

## 동작

### 스케줄 루프
- 앱의 기존 30초 재확인 루프(`supervisor.rs` `start_with`)에서 매번 스케줄을 확인한다(같은 루프, 새 타이머 없음). 앱 시작 직후·저장·삭제·켜기/끄기 때도 한 번 확인.
- 켜진 워크플로우 중 트리거가 `schedule.*`인 것마다 **마지막으로 처리한 차례 시각**(`last_slot`)을 기억한다. 확인할 때:
  1. `now`(벽시계, Asia/Seoul)까지 지나간 차례 중 가장 최근 것 `due`를 cron으로 계산(`last_slot` 이후, `now` 이하).
  2. `due`가 없으면 끝.
  3. `now - due <= 2시간`이면 한 번 실행하고, 아니면 실행하지 않는다. 어느 쪽이든 `last_slot = due`로 넘긴다(여러 차례를 놓쳐도 한 번만, 오래된 건 버림).
- `last_slot`은 워크플로우별로 앱 데이터에 저장해 앱 재시작 뒤에도 같은 차례를 두 번 돌리지 않는다. **처음 켠 워크플로우**(기록 없음)는 `last_slot = now`로 시작 — 켜자마자 지난 차례를 돌리지 않는다.
- 워크플로우를 끄면 기록을 지우지 않는다. 다시 켤 때는 `last_slot = now`로 덮는다(꺼져 있던 동안 차례는 놓친 것으로 치지 않음).
- 트리거 칸을 바꿔 저장하면 `last_slot = now`.
- 시계가 뒤로 가면(`now < last_slot`) `last_slot = now`로 맞추고 그 확인에선 실행 안 함.

### 잘못된 스케줄
- 저장 경로는 지금처럼 정적 검증 없이 저장·켜기를 허용한다(기존 폴더 워크플로우 저장 동작을 바꾸지 않음). 루프가 다음 시각을 계산하지 못하면 그 워크플로우는 돌지 않고 카드에 기존 감시 오류 상태(`WatchState::WatchError`·`watch-status`)로 "스케줄을 계산하지 못했어요"를 보인다. 실행 직전 `validate`의 `CRON`은 [실행]·테스트 경로에서 막는다.

### 계정
- 스케줄은 **로그인한(활성) 계정의 워크플로우만** 돈다(감독자가 활성 계정 하나에만 뜨는 기존 구조). 다른 계정으로 바꿨다 돌아오면 앱 재시작과 같게 친다 — 유예 안의 놓친 차례는 한 번 돈다.

### 겹침
- 같은 워크플로우가 실행 중(또는 큐에 대기 중)일 때 스케줄 차례가 오면 **그 차례는 건너뛴다**(`last_slot`은 넘김). 기존 워크플로우별 큐(`push`, `QUEUE_CAP`)를 스케줄 차례에는 크기 1로 쓴다.
- 건너뜀은 실행 기록에 남기지 않고 앱 로그에만 남긴다.

### 수동 실행
- `manual.run` 워크플로우와 스케줄 워크플로우: 카드·편집기의 [실행]이 파일 고르기 없이 바로 돈다. 트리거 출력은 위 규칙(manual은 `{}`, schedule은 지금 시각 `triggeredAt`).
- 폴더 트리거 워크플로우: 지금처럼 파일을 골라 실행.
- 노드 테스트(빌더 `test.rs`)도 같은 규칙으로 트리거 샘플을 만든다(폴더가 아니면 `FolderMissing` 대신 위 출력).
- `manual.run` 워크플로우는 [실행]으로만 돈다: 카드에 켜기 스위치를 숨기고, 채팅 빌더 완료 확인 카드는 [켜기] 대신 [저장]으로 끝낸다(`enabled`는 true로 저장, 의미 없음).

### 실행 기록
- `RunTrigger`에 `Schedule` 추가(기존 `Watch`·`Manual`·`Test`). 시스템 알림·대시보드 집계는 `Watch`와 같게 다룬다(끝 알림만, 기존 `watch_end` 설정을 함께 따름 — 화면 문구는 "자동 실행 끝"으로). 화면 `RunTrigger` 타입에 `schedule` 추가. 유예로 늦게 돈 실행은 기록에 "예정 HH:MM, 늦게 실행" 표시가 되도록 예정 시각도 기록.

### 화면
- 워크플로우 카드: 트리거 종류 아이콘(이미 `schedule`·`manual` 있음)과 한 줄 요약 — "평일 매일 9시", "매주 월요일 9시", "매월 마지막 날 9시", "매시 정각", cron은 원문. 다음 실행 시각도 함께("다음 11/17 09:00").
- 편집기 노드 팔레트·설정 칸: 카탈로그 정의 그대로(`hour`·`weekday`·`day`·`weekends`·`cron` 칸, 고르기는 드롭다운).
- 채팅 빌더: 첫 노드 규칙을 "트리거 하나"로 넓히고 안내문(`guide.rs`, `chat/prompt.rs`, `reply.rs`)에 스케줄·수동을 넣는다. 클라우드 계약대로 **채팅 빌더 메뉴엔 `schedule.every` 없음**(편집기 팔레트에는 6개 모두).

### 트리거를 폴더로 가정한 곳 (모두 고침)
`supervisor.rs`(`trigger_folder`·`sync`·`Watch`·`QueueDropped`의 `trigger["path"]`·`run_one`의 Watch/Manual 판정·`run_now`·`run_test`·`file_trigger`), `ieum-local/src/paths.rs` `user_folders`, 빌더 `test.rs`(FolderMissing)·`finish.rs`(PendingFinish.folder)·`edit.rs`(FOLDER_FIELDS)·`reply.rs`(TriggerRule 문구)·`guide.rs`, `chat/prompt.rs`, 프런트 `WorkflowCard.tsx`(`FOLDER_TRIGGER`)·`useTestRun.ts`.

### 엔진·카탈로그
- `Catalog::embedded()`·`stage1`에 `schedule/*`·`manual/run` 추가, 프런트 `src/nodes/defs.ts`에도.
- 트리거 노드는 실행기 없이 `trigger_output`을 그대로 내므로 엔진 실행 경로는 그대로. 트리거 출력 계산은 호스트(supervisor)가 한다.
- 정적 검증에 `CRON` 코드 추가(`validate.rs`), field는 위대로.
- 칸→cron·5필드 변환은 적합성 묶음 형식(트리거 출력을 바깥에서 넣음)으로는 대조할 수 없어 **엔진 단위 테스트에 BE 대조표**(BE 테스트·코드에서 옮긴 입력→기대 Quartz 문자열)로 고정한다. 클라우드와 다른 점(놓친 실행 유예, 앱 꺼짐)은 `conformance/GAPS.md`에.
- cron 계산은 Quartz 문법(`?`·`L`·`W`·`#`·요일 이름)을 읽는 크레이트(예: `croner`)를 새로 들인다. **주의**: `croner`는 기본이 Unix 요일 숫자(0=일)라 Quartz 숫자 요일(1=일)이 하루 어긋나고, 요일 자리 `L`·7필드(연도) 해석도 다를 수 있다 — 크레이트를 고르기 전에 Quartz 대조 테스트(숫자 요일, `L`, `#`, 7필드)부터 통과시킨다. 안 되면 Quartz→크레이트 형식 변환을 직접 둔다. 시간대 계산은 `chrono-tz`(Asia/Seoul) 또는 고정 +09:00(한국은 서머타임 없음) — 빌더 몫, 단 결과는 Asia/Seoul과 같아야 한다.

## ② GitHub 트리거가 올라탈 자리
- 스케줄 루프가 "차례가 됐다 → 트리거 출력 만들기 → 큐에 넣기"로 나뉘어 있어, ②는 "차례가 됐다(폴링 주기) → 새 이슈 확인 → 있으면 항목마다 트리거 출력"을 같은 루프에 붙인다. 이번에 이 경계를 함수로 나눠 둔다(구현은 ②).

## 빌더에게 남긴 것
- `last_slot` 저장 위치·형식(계정 폴더 안 권장 — 워크플로우가 계정별).
- 카드 요약 문구의 정확한 모양, en 문구.
- 30초 루프에서 cron 계산 비용(워크플로우 수십 개 기준 무시 가능 — 측정만).
