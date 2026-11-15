# IEUM 데스크톱 — 변환·포맷터·HTTP 노드 실행 (연결 없는 노드 1a)

날짜: 2026-11-15 · 이슈 #5 · 브랜치 `feat/5-builtin-nodes`(base `dev`)

## 1. 왜

데스크톱에서 도는 워크플로우는 무료, 이음 클라우드에서 도는 워크플로우는 나중에 과금한다(사용자 결정 11-15). 그러려면 기본 실행 위치가 데스크톱이어야 하고, 데스크톱이 이음 클라우드 노드를 직접 실행할 수 있어야 한다. 지금 데스크톱이 실행하는 노드는 6개(`local.*` 5개 + `branch.if`)뿐이다.

전체 순서(사용자 승인 11-15): **1a 변환·포맷터·HTTP(이 문서)** → 1b 스케줄·수동 트리거 → 1c 기다리기·승인 → 2 연결(교환 서버) → 3 앱별 노드 → 4 워크플로우 옮기기.

1a는 "입력을 받아 계산해 출력만 내는" 노드 24개다. 멈춤·재개, 트리거, 연결이 필요 없다.

## 2. 목표 / 하지 않는 것 / 실패로 치는 것

**목표**
- 데스크톱 엔진이 아래 24개 event를 실행한다: `transform.map`, `formatter.*` 22개, `http.request`.
- 편집기 팔레트에서 고르고 설정할 수 있다. AI 빌더(`ieum workflow` 명령·채팅 패널)도 쓸 수 있다.
- 같은 워크플로우·같은 입력이면 이음 클라우드(BE Java)와 **의미가 같은** 결과를 낸다.

**하지 않는 것**
- 스케줄·수동 트리거(1b), 기다리기·승인(1c). 1a 노드는 여전히 `local.folder_new_file` 트리거 뒤에만 놓인다.
- HTTP 노드의 `webhookCredentialId` 칸(이음 클라우드에 저장된 웹훅 주소) — 연결 작업(2) 때 다룬다.
- HTTP 재시도·멱등 키(`Idempotency-Key`). 데스크톱 엔진엔 재시도 정책이 없다.
- 워크플로우 옮기기와 옮길 때 검증(4).
- 글자 단위까지 BE와 같은 출력. 다른 곳은 기록한다(§6).
- 카탈로그 서브모듈 올리기. 대상 24개 정의는 지금 고정 커밋(670739f)과 origin/main이 같다(11-15 `git diff` 확인). 승인 노드가 달라지는 1c에서 올린다.

**실패로 치는 것**
- 포맷터 결과가 BE와 **의미가** 다르다(숫자 반올림, 날짜 하루 차이, 시간대 등). 조용히 다른 값을 내느니 실패한다.
- `text_markdown_to_html`이 스크립트·위험한 주소를 통과시킨다(결과가 메일·Slack 본문으로 나간다).
- 사용자 정규식이 앱을 멈추게 한다.
- 새 노드 때문에 기존 워크플로우(6개 노드)의 검증·실행이 달라진다.

## 3. 검토한 방법

| 방법 | 장점 | 단점 |
|---|---|---|
| **A. Rust로 직접 옮기기 (선택)** | 데스크톱 혼자 돈다, 앱 용량 그대로 | Java 라이브러리(정규식·날짜 패턴·commonmark·jsoup)와 세부가 다르다 |
| B. 계산을 이음 클라우드에 맡기기 | 구현 0, 결과 완전 동일 | 데스크톱 무료 실행이라는 목적과 반대, 오프라인 불가 |
| C. BE Java 코드를 앱에 싣기 | 결과 완전 동일 | JVM을 앱에 넣어야 함 — Tauri를 고른 이유(용량·성능 1순위)와 충돌 |

사용자가 A를 골랐다. 두 엔진의 차이는 "의미 동일 + 차이 기록"으로 다룬다(§6).

## 4. 정해진 것 (사용자 결정)

