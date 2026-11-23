# 적합성 묶음 — 계약 공백과 의도된 차이

묶음 형식: `<case>/{workflow.json, input.json, expected.json}`. 실행 = `ieum-dev conformance conformance/`.

- `input.json` = `{trigger, nodeOutputs}`. 트리거 출력은 `trigger`, `branch.if` 밖의 노드는 스텁이 `nodeOutputs[id]`(없으면 `{}`)를 그대로 낸다.
- `expected.json` = `{source, validationErrors: [{nodeId, field, code}], nodes: {id: {status, output?}}}`. `nodes`에 없는 노드는 `NOT_RUN`. `output`이 없으면 출력은 비교하지 않는다. 검증 에러는 순서까지 비교, 메시지는 비교하지 않는다.
- 순서는 "둘 다 시작한 엣지는 source가 먼저"만 검사한다(스펙 §4.6).

## 계약 문서가 말하지 않는 지점

워크스페이스 `docs/cross-repo-contracts.md`는 참조식 문법·`conditionType` 값·분기 출력 키만 정한다. 아래는 현재 구현(실행 = BE, 검증 = agent)을 정답으로 삼았다.

- 참조 값이 없을 때 — 출력이 빈 맵·키 없음·`null`이면 빈 문자열(`missing-ref-empty`, BE `ExecutionCursor.java:193`·`:215`).
- 경로 중간에 스칼라를 만나면 남은 경로를 버리고 그 값을 쓴다(`nested-path-and-index`의 `items.0.name.extra` → `x`, `:207`).
- 목록 인덱스(`items.1`), 범위 밖 인덱스 = 빈 문자열, Map·List 값 = 압축 JSON 문자열(`:229`·`:239`).
- 합류 노드 — 입력 중 하나라도 live면 실행, 전부 dead면 SKIPPED 후 아래로 전파(`merge-one-live-parent`, `branch-otherwise-stop-skips-downstream`).
- 분기 출력의 값 — `else`의 이름 `그 외`, `otherwise: stop`이면 `branch: null`·빈 목록, 레거시 이름 `참`. 계약은 키 이름만 말한다.
- 분기 설정 오류(예약어 key·모르는 연산자 등)는 검증 에러가 아니라 실행 시 노드 FAILED.
- `NOT_RUN` — 실패 뒤 남은 노드 상태. BE는 기록을 남기지 않을 뿐 이름이 없다(묶음 형식이 정한 이름).
- 갈래 `name`은 `ref: false`라 검증이 막지만, BE는 config 전체를 치환하므로 검증을 건너뛰면 이름도 치환된다.
- 정적 검증 중 `REQUIRED`·`TYPE`·`CHOICE`·`TOO_LARGE`는 1단계 러스트 범위 밖 — 묶음 워크플로우는 이 규칙에 걸리지 않게 만들었다. `CRON`은 스케줄 트리거(1b)와 함께 옮겼다(`crates/ieum-engine/tests/validate_schedule.rs`).
- 스케줄 칸→cron·5필드 변환은 트리거 출력을 바깥에서 넣는 묶음 형식으로는 대조할 수 없어, 엔진 단위 테스트에 BE 대조표로 고정했다(`crates/ieum-engine/tests/schedule.rs`).

## 의도된 차이(carry) — 러스트가 BE와 다르게 두는 곳

event가 `(참조식)`이면 특정 노드가 아니라 모든 칸의 참조식 치환에 나타나는 차이다. 차이 칸은 `BE → 러스트`.

