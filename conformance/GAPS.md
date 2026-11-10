# 적합성 묶음 — 계약 공백과 의도된 차이

묶음 형식: `<case>/{workflow.json, input.json, expected.json}`. 실행 = `ieum conformance conformance/`.

- `input.json` = `{trigger, nodeOutputs}`. 트리거 출력은 `trigger`, `branch.if` 밖의 노드는 스텁이 `nodeOutputs[id]`(없으면 `{}`)를 그대로 낸다.
- `expected.json` = `{source, validationErrors: [{nodeId, field, code}], nodes: {id: {status, output?}}}`. `nodes`에 없는 노드는 `NOT_RUN`. `output`이 없으면 출력은 비교하지 않는다. 검증 에러는 순서까지 비교, 메시지는 비교하지 않는다.
- 순서는 "둘 다 시작한 엣지는 source가 먼저"만 검사한다(스펙 §4.6).

## 계약 문서가 말하지 않는 지점

워크스페이스 `CLAUDE.md`는 참조식 문법·`conditionType` 값·분기 출력 키만 정한다. 아래는 현재 구현(실행 = BE, 검증 = agent)을 정답으로 삼았다.

- 참조 값이 없을 때 — 출력이 빈 맵·키 없음·`null`이면 빈 문자열(`missing-ref-empty`, BE `ExecutionCursor.java:193`·`:215`).
- 경로 중간에 스칼라를 만나면 남은 경로를 버리고 그 값을 쓴다(`nested-path-and-index`의 `items.0.name.extra` → `x`, `:207`).
- 목록 인덱스(`items.1`), 범위 밖 인덱스 = 빈 문자열, Map·List 값 = 압축 JSON 문자열(`:229`·`:239`).
- 합류 노드 — 입력 중 하나라도 live면 실행, 전부 dead면 SKIPPED 후 아래로 전파(`merge-one-live-parent`, `branch-otherwise-stop-skips-downstream`).
- 분기 출력의 값 — `else`의 이름 `그 외`, `otherwise: stop`이면 `branch: null`·빈 목록, 레거시 이름 `참`. 계약은 키 이름만 말한다.
- 분기 설정 오류(예약어 key·모르는 연산자 등)는 검증 에러가 아니라 실행 시 노드 FAILED.
- `NOT_RUN` — 실패 뒤 남은 노드 상태. BE는 기록을 남기지 않을 뿐 이름이 없다(묶음 형식이 정한 이름).
- 갈래 `name`은 `ref: false`라 검증이 막지만, BE는 config 전체를 치환하므로 검증을 건너뛰면 이름도 치환된다.
- 정적 검증 중 `REQUIRED`·`TYPE`·`CHOICE`·`CRON`·`TOO_LARGE`는 1단계 러스트 범위 밖 — 묶음 워크플로우는 이 규칙에 걸리지 않게 만들었다.

## 의도된 차이(carry) — 러스트가 BE와 다르게 두는 곳

| 차이 | BE | 러스트 | 고정 테스트 |
|---|---|---|---|
| 실수 표기(1e7 이상·1e-3 미만) | `1.0E7`, `1.0E-4` | `10000000.0`, `0.0001` | `crates/ieum-engine/tests/render.rs` `gap_double_notation_differs_from_java` |
| u64를 넘는 정수 | BigInteger 자릿수 그대로 | f64(`1.8446744073709552e+19`) | `render.rs` `gap_integer_beyond_u64_becomes_f64` |
| JSON 제어문자 이스케이프 | `\u001F`(대문자) | `\u001f`(소문자) | `render.rs` `gap_json_control_escape_is_lowercase` |
| 전각·아랍 숫자 | `BigDecimal`이 숫자로 읽음 | 숫자 아님(판단 불가 → 거짓) | `crates/ieum-engine/tests/condition.rs` `gap_non_ascii_digits_are_not_numbers` |
| chrono 범위 밖 날짜(연도 262143+) | ±999999999년까지 읽음 | 판단 불가(거짓), panic 없음 | `condition.rs` `far_future_rfc_date_does_not_panic` |
| 분기 경고 문구 | 실행 detail에 실음 | 러너가 버림(`evaluate`에서만 보임) | `condition.rs` `gap_runner_drops_warnings` |

실수 표기·이스케이프 차이는 참조식 치환 결과 문자열에만 나타난다(조건 비교는 `java_num`으로 Java 표기를 따른다). u64를 넘는 정수는 읽는 순간 f64가 되므로 조건 비교에도 나타난다.