- 방법 A. 결과는 의미만 같으면 되고, 글자 차이는 `conformance/GAPS.md`에 적는다.
- **HTTP 노드는 데스크톱에서 `localhost`·사설망 주소를 허용한다.** BE의 SSRF 차단(`HttpNodeExecutor.validateUrl`)은 서버 보호용이라 옮기지 않는다. 그런 주소를 쓰는 워크플로우는 이음 클라우드에서 실패한다 — 옮길 때 검증(4)이 알려 줄 몫.
- `webhookCredentialId`는 이번에 뺀다.
- 정규식·날짜 사용자 패턴은 자주 쓰는 범위만. 지원하지 않는 문법은 실패시키고 GAPS에 적는다.
- HTTP 노드 테스트·[켜기] 확인 규칙(§5.7).
- (4의 규칙, 참고) PC 전용 노드가 든 워크플로우는 클라우드로 못 옮기고, 클라우드 전용 노드가 든 워크플로우는 PC로 못 옮긴다.

## 5. 설계

BE 구현 위치(읽기 전용 참조): `ieum-backend/workflow-core/src/main/java/com/ieum/workflowcore/engine/executor/` — `TransformNodeExecutor`, `Formatter`, `FormatterInputs`, `TextFormats`, `NumberFormats`, `DateFormats`, `ListFormats`, `ConditionValues`, `HttpNodeExecutor`. 테스트는 같은 경로의 `src/test`. **설계 문서와 BE 코드가 다르면 BE 코드를 믿는다.**

### 5.1 카탈로그·노드 종류

- `crates/ieum-engine/src/catalog.rs`의 `NodeType`에 `Transform`·`Http`를 더한다(지금은 TRIGGER·AI·ACTION·CONDITION 외엔 `UnknownNodeType`). DELAY·APPROVAL은 1c.
- `NodeType`을 망라하는 match는 `catalog.rs` 말고 `builder/edit.rs`의 `type_name`에도 있다(`_` 갈래 없음).
- 프런트는 정의를 따로 싣는다: `src/nodes/defs.ts`가 JSON을 하나씩 import하고 `node_type` 타입이 4종으로 닫혀 있다. 24개를 여기에도 더한다.
- `Catalog::embedded()`와 `stage1()`이 대상 24개 정의(`catalog/events/{transform,formatter,http}/*.json`)를 싣는다. 목록을 한 줄씩 늘릴지, 폴더째 싣는 방식으로 바꿀지는 빌더 몫.
- 노드 JSON의 `type` 문자열: `TRANSFORM`·`HTTP`(카탈로그 `node_type` 그대로). 채우는 곳은 빌더 `type_name`(`builder/edit.rs`)과 프런트 `newNode`(`src/nodes/defs.ts`).
- 정적 검증(`validate.rs`)에서 TRANSFORM·HTTP는 ACTION과 같은 규칙(참조 검사)을 탄다. 필수 칸은 빌더 전용 `validate_fields`만 보고 실행 전 검증에는 없으므로, **빈 필수 칸은 러너가 직접 막는다**(BE `required`와 같은 문구). ACTION 전용 동적 칸 처리(`validate.rs`의 `fieldsSource`)는 타지 않는다.
- `http.request` 정의의 `webhookCredentialId` 칸은 데스크톱에서 **숨긴다**: 설정 패널, 빌더 `fields` 응답, 빌더 `set`(`builder/fields.rs` — 카탈로그에 있는 칸이면 받으므로 여기서 거절해야 AI가 못 채운다). 이음 클라우드 읽기 전용 화면(`src/screens/Web.tsx`)은 config를 그대로 보여 주므로 거기서는 보여도 된다. `src/i18n.test.ts`가 `DEFS`의 모든 칸에 번역 키를 요구하니 키는 넣는다. 워크플로우 JSON에 값이 들어 있으면(클라우드에서 만든 것) 실행 시 실패: `저장된 웹훅 주소는 데스크톱에서 쓸 수 없어요. URL 칸에 주소를 직접 넣어 주세요.`