| event | 칸 | 차이 | 고정 테스트 |
|---|---|---|---|
| (참조식) | — | 실수 표기(1e7 이상·1e-3 미만): `1.0E7`·`1.0E-4` → `10000000.0`·`0.0001` | `crates/ieum-engine/tests/render.rs` `gap_double_notation_differs_from_java` |
| (참조식) | — | u64를 넘는 정수: BigInteger 자릿수 그대로 → f64(`1.8446744073709552e+19`) | `render.rs` `gap_integer_beyond_u64_becomes_f64` |
| (참조식) | — | JSON 제어문자 이스케이프: `\u001F`(대문자) → `\u001f`(소문자) | `render.rs` `gap_json_control_escape_is_lowercase` |
| `branch.if` | `rules` | 전각·아랍 숫자: `BigDecimal`이 숫자로 읽음 → 숫자 아님(판단 불가 → 거짓) | `crates/ieum-engine/tests/condition.rs` `gap_non_ascii_digits_are_not_numbers` |
| `branch.if` | `rules` | chrono 범위 밖 날짜(연도 262143+): ±999999999년까지 읽음 → 판단 불가(거짓), panic 없음 | `condition.rs` `far_future_rfc_date_does_not_panic` |
| `branch.if` | — | 분기 경고 문구: 실행 detail에 실음 → 러너가 버림(`evaluate`에서만 보임) | `condition.rs` `gap_runner_drops_warnings` |
| `local.ai` | `outputFields` | `ref: false` 목록 속 참조: 검증(agent)이 건너뜀 → 부모 `ref: false`를 자식에 전파해 `REF_NOT_ALLOWED`(보안 강화, 데스크톱 로컬 워크플로우 전용) | `crates/ieum-engine/tests/validate.rs` `ai_output_fields_refuse_refs` |
| `formatter.text_markdown_to_html` | `input` | Markdown 중첩이 101단계를 넘으면: commonmark-java가 열린 블록 100개에서 잘라 낸 결과 → `Markdown 중첩이 너무 깊어요`(TOO_LARGE)로 실패 | `formatter_html.rs` `markdown_depth_limit` |
| `formatter.text_markdown_to_html` | `input` | 링크·이미지 주소의 공백·비 ASCII 글자: 원문 그대로(`http://x/a b`·`https://x.com/€`) → 퍼센트 인코딩(`a%20b`·`%E2%82%AC`). 같은 주소다 | `formatter_html.rs` `gap_markdown_url_percent_encoded` |
| `formatter.text_case` | `input` | `capitalize`·`title`에서 대문자가 여러 글자인 특수 글자(`ᾳ` 등 그리스 iota 하첨자): Java 한 글자 대응(`ᾼ`) → 원문 유지(`ᾳ`). `upper`·`lower`(`ß` → `SS`)는 같다 | 없음(`formatter/text.rs` `upper` 주석) |
| `formatter.text_extract_pattern` | `pattern` | Java 백트래킹이 시간·스택으로 무너지는 패턴(`(a{1,30}){1,30}$`, `(a\|b)*` 긴 입력): `패턴 검사가 1초를 넘었어요`·`패턴이 너무 복잡해요`로 실패 → 뒤·앞 확인·역참조가 없으면 선형 엔진이라 정상 결과(빈 값·전체 일치) | `crates/ieum-engine/tests/formatter_extract.rs` `gap_java_timeout_pattern_ends`·`gap_java_stack_overflow_pattern_matches` |
| `formatter.text_extract_pattern` | `pattern` | 1초 마감 문구: `패턴 검사가 1초를 넘었어요` → 역추적 상한(100만)·1초 마감 모두 `패턴이 너무 복잡해요`. 마감에 버려진 스레드는 찾기 사이마다 마감을 확인해 진행 중인 찾기 한 번(역추적 상한까지)만 마저 돈다. 사용자 패턴에 역추적 문법이 없으면 상한 5천만, 있거나 반복 횟수 곱이 32 이상이면 1백만. 줄 끝 번역이 걸린 글(10만 자 a…ab 뒤 LF)에서 release 실측 버려진 스레드 시작부터 끝까지: `(.?){31}.{31}$` 약 4.5초·`(a?){31}a{31}$` 약 3.6초(곱 31, 5천만), `(a?){1000}a{1000}$`·`(?:a?){20}(?:(?:a{9}){9}){9}$` 약 2초, `(a?){20}(?:a{20}){20}$` 1.5초, `(?:[ab]{1000}\|a)*$` 1.3초, `(a?){100}a{100}$` 0.4초(곱 32 이상, 1백만). 그동안 동시 2개 제한 때문에 다른 패턴 노드가 `다른 패턴 검사가 아직 끝나지 않았어요`로 실패할 수 있다 | `formatter_extract.rs` `runaway_pattern_ends_fast`·`runaway_lookahead_scan_hits_deadline`·`large_counts_get_low_backtrack_limit`, `formatter/extract.rs` `abandoned_find_loop_stops_at_deadline` |
| `formatter.text_extract_pattern` | `pattern` | 반복 횟수의 중첩 곱(`{n}`=n, `{n,m}`=m, `{n,}`=n, 이어 붙은 것은 가장 큰 값)이 1000을 넘는 패턴(`a{1001}`·`(?:[ab]{5000}\|a)*$`·`(a{40}){30}`): Java는 받는다 → `정규식 패턴 오류예요: 반복 횟수가 너무 커요`. 5천만 상한에서 `(a?){5000}a{5000}$`의 버려진 스레드가 403.7초 돌던 것(재리뷰 2 실측: N=100 9.2초, 1000 81.7초, 5000 403.7초)을 막는다 | `formatter_extract.rs` `java_regex_unsupported`·`java_groups_and_counts_pass` |
| `formatter.text_extract_pattern` | `pattern` | 글이 줄바꿈으로 끝나고 줄이 아주 길 때(줄 2000자 9줄, 줄 300자 332줄 등) `.+$`·`.*$`: Java는 마지막 줄 → 줄마다 줄 길이 제곱만큼 역추적해 `패턴이 너무 복잡해요`로 실패 | — |
| `formatter.text_extract_pattern` | `pattern` | 동시 실행: 제한 없음 → 1초 마감에 버려져 아직 도는 스레드를 포함해 정규식 스레드가 이미 2개 돌고 있으면 새 실행은 바로 `다른 패턴 검사가 아직 끝나지 않았어요. 잠시 뒤 다시 실행해 주세요.`(코드 없음 — 패턴 잘못이 아니다)로 실패 | `crates/ieum-engine/src/formatter/extract.rs` `abandoned_threads_are_capped` |
| `formatter.text_extract_pattern` | `pattern` | Java와 뜻이 다르거나 옮기지 않은 문법(`\h \H \v \V \R \X \G \K \N \cX`, `\Q…\E`, 클래스 안 `\b`·`--`, POSIX·java 속성 `\p{Alpha}`·`\p{IsDigit}`·`\p{javaLowerCase}`·`\p{gc=Alpha}` 등, 플래그 `(?d)`·`(?x)`·`(?U)`, 한쪽이 빈 클래스 교집합 `[&&a]`·`[a&&]`, 수량자 뒤 반복 횟수 `x{1}{2}`·`a*{2}`)과 Java에 없는 그룹 문법(`(?~…)`·`(?(1)…)`·`(*FAIL)`·`(?#…)`·`(?'n'…)` 등 — `(?:` `(?=` `(?!` `(?<=` `(?<!` `(?>` `(?<이름>`·플래그 말고는 모두. Java도 거절하므로 값 차이는 없다): Java 뜻대로 실행 → `정규식 패턴 오류예요: <설명>`(FIELD_INVALID pattern)으로 실패. `a{,n}`·짝 없는 `{`는 둘 다 패턴 오류 | `formatter_extract.rs` `java_regex_unsupported` |
| `formatter.text_extract_pattern` | `pattern` | `(?i)`: Java는 ASCII 글자만 대소문자 무시 → 유니코드까지 무시(`(?i)é`가 `É`에, `(?i)k`가 켈빈 기호에 맞음). 비 ASCII 글자에서만 다르다(`(?iu)`는 같다) | `formatter_extract.rs` `gap_case_insensitive_is_unicode` |
| `formatter.text_extract_pattern` | `pattern` | 빈 일치와 BMP 밖 글자: Java는 서로게이트 쌍 가운데서도 빈 일치를 셈(`x*` all을 이모지 하나에 빈 값 3개) → 글자 단위라 2개 | `formatter_extract.rs` `gap_empty_match_inside_surrogate_pair` |
| `formatter.text_extract_pattern` | `pattern` | `\b`·`\B`: Java는 글자·숫자 뒤에 붙은 결합 문자(Mn)도 단어 글자로 침 → ASCII `\w`만 단어 글자(`e`+U+0301+`x`에서 경계 2개 → 4개) | `formatter_extract.rs` `gap_word_boundary_combining_mark` |
| `formatter.date_format`·`formatter.date_add`·`formatter.date_compare` | `input`·`other` | 날짜 범위: Java는 ±999999999년까지 계산 → chrono는 약 ±26만 년까지라 그 범위를 넘는 `date_add` 결과는 `계산한 날짜가 표기할 수 있는 범위를 넘었어요`(TOO_LARGE), 그런 입력은 `날짜로 읽지 못했어요`(FIELD_INVALID)로 실패 | `formatter_date.rs` `out_of_range`·`read_out_of_range` |
| `formatter.date_format` | `customPattern` | `y M d E H h m s a`·따옴표·구두점 밖의 Java 패턴(`QQ`·`MMM`·`SSS`·`VV`·`[ ]` 등): `DateTimeFormatter`로 표기(`yyyy-QQ` → `2026-04`) → `지원하지 않는 날짜 패턴 문자예요: <문자>`로 실패 | `crates/ieum-engine/tests/formatter_date.rs` `date_format_plan_cases`·`custom_pattern_letters` |
| `formatter.list_pick`·`formatter.list_to_text`·`formatter.lookup_table` | `input`·`table` | JSON 글 속 객체·배열 안쪽(2단계 이하)의 소수: `BigDecimal` 원문 자릿수(`[1.50]` → `[1.50]`) → f64 표기(`[1.5]`). 1단계 원소·값은 원문에서 다시 읽어 BE와 같다 | `crates/ieum-engine/tests/formatter_list.rs` `decimals_keep_source_digits` |
| `formatter.list_pick`·`formatter.list_to_text`·`formatter.lookup_table` | `input`·`table` | 펴면 10만 자를 넘는 소수(`0e-999999999`·`1e-99999`): 수억 자 글을 만듦 → `목록 속 숫자가 10만 자를 넘어요`(TOO_LARGE)로 실패 | `formatter_list.rs` `decimals_keep_source_digits`·`huge_exponents_fail_fast` |
| `formatter.list_pick`·`formatter.list_to_text`·`formatter.lookup_table` | `input`·`table` | 한 목록·표에서 편 소수 글의 합계가 100만 자를 넘으면(`1e-99997` × 11): 그대로 폄 → `목록 속 숫자가 모두 합해 100만 자를 넘어요`(TOO_LARGE)로 실패 | `formatter_list.rs` `huge_exponents_fail_fast` |
| `formatter.list_pick`·`formatter.list_to_text`·`formatter.lookup_table` | `input`·`table` | 중첩이 128단을 넘는 JSON 글: Jackson은 1000단까지 목록으로 읽음 → serde_json 재귀 상한에 걸려 `목록이 너무 깊어요`(TOO_LARGE)로 실패(쉼표 나누기로 가지 않는다). 재귀 상한은 serde_json 오류 문구(`recursion limit exceeded`)로 가린다 | `formatter_list.rs` `deep_nesting_fails`, `formatter/inputs.rs` `recursion_limit_message_is_pinned` |
| `formatter.list_pick` | `input` | u64·i64 범위를 넘는 정수 원소(`[18446744073709551616]`·`[-9223372036854775809]`): BigInteger(JSON 숫자) → 같은 자릿수의 글. 참조식으로 쓰면 같은 글이지만 출력 타입·실행 기록이 다르다 | `formatter_list.rs` `gap_integer_beyond_u64_is_text` |
| `http.request` | — | 4xx·5xx 실패 문구의 응답 본문: 1MB까지 그대로 → 앞 2,000자(UTF-16 단위)까지, 넘으면 `…`(실행 기록이 커지지 않게) | `crates/ieum-local/tests/http.rs` `error_body_in_message_is_capped_at_2000_utf16_units` |
| `http.request` | — | 요청 전체 시간: 읽기 제한(30초 동안 아무것도 안 오면 실패)만 → 요청 하나(리다이렉트 홉·본문 읽기 포함)에 전체 5분 상한도 둔다. 넘으면 `요청을 보내지 못했어요: 시간 초과`(HTTP_UNREACHABLE) | `crates/ieum-local/tests/http.rs` `trickling_body_hits_total_timeout` |
| `http.request` | `headers` | 다른 출처(scheme·host·port)로 리다이렉트될 때: `Authorization`·`Proxy-Authorization`·`Cookie`만 뗌 → `Content-Type`·`Accept` 밖의 머리말을 전부 뗌(`X-API-Key` 등이 다른 서버로 가지 않게). 같은 출처 안에서는 둘 다 유지 | `http.rs` `cross_origin_redirect_drops_all_user_headers`·`same_origin_redirect_keeps_user_headers` |
| `http.request` | `headers` | 머리말 값의 비 ASCII 글자(`홍길동 é`): Netty 인코더가 Latin-1 밖 글자를 `?`로 바꿔 보냄(`3f 3f 3f 20 e9`) → UTF-8 바이트 그대로 보냄. 받는 서버가 보는 바이트가 다르다 | 없음 |
| `http.request` | 응답 `body` | 글자 집합: `Content-Type`의 charset을 따르고 없으면 JSON은 UTF-8, 그 밖은 ISO-8859-1 → charset을 보지 않고 늘 UTF-8(깨진 바이트는 U+FFFD). `euc-kr` 응답은 데스크톱에서 깨지고, charset 없는 `text/plain` UTF-8 한글은 BE에서 깨진다 | `http.rs` `non_utf8_body_is_lossy_string` |
| `http.request` | `url` | 실패 문구(주소 형식·scheme·호스트 없음·리다이렉트 3회 초과·Location 형식)에 싣는 주소: 원문 그대로 → 쿼리·fragment를 `?…`·`#…`로, `://` 뒤 계정 정보(`@` 앞)를 `…`로 가림(토큰이 실행 기록에 남지 않게) | `crates/ieum-local/tests/http.rs` `url_secrets_are_masked_in_messages`·`fourth_redirect_location_secret_is_masked` |
| `formatter.list_pick`·`formatter.list_to_text`·`formatter.lookup_table` | `input`·`table` | f64 범위를 넘는 숫자가 든 JSON 글(`[1e99999]`·`{"a": -1e400}`): `BigDecimal`로 읽어 폄 → `목록 속 숫자가 너무 커요`(TOO_LARGE)로 실패. 그 밖의 파싱 실패(뒤에 글자가 남음·깨진 JSON)는 BE처럼 쉼표로 나눈다. 범위 초과는 serde_json 오류 문구(`number out of range`)로 가린다 | `formatter_list.rs` `numbers_beyond_f64_fail`, `formatter/inputs.rs` `out_of_range_message_is_pinned` |

