# 적합성 묶음 — 계약 공백과 의도된 차이

묶음 형식: `<case>/{workflow.json, input.json, expected.json}`. 실행 = `ieum-dev conformance conformance/`.

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
| `formatter.text_extract_pattern` | `pattern` | 1초 마감 문구: `패턴 검사가 1초를 넘었어요` → 역추적 상한(100만)·1초 마감 모두 `패턴이 너무 복잡해요`. 마감에 버려진 스레드는 찾기 사이마다 마감을 확인해 진행 중인 찾기 한 번(역추적 상한까지, release 최악 약 13초)만 마저 돈다 | `formatter_extract.rs` `runaway_pattern_ends_fast`·`runaway_lookahead_scan_hits_deadline`, `formatter/extract.rs` `abandoned_find_loop_stops_at_deadline` |
| `formatter.text_extract_pattern` | `pattern` | 동시 실행: 제한 없음 → 1초 마감에 버려져 아직 도는 스레드를 포함해 정규식 스레드가 이미 2개 돌고 있으면 새 실행은 바로 `다른 패턴 검사가 아직 끝나지 않았어요. 잠시 뒤 다시 실행해 주세요.`(코드 없음 — 패턴 잘못이 아니다)로 실패 | `crates/ieum-engine/src/formatter/extract.rs` `abandoned_threads_are_capped` |
| `formatter.text_extract_pattern` | `pattern` | Java와 뜻이 다르거나 옮기지 않은 문법(`\h \H \v \V \R \X \G \K \N \cX`, `\Q…\E`, 클래스 안 `\b`·`--`, POSIX·java 속성 `\p{Alpha}`·`\p{IsDigit}`·`\p{javaLowerCase}`·`\p{gc=Alpha}` 등, 플래그 `(?d)`·`(?x)`·`(?U)`, 한쪽이 빈 클래스 교집합 `[&&a]`·`[a&&]`, 수량자 뒤 반복 횟수 `x{1}{2}`·`a*{2}`): Java 뜻대로 실행 → `정규식 패턴 오류예요: <설명>`(FIELD_INVALID pattern)으로 실패. `a{,n}`·짝 없는 `{`는 둘 다 패턴 오류 | `formatter_extract.rs` `java_regex_unsupported` |
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