### 5.2 변환·포맷터 러너 (ieum-engine)

- 새 모듈을 `crates/ieum-engine`에 두고 `Runners::with_builtin()`에 등록한다 → 앱·CLI·적합성 묶음에 한 번에 들어간다. 파일 나누기는 빌더 몫(BE처럼 text·number·date·list로 나누는 걸 권한다).
- 시계는 주입한다(`ConditionRunner::at`과 같은 방식). 시간대 기본값은 Asia/Seoul. `Runners::with_builtin()`은 시스템 시계를 쓰고 적합성 묶음엔 시계를 넣을 길이 없으므로, `오늘`·`지금`에 기대는 케이스는 표 테스트에만 둔다.
- **입력은 이미 참조식이 치환된 config**(`NodeCtx.config`)에서 읽고 다시 치환하지 않는다(BE와 같음 — 이중 치환 금지).
- **`transform.map`만 예외**: BE는 원본 `mappings`의 1단계 문자열 값만 치환하고 중첩 값은 그대로 둔다. Rust도 원본 노드 config에서 1단계 문자열만 치환해 BE와 맞춘다.
- 공용 입력 규칙은 BE `FormatterInputs`를 그대로 옮긴다: 글자 상한 10만(UTF-16 단위로 센다), 목록 1만 개, 패턴 200자·사용자 날짜 패턴 100자, `choice`의 빈 값 = 기본값, `flag` 참 값(true·yes·y·예·1), `positiveInt`("3.0" 통과·"3.5" 실패), `list`(JSON 배열 → 실패하면 쉼표 분할).
- `condition.rs`에서 같이 쓰는 것: `bool_of`·`is_empty`는 그대로, 날짜 읽기(`date`와 그 반환 타입 `When`, KST 규칙 포함)는 타입까지 crate 안으로 연다. **숫자는 입력 문자열 정리 규칙(공백·쉼표·`원`·`%` 제거, 1000자 상한)만 같이 쓴다** — `condition.rs`의 `Decimal`은 비교 전용이라 연산이 없고 소수 자릿수를 버린다. 계산·표기는 임의 정밀도 십진수로 따로 한다.
- **공백 기준은 Java `String.strip()`/`Character.isWhitespace`다**: NBSP(U+00A0)·U+2007·U+202F·U+0085는 떼지 않고 U+001C~U+001F는 뗀다. Rust `trim()`과 반대라 직접 맞춘다. 쓰이는 곳: `text_trim`, `text_split` 조각, `lookup_table` 키, 쉼표 분할 원소, `positiveInt`, 시간대 칸, `text_case` title의 단어 시작. `text_length`의 words는 ASCII `\s+` 기준이다(전각 공백으로는 안 나뉜다).
- **실패 메시지는 BE 문구·형식 그대로**: `"<노드 title>: <이유> (입력: '<input 앞 50자…>')"`. title은 사용자가 붙인 이름이 아니라 카탈로그 title이다. 이 메시지는 실행 기록·화면용이다.
- **AI 빌더에게는 값이 섞이지 않은 코드만 준다**(내 판단 — 빌더는 지금도 `error_code`만 AI에게 넘기고 문구·값은 숨긴다. 코드가 없으면 24개 노드의 모든 실패가 `NODE_FAILED` 하나가 돼 AI가 스스로 못 고친다): `FIELD_EMPTY <칸>`(필수 칸 빔), `FIELD_INVALID <칸>`(값을 못 읽음·선택지 밖·패턴 오류·지원 안 하는 패턴), `TOO_LARGE`(글자·목록·자릿수 상한), `HTTP_STATUS <code>`, `HTTP_UNREACHABLE`(연결 실패·시간 초과·리다이렉트 초과). 그 밖은 `NODE_FAILED`. 코드 이름·문구는 빌더가 `builder/reply.rs`의 기존 규칙에 맞춰 다듬는다.
- 노드별 동작(칸·기본값·출력 모양·상한·오류 문구)은 BE 코드가 기준이다. 요약은 부록 A.