실수 표기·이스케이프 차이는 참조식 치환 결과 문자열에만 나타난다(조건 비교는 `java_num`으로 Java 표기를 따른다). u64를 넘는 정수는 읽는 순간 f64가 되므로 조건 비교에도 나타난다.

## 스케줄 트리거(1b) — 데스크톱이 클라우드(Quartz)와 다르게 도는 곳

차이 칸은 `클라우드 → 데스크톱`. 시각은 모두 Asia/Seoul(Mac 시간대와 무관).

| event | 칸 | 차이 | 고정 테스트 |
|---|---|---|---|
| `schedule.*` | — | 놓친 차례: 서버가 늘 떠 있어 놓치지 않고, 놓치면 건너뜀 → Mac이 잠들었거나 앱이 꺼져 놓친 차례는 가장 최근 하나만, 그 차례에서 2시간 안이면 깨어난 뒤 한 번 실행. 2시간을 넘으면 건너뛰고 기록만 넘긴다 | `src-tauri/tests/supervisor.rs` `missed_within_grace_once`·`missed_beyond_grace_none` |
| `schedule.*` | — | 시계가 뒤로 감: 해당 없음 → 마지막 차례보다 2시간 안으로 뒤면 기록을 그대로 둬 이미 돈 차례를 다시 돌지 않고, 2시간보다 크게 뒤면 기록을 지금으로 맞춘다. 그 확인에서는 실행하지 않는다 | `supervisor.rs` `clock_back_small_keeps_slot`·`clock_back_resets_slot` |
| `schedule.*` | — | 도는 조건: 서버에서 늘 → 앱이 켜져 있고 로그인한(활성) 계정의 켜진 워크플로우만. 다른 계정으로 바꿨다 돌아오면 앱 재시작과 같게 친다(유예 안의 놓친 차례는 한 번). 처음 켠·다시 켠·트리거 칸을 바꾼 워크플로우는 지난 차례를 돌리지 않고 지금부터 센다 | `supervisor.rs` `first_enable_no_backfill`·`reenable_starts_now`·`trigger_change_starts_now`·`restart_no_duplicate` |
| `schedule.*` | — | 겹침: Quartz 잡이 따로 돈다 → 같은 워크플로우가 실행 중·대기 중이면 그 차례는 건너뜀(기록 없이 앱 로그만) | `supervisor.rs` `overlap_skips` |
| `schedule.*` | 출력 `triggeredAt` | `LocalDateTime.now().format(ISO_LOCAL_DATE_TIME)`라 소수 초까지(`2026-11-17T09:00:00.123456`) → 초까지(`2026-11-17T09:00:00`). 뜻은 같다 | `crates/ieum-engine/tests/schedule.rs` `output_shape` |
| `schedule.*` | — | 저장소에 차례 기록(`schedule.json`)을 못 쓸 때(디스크 가득 참·권한): 해당 없음 → 같은 프로세스 안에선 메모리 기록으로 같은 차례를 다시 돌지 않는다. 앱을 다시 켜면 그 차례가 유예 안에서 한 번 더 돌 수 있다(로그만 남김) | `supervisor.rs` `slot_write_failure_runs_once` |
| `schedule.every` | `cron` | 30초보다 잦은 식(`*/10 * * * * ?`·`* * * * * ?`): 10초·1초마다 → 확인이 30초마다라 그때마다 가장 최근 차례 하나만(30초에 한 번) | 없음(설계상 확인 간격) |
| `schedule.every` | `cron` | `W`(가장 가까운 평일) 날에 여러 번 도는 식(`* 18 15W * *` 등): Quartz는 옮겨진 W 날에 첫 번째만 돌고 다음 계산이 다음 달로 넘어감 → 그날 차례를 모두 돈다. `LW`·`nW` 날로 넘어갈 때 Quartz가 시작 시각의 시를 가져가는 결함도 따르지 않는다 | 없음(Quartz 결함) |
| `schedule.every` | `cron` | 데스크톱만 거절하는 식(`지원하지 않는 cron 형식이에요` 또는 `올바르지 않은 cron 표현식입니다`): 일 자리 `L-n`(말일 n일 전), 넘어가는 요일 범위에 간격(`FRI-MON/2`), 월·요일 이름 뒤 간격(`MON/2`·`MON-FRI/2`·`SUN/3`·`JAN/3`·`JAN-DEC/3` — Quartz는 간격을 버리고 계산해 데스크톱과 시각이 달라지므로 거절), 시·일·월의 넘어가는 범위(`22-2`·`28-3`·`NOV-FEB`), 목록 속 `*`(`*,12-18`). 넘어가는 요일 범위만(`FRI-MON`·Unix `6-7`)·요일 자리 `L` 단독(토요일)·숫자 간격(`2/2`·`1/3`)은 같다 | `schedule.rs` `unsupported_quartz_rejected`·`name_with_step_rejected`·`weekday_range_wraps`·`weekday_l_alone_is_saturday` |
| `schedule.every` | `cron` | 데스크톱만 받는 식: 범위보다 큰 간격(`0 0/90 * * * ?`·`0 5/70 * * * ?` — Quartz는 거절, 데스크톱은 매시 :00·:05), 숫자와 이름이 섞인 Unix 요일 범위(`11 * * * TUE-7` — BE는 `TUE-1`로 바꾼 뒤 거절, 데스크톱은 화~토·일로 폄). 정적 검증 `CRON`도 통과시킨다 | 없음 |
| `schedule.every` | `cron` | 둘 다 거절해 같은 것: 요일 목록에 `#`·`L`이 섞인 `,`(`2#1,6#3`·`6L,2`), 일·요일이 둘 다 지정되거나 둘 다 `?`인 Quartz 식, 일·요일을 둘 다 지정한 Unix 5필드, 두 자리 Unix 요일(`07`·`00`), 이름 요일에 간격(Unix `MON/2`) | `schedule.rs` `unsupported_quartz_rejected`·`unix_weekday_single_digit_only`·`quartz_features`·`to_quartz_matches_be` |
| `github.*` | — | 오류 문구: `GitHub API 오류 (코드): <응답 본문>` → 상태 코드별 정해 둔 문구(본문 안 실음) | `crates/ieum-local/tests/github_api.rs` `status_table` |
| `github.*` | — | 응답 크기: 상한 없음 → 응답 하나당 32MB 넘으면 실패 | `github_api.rs` `too_large_fails` |
| `github.*` | — | 조회 노드: 403 문구가 읽기·쓰기로 나뉨(agent는 응답 본문을 실음), 드롭다운(`github.owners`·`repos`·`branches`·`milestones`)은 목록 + 검색 + 직접 입력·앞 단계 값(클라우드는 선택만), 목록은 300개까지 받아 화면에서 거름(클라우드는 페이지 + 브랜치·마일스톤 `q`), 남의 개인 계정 저장소는 `/users/{owner}/repos`로 공개 저장소를 보임 | `crates/ieum-local/tests/github_api.rs` `status_table` |
| `github.*` | — | PR 리뷰 작성자: `bot` 없음(데스크톱은 `me`만, 그 밖은 입력 오류) · `include_comments` 글 `"false"`를 거짓으로 처리 · PR 노드도 오류 문구에 응답 본문을 안 실음 | `github_review.rs` `author_bot_rejected`·`reviews_rejects`, `github_pulls.rs` `pulls_422_is_invalid` |
| `github.create_pull_request`·`find_or_create_pull_request` | `draft`·`maintainer_can_modify` | `include_comments`처럼 글 `"false"`를 거짓으로 처리(agent는 `bool("false")`가 참) | `github_pulls.rs` `create_pr_bool_strings` |
| `github.submit_pr_review` | — | `리뷰 내용이 없습니다.` 확인: 파일·`/user` 호출 뒤 → 토큰을 읽고 요청을 보내기 전에 먼저. 요청이 줄 뿐 결과는 같다(그 호출이 실패하는 경우만 다름) | `github_review.rs` `empty_review_rejected` |