**새 의존성(내 판단, 빌더가 더 나은 걸 찾으면 바꿔도 된다)**

| 용도 | 크레이트 |
|---|---|
| 글자 수(grapheme) — `text_truncate`·`text_length` | `unicode-segmentation` |
| 임의 정밀도 십진수 — `number_math`·`number_format`·`number_currency` | `bigdecimal` |
| 뒤 확인 등 Java식 정규식 — `text_extract`·`text_extract_pattern` | `fancy-regex` |
| Markdown → HTML + 정화 | `pulldown-cmark` + `ammonia` |
| HTML → 글 — `text_strip_html` | HTML 파서(`scraper` 등) |

### 5.3 차이를 허용하는 곳과 그 규칙

- **`text_extract_pattern`(사용자 정규식)**: `fancy-regex`로 돌린다. 1초 마감 대신 역추적 상한(`backtrack_limit`)을 두고, 넘으면 BE와 같은 문구(`패턴 검사가 1초를 넘었어요` 대신 `패턴이 너무 복잡해요`)로 실패. 컴파일 안 되는 Java 전용 문법은 `정규식 패턴 오류예요: <설명>`. `\d`·`\w`·`\s`는 Java 기본값(ASCII)에 맞춘다.
- **`text_extract`(이메일·전화·URL·숫자)**: BE의 패턴과 URL 뒷정리(`trimUrl`)를 그대로 옮긴다. 여기는 차이를 허용하지 않는다 — BE 테스트 기대값을 전부 통과해야 한다.
- **`date_format`의 `custom`**: Java `DateTimeFormatter` 패턴 중 `y M d E H h m s a`와 따옴표 글자·구두점만 지원한다. 그 밖의 패턴 문자는 `지원하지 않는 날짜 패턴 문자예요: <문자>`로 실패. 요일·오전/오후는 한국어(`Locale.KOREAN`) 표기. 미리 정해진 표기 5종(korean·korean_time·date·datetime·iso)은 BE와 같아야 한다.
- **`text_markdown_to_html`**: 허용 태그·속성은 jsoup `Safelist.relaxed()`에 맞추고, 원문 HTML은 이스케이프, `javascript:` 등 위험한 주소는 제거. 공백·줄바꿈·속성 순서 차이는 허용. 주소 규칙은 BE(jsoup, baseUri 없음)에 맞춘다: `a[href]`는 ftp·http·https·mailto만, `img[src]`는 http·https만, **상대 주소는 속성째 지운다**(`ammonia` 기본값은 더 넓으니 좁혀야 한다 — BE 테스트에 이 케이스가 없어 빌더가 BE에서 직접 확인한다). 결과 앞뒤 공백 제거.
- **`text_strip_html`**: script·style 제거, 블록 요소·`<br>`에서 줄바꿈, 줄마다 앞뒤 공백 제거·빈 줄 제거. BE 테스트 기대값을 통과하는 선까지 맞춘다.
- **대소문자 변환**: Java와 Rust의 특수 글자(ß 등) 차이는 허용.
- 허용한 차이는 전부 `conformance/GAPS.md` "의도된 차이" 표에 한 줄씩 적는다.

### 5.4 HTTP 러너 (ieum-local)

`reqwest`가 이미 있는 `crates/ieum-local`에 둔다. 등록은 앱(`supervisor.rs`)·`ieum-dev run`(`ieum-cli/src/main.rs`) 두 곳, 적합성 묶음에서는 스텁(`input.nodeOutputs`).

- 칸: `method`(없으면 GET. GET·POST·PUT·DELETE, 대소문자 무시, 그 밖은 `지원하지 않는 HTTP 메서드: <대문자로 바꾼 값>`), `url`(필수), `headers`(dict), `body`.
- 주소: `http`·`https`만(`http·https URL만 허용됩니다: <url>`). 호스트 없으면 `URL에 호스트가 없습니다: <url>`. **내부망 차단 없음**(§4).
- 헤더: 기본 `Content-Type: application/json`, 사용자 값이 덮어쓴다.
- 본문: 값이 있으면 **JSON으로 직렬화해 보낸다 — 문자열도 JSON 문자열(`"abc"`)이 된다.** BE 동작 그대로다(같은 워크플로우가 받는 서버에 같은 본문을 보내야 한다). "값이 있으면"의 기준은 null이 아님이다 — 빈 문자열도 `""`로 나간다. 객체·배열은 깊게 치환한 뒤 직렬화. GET·DELETE도 본문이 있으면 보낸다.
- **본문 저장 모양(내 판단)**: 카탈로그 칸은 `text`라 편집기에서 글로 받는다. 받은 글이 JSON 객체·배열로 읽히면 **객체로 저장**하고(받는 서버가 JSON 객체를 받게), 아니면 문자열로 저장한다. 빌더 `set`도 같다. 두 모양 다 BE가 그대로 처리하므로 옮겨도 같은 본문이 나간다.
- 리다이렉트: 자동으로 따라가지 않고 직접 따라간다. 301·302·303·307·308이고 `Location`이 있을 때만, 최대 3번(넘으면 `리다이렉트가 3회를 넘었습니다 — 마지막 Location: ..`). 출처(scheme·host·port)가 바뀌면 `Authorization`·`Proxy-Authorization`·`Cookie`를 떼고, 이후 홉에서 원래 출처로 돌아와도 되살리지 않는다. 30x인데 `Location`이 없으면 실패가 아니라 그 상태 코드로 성공. `Location`을 못 읽으면 `리다이렉트 Location 형식이 올바르지 않습니다: <location>`. 301·302·303은 GET으로 바꾸고 본문을 버린다. 307·308은 유지.
- 시간: 연결 10초, 읽기 30초(읽다 30초 동안 아무것도 안 오면 실패 — 요청 전체 마감이 아니다. `reqwest`의 `.timeout()`은 전체 마감이라 다르다). 실행 취소(`cancel`)에 반응한다.
- 응답 본문: 1MB까지만 읽고 그 뒤는 버린다(실패 아님). 오류 응답 본문에도 같다.
- 출력 `{statusCode, body}`: 본문이 비거나 공백뿐이면 `null`, JSON **객체**면 객체, 그 밖(배열 포함)은 문자열.
- 4xx·5xx는 노드 실패: `HTTP <code>: <응답 본문>`. 연결 실패·시간 초과도 실패(문구는 빌더 몫, 한국어).
- 보안(실패로 치는 것): 실패 메시지·실행 기록에 요청 헤더 값(`Authorization` 등)을 남기지 않는다.

### 5.5 화면