## GitHub 트리거 — 데스크톱이 클라우드(웹훅)와 다르게 도는 곳 (#35)

`github.new_issue`·`github.new_pull_request`. 차이 칸은 `클라우드 → 데스크톱`. 고정 테스트는 `crates/ieum-local/tests/github_watch.rs`(판정)·`src-tauri/tests/github_trigger.rs`(감독 루프)·`crates/ieum-engine/tests/catalog.rs`(칸).

| event | 칸 | 차이 | 고정 테스트 |
|---|---|---|---|
| `github.*` | — | 받는 방식: 웹훅 즉시 → 1분마다 확인. 앱이 켜져 있고 로그인한 계정의 켜진 워크플로우만 | `github_trigger.rs` `github_new_issue_runs_once`·`github_zero_workflows_no_token_call` |
| `github.*` | — | 놓친 변화: 앱이 꺼져 있던 동안은 2시간 안의 것만 실행(이슈·opened는 만든 시각, 나머지는 갱신 시각), 넘으면 기록만 | `github_trigger.rs` `github_restart_runs_only_within_grace`, `github_watch.rs` `issue_outside_grace_recorded_only`·`pr_outside_grace_recorded_only` |
| `github.new_pull_request` | — | 두 확인 사이 여러 변화: 이벤트마다 실행 → PR당 하나(opened > reopened > ready_for_review > synchronize). 1분 안에 닫았다 다시 연 PR·초안으로 되돌렸다 준비 완료한 PR은 못 잡고, 1분 안에 열렸다 닫힌 PR도 못 잡는다(이슈는 잡음) | `github_watch.rs` `pr_priority_one_action`·`pr_closed_removed_when_complete` |
| `github.*` | `repoId` → `owner`·`repo` | 저장소 칸: 숫자 id(App 설치 저장소) → 내가 볼 수 있는 저장소(이름) | `catalog.rs` `github_triggers_use_owner_repo_fields` |
| `github.new_issue` | — | 자기 발화: App ID로 거름 → 이 앱이 만든 이슈 번호를 24시간 기억해 거름(owner·repo 소문자 키) | `github_trigger.rs` `github_issue_created_by_app_skipped`, `github_watch.rs` `created_issues_case_insensitive`·`created_issues_expire_after_24h`·`created_issues_survive_reopen` |
| `github.*` | — | 중복: delivery id(4일) → 상태(번호·SHA) 기준. 클라우드보다 중복이 적다 | `github_watch.rs` `pr_opened_reopened_ready_sync`·`pr_baseline_takes_larger_number` |
| `github.*` | — | 테스트 샘플: BE와 같은 방식(이슈 최근 20건에서 PR 뺀 첫 항목, PR 최근 20건에서 멤버·초안 거른 첫 항목, action `opened`). 다른 점: 항목이 없으면 막는다 | `github_watch.rs` `samples`, `github_trigger.rs` `github_node_test_no_items_errors`·`github_pr_sample_skips_non_members` |
| `github.new_pull_request` | — | 열린 PR이 100건을 넘는 저장소: 목록 밖 PR의 변화는 잡지 못하고, 그런 PR이 목록에 처음 나타나면 실행 없이 기록만 | `github_watch.rs` `pr_truncated_keeps_open_and_records_unknown` |
| `github.*` | — | 한도: 웹훅이라 해당 없음 → 남은 한도가 10% 아래면 확인을 쉰다(직접 누른 샘플은 안 막음) | `github_trigger.rs` `github_rate_limit_pauses`·`github_sample_ignores_reserve` |

## 노드별 테스트 — 데스크톱이 클라우드와 다르게 도는 곳

| 칸 | 차이 | 고정 테스트 |
|---|---|---|
| 샘플 저장 | 클라우드는 노드 테스트 결과만 샘플이 됨 → 데스크톱은 빌더 AI의 테스트 결과도 샘플이 된다 | 없`src-tauri/tests/node_test.rs` `builder_test_writes_samples` |
| 파일 노드 | 해당 없음 → 파일 노드 테스트는 실행한 뒤 되돌리고, 샘플의 경로를 원래 경로로 바꿔 저장한다 | 없`src-tauri/tests/node_test.rs` `file_node_sample_uses_undone_path` |
| 실행 기록 | 클라우드는 사용량·한도 때문에 남김 → 데스크톱도 노드 테스트가 `test` 실행 기록을 남긴다(exec-event는 내지 않아 캔버스·실행 목록은 그대로) | 없`src-tauri/tests/node_test.rs` `run_record_keeps_usage` |
| 검증 | 클라우드는 그래프 전체 검증 → 데스크톱은 노드 단위 검증만으로 테스트한다(그래프 오류는 막지 않음) | 없`crates/ieum-engine/tests/run_node.rs` `node_errors_ignores_graph_errors` |