- **팔레트**(`src/editor/Palette.tsx`): 그룹에 "변환"(TRANSFORM)·"HTTP"를 더한다. 포맷터 22개도 `node_type`이 TRANSFORM이라 종류로 거르면 한 그룹에 다 섞인다 — event 이름(`text_`·`number_`·`date_`·`list_`/`lookup_`)으로 글·숫자·날짜·목록 묶음을 나눠 보여 준다(묶는 모양은 빌더 몫, 이음 클라우드 편집기와 같은 순서·이름 우선).
- **아이콘·색**(`src/nodes/meta.tsx`): 지금은 `local.`이 아닌 노드를 "웹 앱 노드"(분홍·서비스 아이콘)로 그린다. TRANSFORM·HTTP는 내장 노드 색·아이콘으로 그린다(이음 클라우드 편집기의 색을 따른다). 이음 클라우드 읽기 전용 화면(`src/screens/Web.tsx`)도 같은 `DEFS`·`nodeMeta`·i18n을 쓰므로 거기 보이는 변환·HTTP 노드도 함께 바뀐다(의도한 것).
- **설정 패널**(`src/editor/ConfigPanel.tsx`): `dict` 칸(키·값 줄 추가/삭제 — `transform.map`의 `mappings`, `lookup_table`의 `table`, HTTP `headers`), `integer` 칸(숫자 입력). 지금은 dict가 문자열 입력으로 떨어져 깨지고 integer는 문자열 `"3"`으로 저장된다. 긴 글 `text` 칸은 이미 있다.
- **출력 칸 목록**(`src/nodes/defs.ts` `outputKeys`): `outputsFrom`(→ `transform.map`은 `mappings`의 키)을 읽는다. 뒤 노드의 참조 고르기에 쓰인다.
- **i18n**: 24개 노드의 이름·설명·칸·선택지를 한·영으로 넣는다. 한국어는 카탈로그 `title`·`description` 그대로. 선택지 키가 지금 `choice.<칸 key>.<id>`라 노드끼리 겹친다(`format`·`unit` 등) — event까지 넣은 키로 바꾸는 것은 빌더 몫. `src/i18n.test.ts`가 빠진 키를 잡는다.
- "내 PC" 알약은 `local.*`에만 그대로 둔다(새 노드엔 없음).

### 5.6 AI 빌더

- 빌더는 `Catalog::embedded()`를 허용 목록으로 쓰므로 24개가 자동으로 보인다. 손볼 곳:
  - `builder/fields.rs` 값 변환: `integer`·`dict` 칸을 타입대로 저장(지금은 list·boolean만).
  - `builder/guide.rs`·`chat/prompt.rs` 안내문에 변환·HTTP 노드가 있다는 것과 쓰임새 한두 줄. 트리거 안내("첫 노드는 폴더 트리거")는 그대로.
  - 노드 테스트와 [켜기] 확인 카드의 HTTP 처리는 §5.7.

### 5.7 HTTP 노드의 테스트와 [켜기] 확인

사용자 확인(11-15).

배경: 노드 테스트는 트리거부터 대상 노드까지 조상 전체를 실제로 돌린다(`builder/test.rs`). 파일 동작은 테스트 뒤 되돌리지만 HTTP 요청은 되돌릴 수 없다. 또 빌더는 AI에게 값(파일 이름·내용·AI 출력)을 주지 않는데, AI가 주소에 참조식을 넣고 테스트를 돌리면 사람 모르게 값이 밖으로 나갈 수 있다(파일 내용을 통한 프롬프트 주입 경로).

- **AI가 돌리는 테스트**: 자른 그래프 안에 HTTP 노드가 하나라도 있으면(GET 포함) 돌리기 전에 사람 확인 카드를 띄운다. 카드에는 노드마다 메서드와 주소(참조식은 "앞 노드 값"으로 표시)를 보여 준다. [보내기] / [테스트 없이 넘어가기].
- **[테스트 없이 넘어가기]**를 고르면 그 HTTP 노드와 그 뒤 노드는 "사람이 건너뜀" 상태가 되고, [켜기] 준비 검사(`finish.rs` `readiness`)는 이를 테스트한 것으로 친다. 결제·삭제 같은 요청을 테스트로 한 번 보내야만 켤 수 있는 일을 막는다.
- **사람이 편집기에서 누르는 테스트 실행**(`useTestRun`): 그래프에 GET이 아닌 HTTP 노드가 있으면 같은 확인을 한 번 띄운다. GET만 있으면 바로 돌린다.
- **[켜기] 확인 카드**: HTTP 노드마다 메서드·주소를 파일 동작과 같은 목록에 보여 준다. 손볼 곳은 `builder/finish.rs`(`actions`가 지금 `local.file_*` 세 개만 고른다), `builder/state.rs`(`PendingFinish`·`FinishAction`), `src/editor/chat/FinishCard.tsx`(`finish.action.<event>` 번역 키).
- 테스트 결과에는 "실제로 요청을 보냈어요"를 표시한다.

### 5.8 시험

- **BE 단위 테스트의 입력·기대값을 Rust 표 테스트로 옮긴다**: `TextFormatsTest`·`NumberFormatsTest`·`DateFormatsTest`·`ListFormatsTest`·`TransformNodeExecutorTest`(고정 시계 `2026-11-07T04:30:00Z`), `HttpNodeExecutorTest`(본문·리다이렉트·자격 헤더 제거 — SSRF·멱등성 케이스는 제외). §5.3에서 차이를 허용한 케이스는 기대값을 바꾸고 GAPS에 적는다.
- **카탈로그 ↔ 러너 양방향 검사**: 카탈로그의 `formatter.*`·`transform.*`마다 러너가 있고, 러너마다 카탈로그 정의가 있다(BE `FormatterCatalogTest`와 같은 역할).
- **적합성 묶음**(`conformance/`): 포맷터·`transform.map`이 낀 실행 케이스를 몇 개 더한다(참조식 → 포맷터 → 분기로 이어지는 흐름 위주). HTTP는 스텁.
- **HTTP**: 로컬 시험 서버로 리다이렉트·1MB 자르기·시간 초과·헤더 제거를 본다.
- 기존 단언 갱신: `ieum-engine/tests/catalog.rs`(events id 목록 비교, 그리고 `load_errors_do_not_panic`이 `"node_type": "HTTP"`를 모르는 종류의 예로 쓴다 — `DELAY` 등으로 바꾼다. 새 24개가 내장됐는지 보는 단언도 더한다), 빌더 events 개수(`ieum-local/tests/builder_edit.rs`).
- 끝-끝 확인(사람): 앱에서 폴더 트리거 → AI → 포맷터 → HTTP(로컬 시험 서버) 워크플로우를 만들고 돌린다.

## 6. 두 엔진의 차이 다루기

- 기준은 BE다. Rust가 BE 테스트 기대값을 못 맞추면: (1) 맞춘다 → (2) §5.3에 든 허용 범위면 GAPS에 적고 넘어간다 → (3) 그 밖이면 조용히 다른 값을 내지 말고 실패시킨다.
- `GAPS.md`의 줄은 4(옮길 때 검증)에서 "이 노드는 이음 클라우드에서 결과가 조금 다를 수 있어요" 경고의 재료가 된다. 그래서 **노드 event·칸 단위로** 적는다.

## 7. 빌더 몫

- 모듈·파일 나누기, 카탈로그 싣는 방식, 크레이트 선택(§5.2 표는 권고).
- 팔레트 묶음 모양, 아이콘 고르기, dict 편집 칸 모양, i18n 키 이름.
- HTTP 연결 오류 문구, 시험 서버 구성.
- HTTP 테스트 전 사람 확인의 화면 모양.
- 작업 순서(권고: 카탈로그·종류 → 포맷터 러너 + 표 테스트 → HTTP → 화면 → 빌더).

## 부록 A. 노드 24개 요약

기준은 BE 코드. 아래는 길잡이다. 출력은 따로 적지 않으면 `{output: 문자열}`.

**변환**
- `transform.map` — `mappings`(dict). 출력 = mappings의 키 그대로. 1단계 문자열 값만 치환.

**글**
- `formatter.text_case` — `mode` upper·lower·capitalize·title(기본 title). title은 공백 뒤 첫 글자만 대문자, 나머지 소문자. capitalize는 첫 글자만 대문자로 바꾸고 나머지는 건드리지 않는다.
- `formatter.text_trim` — 앞뒤 공백 제거(Java `strip()` 기준, §5.2).
- `formatter.text_replace` — `find`(필수)·`replace`. 글자 그대로. `[:space:]`·`[:newline:]`·`[:tab:]` 표기 지원. 결과 10만 자 상한.
- `formatter.text_truncate` — `maxLength`(1 이상 정수)·`ellipsis`. grapheme 기준. ellipsis면 `max-1`자 + `…`.
- `formatter.text_split` — `separator`(기본 `,`)·`part` first·second·last·all(기본 first). 조각마다 앞뒤 공백 제거. second가 없으면 `""`. all이면 목록(빈 입력은 `[]`, 1만 개 넘으면 실패).
- `formatter.text_default` — 비어 있으면(`""`·공백·`[]`·`{}`) `defaultValue`(필수 칸).
- `formatter.text_extract` — `kind` email·phone·url·number(필수)·`all`. 없으면 `""`, all이면 목록.
- `formatter.text_extract_pattern` — `pattern`(200자)·`all`. 일치한 부분 전체.
- `formatter.text_strip_html` — HTML → 글.
- `formatter.text_markdown_to_html` — Markdown → 정화된 HTML.
- `formatter.text_url_encode` — `direction` encode·decode(기본 encode). encode는 Java `URLEncoder` 안전 문자(`A-Za-z0-9.-*_`) + 공백은 `%20`. decode는 `+` → 공백.
- `formatter.text_length` — `unit` chars(grapheme)·words(ASCII 공백 기준). 출력은 **정수**.

**숫자**
- `formatter.number_math` — `operation` add·subtract·multiply·divide·negate, `value`. 나눗셈은 소수 10자리 반올림(HALF_UP). 정수·소수 각 1000자리 상한. 출력은 뒤 0을 뗀 평문 문자열.
- `formatter.number_format` — `thousands` comma·period·space·none, `decimal` period·comma. 두 기호가 같으면 실패. 소수 자릿수 유지.
- `formatter.number_currency` — `currency` KRW·USD·JPY·EUR·CNY·GBP, `style` korean·symbol. KRW·JPY 0자리, 나머지 2자리(HALF_UP). `-12,000원`·`-₩12,000`. korean 표기는 KRW에만 적용되고 다른 통화는 korean이어도 기호 표기. JPY·CNY 기호는 둘 다 `¥`.
- `formatter.number_phone` — `format` dash·international·digits. 한국 번호만.

**날짜**
- `formatter.date_format` — `format` korean·korean_time·date·datetime·iso·custom, `customPattern`, `fromTimezone`·`toTimezone`(기본 Asia/Seoul). 초 아래 버림. 날짜만 있는 입력은 `fromTimezone`의 00:00, `오늘`은 `fromTimezone`과 무관하게 KST 날짜. iso는 UTC면 `+00:00`이 아니라 `Z`로 끝난다.
- `formatter.date_add` — `expression`(`+3일 -2시간` 식, 부호 필수, 20항·6자리). 입력이 날짜만이고 **모든 항이 년·월·주·일일 때만** `yyyy-MM-dd`. 시간·분·초 항이 하나라도 있으면 KST 00:00 기준으로 계산해 `+09:00` ISO.
- `formatter.date_compare` — `other`. 출력 `{days, hours, minutes, isBefore}`(other − input, 0 쪽으로 버림). 둘 중 하나라도 날짜만이면 KST 날짜 차이로만 계산(hours = days×24, minutes = days×1440).

**목록**
- `formatter.lookup_table` — `table`(dict 또는 JSON 문자열)·`fallback`. 키는 앞뒤 공백 뗀 input, 대소문자 구분. 키가 비면 fallback이 아니라 `""`. 값이 문자열이 아니면 JSON 글, null이면 `""`. 표 1만 개 상한.
- `formatter.list_pick` — `position` first·last·nth, `index`(1부터, 기본 position은 first). 출력은 원소 그대로 — 단 JSON 글에서 읽힌 소수는 평문 문자열(`[1.5]` → `"1.5"`). 빈 목록·범위 밖은 실패.
- `formatter.list_to_text` — `separator`(기본 `, `). 결과 10만 자 상한.

**HTTP**
- `http.request` — §5.4.
