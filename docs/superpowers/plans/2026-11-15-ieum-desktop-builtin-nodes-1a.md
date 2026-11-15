# 변환·포맷터·HTTP 노드 (1a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 데스크톱 엔진이 `transform.map`·`formatter.*` 22개·`http.request`를 실행하고, 편집기와 AI 빌더에서 쓸 수 있게 한다.

**Architecture:** 변환·포맷터는 `ieum-engine`의 새 `formatter` 모듈(순수 함수 + 시계 주입)로 옮기고 `Runners::with_builtin()`에 등록한다. HTTP는 `reqwest`가 있는 `ieum-local`에 러너를 두고 앱·CLI에서 등록한다. 기준 구현은 BE Java이고, 결과는 의미가 같게 맞추되 허용한 차이는 `conformance/GAPS.md`에 적는다.

**Tech Stack:** Rust(ieum-engine·ieum-local·ieum-cli·src-tauri), React 19 + TS + vitest, 새 크레이트 `unicode-segmentation`·`bigdecimal`·`fancy-regex`·`pulldown-cmark`·`ammonia`·`scraper`.

**Spec:** `docs/superpowers/specs/2026-11-15-ieum-desktop-builtin-nodes-1a-design.md` — 실행자는 스펙과 이 계획을 같이 읽는다.

## Global Constraints

- **기준은 BE 코드다.** `ieum-backend/workflow-core/src/main/java/com/ieum/workflowcore/engine/executor/`(`Formatter`·`FormatterInputs`·`TextFormats`·`NumberFormats`·`DateFormats`·`ListFormats`·`ConditionValues`·`TransformNodeExecutor`·`HttpNodeExecutor`)와 같은 경로 `src/test`. 경로 접두 `BE=/Users/dobee/WorkSpace/IEUMWorkSpace/ieum-backend`. **이 계획·스펙이 BE 코드와 다르면 BE 코드를 믿고 보고한다.** BE는 읽기 전용.
- BE 테스트 기대값을 못 맞추면: 맞춘다 → 스펙 §5.3 허용 범위면 `conformance/GAPS.md` "의도된 차이" 표에 event·칸 단위로 한 줄 → 그 밖이면 조용히 다른 값을 내지 말고 실패시킨다.
- 실패 메시지(화면용): `"<카탈로그 title>: <이유> (입력: '<input 앞 50자…>')"`, 이유 문구는 BE 한국어 문구 그대로.
- `NodeError.code`(AI용)는 `FIELD_EMPTY <칸>`·`FIELD_INVALID <칸>`·`TOO_LARGE`·`HTTP_STATUS <code>`·`HTTP_UNREACHABLE`만. 값·경로·주소를 넣지 않는다.
- 상한: 글 100,000(UTF-16 단위), 목록 10,000개, 정규식 패턴 200자, 날짜 사용자 패턴 100자, 숫자 정수·소수 각 1000자리, HTTP 응답 본문 1,048,576바이트.
- 시간대 기본 Asia/Seoul. 공백 기준은 Java `String.strip()`(`condition.rs`의 `strip`).
- HTTP: 내부망 차단 없음, 리다이렉트 최대 3회, 연결 10초·읽기 30초.
- 기존 6개 노드의 검증·실행·적합성 20케이스는 그대로 통과해야 한다.
- 카탈로그 서브모듈(670739f)은 올리지 않는다. `catalog/`·BE·`ieum-frontend`는 수정 금지.
- 프런트는 prettier를 돌리지 않는다(설정 없음). Rust는 `cargo fmt`·`cargo clippy --workspace` 통과.
- 커밋은 작업자가 하지 않는다 — 태스크마다 코디네이터가 한다. 메시지는 한국어 `feat: …`, 본문에 `#5`, `Co-Authored-By` 없음.
- 검증 명령: `cargo test --workspace`, `npm test`, `npm run build`(tsc). 빌드 캐시는 `CARGO_TARGET_DIR=~/.cache/ieum-target-A`(다른 워크트리와 동시에 쓰지 않는다).

## Review Focus

1. **참조식으로 들어온 목록·대응표는 JSON 문자열이다** — `list_pick`·`list_to_text`·`lookup_table`에 `"[1.5, {\"a\":1}, null]"`·`"{\"a\":1}"` 문자열이 와도 목록·표로 읽혀야 한다(Task 8).
2. **사용자 정규식이 폭주한다** — `(a+)+$`에 `a`×30 + `!` 같은 입력에서 1초 안에 `패턴이 너무 복잡해요`로 끝나야 한다(Task 4).
3. **HTTP 응답이 JSON도 UTF-8도 아니다** — 바이너리·잘린 멀티바이트 본문에서 패닉 없이 문자열(손실 변환)로 나와야 하고, 1MB에서 잘려도 실패가 아니다(Task 9).
4. **확인 창을 기다리는 사이 워크플로우가 바뀐다** — HTTP 테스트 확인 대기 중 사람이 캔버스를 고치면 옛 그래프로 요청을 보내지 않고 `STALE`로 끝나야 한다(Task 11).
5. **KST가 아닌 시간대의 서머타임 경계** — `date_format`에 `fromTimezone=America/New_York`, 틈·겹침 시각을 넣어도 패닉 없이 Java 규칙(겹치면 이른 쪽, 틈이면 틈 전 오프셋)으로 읽혀야 한다(Task 7).

---

## 파일 구조

```
crates/ieum-engine/src/
  catalog.rs            NodeType에 Transform·Http, embedded/stage1에 24개
  run.rs                NodeCtx에 outputs 추가
  condition.rs          헬퍼 pub(crate) 열기, clean_number 추출, with_builtin 확장
  transform.rs          (새) TransformMapRunner
  formatter/
    mod.rs              (새) FormatterRunner, apply, OPS 합치기, 메시지·코드
    inputs.rs           (새) In — BE FormatterInputs
    text.rs             (새) case·trim·replace·truncate·split·default·url_encode·length
    extract.rs          (새) text_extract·text_extract_pattern
    html.rs             (새) text_strip_html·text_markdown_to_html
    number.rs           (새) math·format·currency·phone
    date.rs             (새) format·add·compare
    list.rs             (새) lookup_table·list_pick·list_to_text
crates/ieum-engine/tests/formatter_{text,extract,html,number,date,list}.rs, transform.rs
crates/ieum-local/src/http.rs            (새) HttpRunner
crates/ieum-local/tests/http.rs          (새)
crates/ieum-local/src/builder/           edit·fields·reply·test·finish·host·state·guide, chat/prompt.rs
src-tauri/src/http_confirm.rs            (새) 사람 확인 대기
src/nodes/{defs.ts,meta.tsx}, src/editor/{Palette,ConfigPanel}.tsx, src/i18n.ts
src/editor/DictField.tsx                 (새)
src/editor/chat/HttpTestCard.tsx         (새)
```

의존 순서: 1 → 2 → (3·4·5·6·7·8 병렬 가능, 각자 자기 파일만) → 9 → 10 → 11 → 12 → 13 → 14 → 15.

---

### Task 1: 노드 종류와 카탈로그 24개

**Files:**
- Modify: `crates/ieum-engine/src/catalog.rs`(6-12, 123-155, 166-177, 215-239), `crates/ieum-engine/src/validate.rs`(NodeType match 지점), `crates/ieum-local/src/builder/edit.rs:25-32`
- Test: `crates/ieum-engine/tests/catalog.rs`

**Interfaces:**
- Produces: `NodeType::{Transform, Http}`. `Catalog::embedded()`·`Catalog::stage1()`이 30개 event를 싣는다(기존 6 + `transform.map` + `formatter.*` 22 + `http.request`). 빌더 `type_name`은 `"TRANSFORM"`·`"HTTP"`를 낸다.

- [ ] **Step 1: 실패하는 테스트** — `tests/catalog.rs`에:

```rust
#[test]
fn embedded_has_builtin_24() {
    let c = Catalog::embedded().unwrap();
    assert_eq!(c.events().len(), 30);
    assert_eq!(c.get("transform.map").unwrap().node_type, NodeType::Transform);
    assert_eq!(c.get("formatter.date_format").unwrap().node_type, NodeType::Transform);
    assert_eq!(c.get("http.request").unwrap().node_type, NodeType::Http);
    assert_eq!(c.events().iter().filter(|e| e.event.starts_with("formatter.")).count(), 22);
    assert_eq!(c.get("transform.map").unwrap().outputs_from.as_deref(), Some("config.mappings"));
}
```

  같은 파일의 기존 단언도 고친다: `load_errors_do_not_panic`(79-84)의 모르는 종류 예 `"HTTP"` → `"DELAY"`, events id 목록 비교(123-136)는 30개 기준으로. `stage1` 테스트도 30개.

- [ ] **Step 2:** `cargo test -p ieum-engine --test catalog` → FAIL(`UnknownNodeType`/variant 없음).
- [ ] **Step 3: 구현** — `NodeType`에 `Transform`·`Http`, 파싱에 `"TRANSFORM"`·`"HTTP"`. `embedded()`는 `src!` 24줄 추가, `stage1()`은 `catalog/events/{transform,formatter,http}/*.json`을 읽어 event = `<폴더>.<파일 이름>`. `validate.rs`에서 Transform·Http는 Action과 같은 갈래(단 ACTION 전용 `fieldsSource` 동적 칸 처리는 Action만). `type_name`에 두 갈래.
- [ ] **Step 4:** `cargo test --workspace` → PASS(빌더 events 개수 단언 `crates/ieum-local/tests/builder_edit.rs:313`의 `6`은 `30`으로). `cargo test -p ieum-cli --test conformance` → 기존 20케이스 PASS.
- [ ] **Step 5: 커밋** `feat: 엔진이 변환·HTTP 노드 종류와 정의 24개를 읽음`

---

### Task 2: 포맷터 뼈대·입력 규칙·transform.map

**Files:**
- Create: `crates/ieum-engine/src/formatter/{mod,inputs,text,extract,html,number,date,list}.rs`(뒤 6개는 빈 `OPS`만), `crates/ieum-engine/src/transform.rs`, `crates/ieum-engine/tests/transform.rs`, `crates/ieum-engine/tests/formatter_inputs.rs`
- Modify: `crates/ieum-engine/src/{lib.rs,run.rs,condition.rs}`, `crates/ieum-engine/Cargo.toml`, `crates/ieum-engine/tests/condition.rs:569,586,598`(NodeCtx 리터럴)

**Interfaces:**
- Consumes: Task 1의 카탈로그.
- Produces:

```rust
// run.rs — NodeCtx에 필드 추가(치환 전 상류 출력)
pub struct NodeCtx<'a> { pub node: &'a Node, pub config: Map<String, Value>, pub outputs: &'a Outputs, pub events: &'a EventSink, pub cancel: &'a CancellationToken }

// condition.rs — pub(crate)로 연다: strip, java_str, bool_of, is_empty, kst, date, When{instant, day, date_only}(필드도)
pub(crate) fn clean_number(value: &Value) -> Option<String>; // number()에서 추출: 공백·쉼표·원·% 제거, 빈 값·1000자 초과는 None

// formatter/inputs.rs
pub(crate) struct Fail { pub reason: String, pub code: &'static str, pub field: Option<&'static str> }
pub(crate) type R<T> = Result<T, Fail>;
pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Fail; // code "FIELD_INVALID"
pub(crate) fn too_large(reason: impl Into<String>) -> Fail;                    // code "TOO_LARGE"
pub(crate) const MAX_TEXT: usize = 100_000; pub(crate) const MAX_ITEMS: usize = 10_000;
pub(crate) const MAX_PATTERN: usize = 200;  pub(crate) const MAX_CUSTOM_PATTERN: usize = 100;
pub(crate) struct In<'a>(pub &'a Map<String, Value>);
impl In<'_> {
    pub fn text(&self, key: &'static str) -> R<String>;                       // null→"", Map·List→JSON, 10만 자 초과 TOO_LARGE
    pub fn required(&self, key: &'static str, title: &str) -> R<String>;      // 빈 값 FIELD_EMPTY "'<title>' 칸이 비어 있어요"
    pub fn choice(&self, key: &'static str, default: Option<&'static str>, allowed: &[&'static str]) -> R<&'static str>;
    pub fn flag(&self, key: &str) -> bool;                                    // bool_of == Some(true)
    pub fn positive_int(&self, key: &'static str, title: &str) -> R<usize>;
    pub fn list(&self, key: &'static str) -> R<Vec<Value>>;
}

// formatter/mod.rs
pub(crate) type Op = fn(&In, DateTime<Utc>) -> R<Map<String, Value>>;
pub(crate) type Entry = (&'static str /*event*/, &'static str /*BE Formatter.OPS title*/, Op);
pub(crate) fn out(v: impl Into<Value>) -> Map<String, Value>;                 // {"output": v}
pub fn events() -> Vec<&'static str>;                                         // 등록된 event 전부
pub fn apply(event: &str, config: &Map<String, Value>, now: DateTime<Utc>) -> Result<Map<String, Value>, NodeError>;
pub struct FormatterRunner { /* now: Option<DateTime<Utc>> */ }
impl FormatterRunner { pub fn at(now: DateTime<Utc>) -> Self }
// 각 범주 파일: pub(crate) const OPS: &[Entry]

// transform.rs
pub struct TransformMapRunner;
```

- `Runners::with_builtin()`은 `branch.if` + `formatter::events()` 전부(`FormatterRunner::default()` 하나를 공유) + `transform.map`.

- [ ] **Step 1: 실패하는 테스트**

```rust
// tests/formatter_inputs.rs — text_trim을 Task 3 전에 쓸 수 없으니 apply의 공통 동작만
#[test] fn unknown_event_message() {
    let e = apply("formatter.nope", &Map::new(), now()).unwrap_err();
    assert_eq!(e.message, "지원하지 않는 포맷터예요: formatter.nope");
}
// tests/transform.rs
#[tokio::test] async fn map_renders_only_first_level_strings() {
    // mappings = {"a": "{{nodes.t.output.x}}", "b": {"c": "{{nodes.t.output.x}}"}, "n": 3}, outputs t = {x: "1"}
    // → {"a": "1", "b": {"c": "{{nodes.t.output.x}}"}, "n": 3}
}
#[tokio::test] async fn map_without_mappings_is_empty() { /* → {} */ }
#[test] fn with_builtin_registers_transform_map() { assert!(Runners::with_builtin().contains_key("transform.map")); }
```

  `In`의 단위 테스트는 `inputs.rs` 안 `#[cfg(test)]`에: `text`가 이모지 50,001개(UTF-16 100,002)에서 `TOO_LARGE`·`'input' 칸이 10만 자를 넘어요`, `positive_int("3.0")`=3·`("3.5")` 실패, `list("a,,b")` 3개·`list(" [1,2] x")`는 쉼표 분할로, `list("")` 빈 목록, `choice("")`=기본값·기본값 없으면 `'<key>' 칸을 골라 주세요`, `flag("예")`.

- [ ] **Step 2:** `cargo test -p ieum-engine` → FAIL.
- [ ] **Step 3: 구현** — `apply`는 OPS에서 event를 찾고 `Fail`을 `NodeError { message: "<title>: <reason> (입력: '<input 앞 50자…>')", code }`로 바꾼다. 입력 요약은 `config["input"]`의 글 앞 50자(서로게이트 쌍을 가르지 않게 char 단위, 넘으면 `…`). code는 `field`가 있으면 `"<code> <field>"`. `TransformMapRunner`는 `ctx.node.config["mappings"]`(원본)의 1단계 문자열 값만 `render_deep(&Value::String(..), ctx.outputs)`로 치환, 나머지 값은 그대로. `Cargo.toml`에 이후 태스크가 쓸 의존성을 한 번에 넣는다: `unicode-segmentation`, `bigdecimal`, `fancy-regex`, `pulldown-cmark`, `ammonia`, `scraper`(Task 3~8이 `Cargo.toml`을 건드리지 않게).
- [ ] **Step 4:** `cargo test --workspace` → PASS.
- [ ] **Step 5: 커밋** `feat: 포맷터 러너 뼈대와 transform.map`

---

### Task 3: 글 포맷터 8개

**Files:** Modify `crates/ieum-engine/src/formatter/text.rs` · Test `crates/ieum-engine/tests/formatter_text.rs`

**Interfaces:** Consumes Task 2의 `In`·`Entry`·`out`. Produces `text::OPS` = `formatter.text_{case,trim,replace,truncate,split,default,url_encode,length}`(title은 BE `Formatter.OPS` 그대로).

- [ ] **Step 1: 실패하는 테스트** — `BE/workflow-core/src/test/.../executor/TextFormatsTest.java`에서 이 8개 event의 케이스를 **전부** `apply(event, &cfg, now)` 표 테스트로 옮긴다(입력·기대값 그대로, 고정 시계 `2026-11-07T04:30:00Z`). 반드시 들어갈 것:

```rust
case("formatter.text_case", json!({"input":"hello wORLD"}), "Hello World");            // 기본 title
case("formatter.text_case", json!({"input":"hello wORLD","mode":"capitalize"}), "Hello wORLD");
case("formatter.text_trim", json!({"input":"\u{a0}a\u{1f} "}), "\u{a0}a");             // NBSP는 남고 U+001F는 뗀다
case("formatter.text_replace", json!({"input":"a b","find":"[:space:]","replace":"[:newline:]"}), "a\nb");
case("formatter.text_truncate", json!({"input":"👨‍👩‍👧가나","maxLength":2,"ellipsis":true}), "👨‍👩‍👧…");
case_v("formatter.text_split", json!({"input":"","part":"all"}), json!([]));
case("formatter.text_split", json!({"input":"a","part":"second"}), "");
case("formatter.text_default", json!({"input":" [] ","defaultValue":"x"}), "x");
fails("formatter.text_default", json!({"input":"a"}), "'기본값' 칸이 비어 있어요", "FIELD_EMPTY defaultValue");
case("formatter.text_url_encode", json!({"input":"a b~*"}), "a%20b%7E*");
case_v("formatter.text_length", json!({"input":"가 나\u{3000}다","unit":"words"}), json!(2)); // 전각 공백으로는 안 나뉜다
```

- [ ] **Step 2:** `cargo test -p ieum-engine --test formatter_text` → FAIL.
- [ ] **Step 3: 구현** — 스펙 부록 A + BE `TextFormats.java`. grapheme은 `unicode-segmentation`. `text_replace`·`text_split(all)`은 결과 상한(10만 자·1만 개)을 만들기 전에 계산해 넘으면 `TOO_LARGE`.
- [ ] **Step 4:** 같은 명령 → PASS. Java `BreakIterator`와 달라 못 맞춘 케이스가 있으면 `GAPS.md`에 적는다.
- [ ] **Step 5: 커밋** `feat: 글 포맷터 8개`

---

### Task 4: 뽑기 포맷터 2개

**Files:** Modify `formatter/extract.rs` · Test `tests/formatter_extract.rs`

**Interfaces:** Produces `extract::OPS` = `formatter.text_extract`, `formatter.text_extract_pattern`.

- [ ] **Step 1: 실패하는 테스트** — `TextFormatsTest.java`의 extract·extractPattern 케이스 전부(이메일·전화·URL 뒷정리·숫자). 차이 허용 없음(`text_extract`). 추가:

```rust
#[test] fn runaway_pattern_ends_fast() {
    let input = format!("{}!", "a".repeat(30));
    let t = Instant::now();
    let e = apply("formatter.text_extract_pattern", &cfg(json!({"input": input, "pattern": "(a+)+$"})), now()).unwrap_err();
    assert!(e.message.contains("패턴이 너무 복잡해요"));
    assert!(t.elapsed() < Duration::from_secs(1));
    assert_eq!(e.code.as_deref(), Some("FIELD_INVALID pattern"));
}
#[test] fn bad_pattern_message() { /* pattern "(" → "정규식 패턴 오류예요: " 로 시작, FIELD_INVALID pattern */ }
#[test] fn ascii_digit_class() { /* pattern r"\d+" , input "１２3" → "3" (전각 숫자는 안 잡힌다) */ }
```

- [ ] **Step 2:** → FAIL.
- [ ] **Step 3: 구현** — `fancy-regex`. BE의 EMAIL·PHONE·URL·NUMBER 패턴을 그대로 옮기되 `\d`는 `[0-9]`, 소유격 수량자(`++`)는 원자 그룹으로. `trimUrl` 뒷정리 그대로. 사용자 패턴은 `RegexBuilder::backtrack_limit`로 상한을 두고(1초 안에 끝나는 값으로 실측해 정한다) 넘으면 `패턴이 너무 복잡해요`. `\d`·`\w`·`\s`가 ASCII로 동작하게 한다(유니코드 끔 또는 패턴 앞에 플래그). 지원 못 하는 Java 문법은 `GAPS.md`에.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5: 커밋** `feat: 뽑기 포맷터(이메일·전화·URL·숫자·패턴)`

---

### Task 5: HTML 포맷터 2개

**Files:** Modify `formatter/html.rs` · Test `tests/formatter_html.rs`

**Interfaces:** Produces `html::OPS` = `formatter.text_strip_html`, `formatter.text_markdown_to_html`.

- [ ] **Step 1: 실패하는 테스트** — `TextFormatsTest.java`의 stripHtml·markdownToHtml 케이스 전부. `text_strip_html`은 기대값 그대로. `text_markdown_to_html`은 공백·줄바꿈·속성 순서를 정규화해 비교하는 도우미로. 보안 케이스 추가:

```rust
#[test] fn markdown_blocks_script_and_bad_urls() {
    let h = md("<script>alert(1)</script>\n\n[a](javascript:alert(1)) [b](/rel) [c](https://ok.example) ![i](data:x) [t](tel:1)");
    assert!(!h.contains("<script"));            // 원문 HTML은 이스케이프된다
    assert!(!h.contains("javascript:"));
    assert!(!h.contains("href=\"/rel\""));       // 상대 주소는 속성째 없다
    assert!(h.contains("href=\"https://ok.example\""));
    assert!(!h.contains("data:x") && !h.contains("tel:1"));
}
```

- [ ] **Step 2:** → FAIL.
- [ ] **Step 3: 구현** — Markdown은 `pulldown-cmark`(원문 HTML 이벤트는 글로 이스케이프) → `ammonia::Builder`로 정화. 허용 태그·속성은 jsoup 1.23.2 `Safelist.relaxed()` 목록에 맞추고, `url_schemes`는 a[href] `ftp·http·https·mailto`, img[src] `http·https`, 상대 주소 거부(`UrlRelative::Deny`). 상대 주소·`tel:` 동작은 BE에서 직접 확인한다(작은 Java 테스트를 임시로 돌려 보거나 jsoup 소스 확인 — BE 레포에 파일을 남기지 않는다). `text_strip_html`은 `scraper`로 파싱, script·style 제거, 블록 요소 시작·끝과 `<br>`에서 줄바꿈, 줄마다 `strip`, 빈 줄 제거, `\n`으로 잇기. 블록 요소 목록은 jsoup `Tag`의 block 목록.
- [ ] **Step 4:** → PASS. 허용한 차이는 `GAPS.md`에.
- [ ] **Step 5: 커밋** `feat: HTML 포맷터(태그 없애기·Markdown 변환)`

---

### Task 6: 숫자 포맷터 4개

**Files:** Modify `formatter/number.rs` · Test `tests/formatter_number.rs`

**Interfaces:** Consumes `condition::clean_number`. Produces `number::OPS` = `formatter.number_{math,format,currency,phone}`.

- [ ] **Step 1: 실패하는 테스트** — `NumberFormatsTest.java` 케이스 전부(34-133행). 반드시:

```rust
case("formatter.number_math", json!({"input":"10","operation":"divide","value":"3"}), "3.3333333333");
fails("formatter.number_math", json!({"input":"1","operation":"divide","value":"0"}), "0으로 나눌 수 없어요", "FIELD_INVALID value");
fails("formatter.number_math", json!({"input":"0e-999999999","operation":"negate"}), "숫자가 너무 커요(정수·소수 각 1000자리까지)", "TOO_LARGE");
case("formatter.number_format", json!({"input":"1234.50"}), "1,234.50");                 // 소수 자릿수 유지
fails("formatter.number_format", json!({"input":"1","thousands":"comma","decimal":"comma"}), "천 단위 구분과 소수점 기호가 같아요", "FIELD_INVALID decimal");
case("formatter.number_currency", json!({"input":"-12000"}), "-12,000원");
case("formatter.number_currency", json!({"input":"12","currency":"USD","style":"korean"}), "$12.00");
case("formatter.number_currency", json!({"input":"-0"}), "0원");
case("formatter.number_phone", json!({"input":"+82 10-1234-5678","format":"dash"}), "010-1234-5678");
fails("formatter.number_phone", json!({"input":"123"}), "한국 전화번호로 읽지 못했어요", "FIELD_INVALID input");
```

- [ ] **Step 2:** → FAIL.
- [ ] **Step 3: 구현** — `bigdecimal`. 자릿수 상한은 계산 전 원문 표기와 결과 둘 다 검사. 반올림은 HALF_UP. 전화 패턴은 `regex`(`[0-9]`), 전체 일치.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5: 커밋** `feat: 숫자 포맷터 4개`

---

### Task 7: 날짜 포맷터 3개

**Files:** Modify `formatter/date.rs` · Test `tests/formatter_date.rs`

**Interfaces:** Consumes `condition::{date, When, kst}`. Produces `date::OPS` = `formatter.date_{format,add,compare}`.

- [ ] **Step 1: 실패하는 테스트** — `DateFormatsTest.java` 케이스 전부(37-167행). 반드시:

```rust
case("formatter.date_format", json!({"input":"2026-11-07"}), "2026년 11월 7일 (토)");
case("formatter.date_format", json!({"input":"2026-11-07T04:30:00Z","format":"iso","toTimezone":"UTC"}), "2026-11-07T04:30:00Z");
case("formatter.date_format", json!({"input":"2026-11-07 13:30","format":"custom","customPattern":"yy/M/d a h:mm"}), "26/11/7 오후 1:30");
fails("formatter.date_format", json!({"input":"2026-11-07","format":"custom","customPattern":"yyyy-QQ"}), "지원하지 않는 날짜 패턴 문자예요: Q", "FIELD_INVALID customPattern");
fails("formatter.date_format", json!({"input":"2026-11-07","toTimezone":"Mars/Base"}), "알 수 없는 시간대예요", "FIELD_INVALID toTimezone");
case("formatter.date_add", json!({"input":"2026-01-31","expression":"+1달"}), "2026-02-28");
case("formatter.date_add", json!({"input":"2026-01-31","expression":"+1일 +2시간"}), "2026-02-01T02:00:00+09:00");
case_v("formatter.date_compare", json!({"input":"2026-11-07","other":"2026-11-09T23:00:00+09:00"}),
       json!({"days":2,"hours":48,"minutes":2880,"isBefore":true}));
// Review Focus 5 — 서머타임 틈·겹침(뉴욕 2026-03-08 02:30은 없는 시각, 2026-11-01 01:30은 두 번)
#[test] fn dst_gap_and_overlap_do_not_panic() {
    assert_eq!(fmt(json!({"input":"2026-03-08 02:30","fromTimezone":"America/New_York","toTimezone":"UTC","format":"datetime"})), "2026-03-08 07:30");
    assert_eq!(fmt(json!({"input":"2026-11-01 01:30","fromTimezone":"America/New_York","toTimezone":"UTC","format":"datetime"})), "2026-11-01 05:30");
}
```

- [ ] **Step 2:** → FAIL.
- [ ] **Step 3: 구현** — 시간대는 `chrono-tz` 이름 + `+09:00`·`UTC`·`Z` 오프셋 표기. 시간대 없는 입력은 `fromTimezone`으로(Java `atZone` 규칙: 겹치면 이른 쪽, 틈이면 틈 전 오프셋 — `condition::kst`를 시간대 인자를 받게 일반화). 초 아래 버림. custom 패턴은 `y M d E H h m s a`·따옴표 글자·구두점·공백만 받는 작은 변환기(글자 반복 수로 자릿수: `yy`·`yyyy`, `M`·`MM`, `E`=`토`, `a`=`오전`/`오후`). 100자 초과 `TOO_LARGE`. `date_add` 식은 `\s*([+-])(\d+)\s*(\p{L}+)\s*` 반복, 20항·6자리, 단위 표는 BE `DateFormats.java:31-41`.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5: 커밋** `feat: 날짜 포맷터 3개`

---

### Task 8: 목록 포맷터 3개 + 묶음 검사

**Files:**
- Modify: `formatter/list.rs`, `conformance/GAPS.md`, `crates/ieum-cli/tests/conformance.rs:37`(케이스 수)
- Create: `conformance/formatter-chain/{workflow,input,expected}.json`, `conformance/transform-map/{…}.json`, `conformance/formatter-failure/{…}.json`
- Test: `tests/formatter_list.rs`, `tests/catalog.rs`

**Interfaces:** Produces `list::OPS` = `formatter.lookup_table`, `formatter.list_pick`, `formatter.list_to_text`. 이후 `formatter::events()`는 22개.

- [ ] **Step 1: 실패하는 테스트** — `ListFormatsTest.java` 전부(44-154행) + Review Focus 1:

```rust
case_v("formatter.list_pick", json!({"input":"[1.5, {\"a\":1}, null]","position":"nth","index":1}), json!("1.5")); // 소수는 평문 글
case_v("formatter.list_pick", json!({"input":"[1.5, {\"a\":1}, null]","position":"nth","index":2}), json!({"a":1}));
case("formatter.list_to_text", json!({"input":"[1.5, {\"a\":1}, null]"}), "1.5, {\"a\":1}, ");
case("formatter.lookup_table", json!({"input":" a ","table":"{\"a\":1}"}), "1");
case("formatter.lookup_table", json!({"input":"  ","table":{"a":"x"},"fallback":"f"}), "");   // 빈 키는 fallback 아님
fails("formatter.list_pick", json!({"input":""}), "목록이 비어 있어요", "FIELD_INVALID input");
// tests/catalog.rs — 양방향
#[test] fn formatter_catalog_and_runners_match() {
    let c = Catalog::embedded().unwrap();
    let cat: BTreeSet<_> = c.events().iter().filter(|e| e.event.starts_with("formatter.")).map(|e| e.event.clone()).collect();
    let run: BTreeSet<_> = ieum_engine::formatter::events().into_iter().map(String::from).collect();
    assert_eq!(cat, run);
    assert_eq!(run.len(), 22);
    // title도 카탈로그와 같다
}
```

- [ ] **Step 2:** → FAIL.
- [ ] **Step 3: 구현** — 목록 3개. 적합성 케이스 3개: (a) 트리거(스텁 출력) → `text_trim` → `text_case` → `branch.if`가 포맷터 출력을 참조, (b) `transform.map` 1단계 치환, (c) 필수 칸이 빈 포맷터 → 노드 Failed·뒤 노드 NotRun. 시계에 기대는 값(`오늘`·`지금`)은 쓰지 않는다. `GAPS.md` "의도된 차이" 표에 Task 3~7이 남긴 줄을 모으고 형식을 `event · 칸 · 차이`로 맞춘다.
- [ ] **Step 4:** `cargo test --workspace` → PASS, `cargo run -p ieum-cli --bin ieum-dev -- conformance conformance/` → 23케이스 통과.
- [ ] **Step 5: 커밋** `feat: 목록 포맷터 3개와 포맷터 적합성 케이스`

---

### Task 9: HTTP 러너

**Files:**
- Create: `crates/ieum-local/src/http.rs`, `crates/ieum-local/tests/http.rs`
- Modify: `crates/ieum-local/src/lib.rs`, `src-tauri/src/supervisor.rs:635-669`, `crates/ieum-cli/src/main.rs:143-170`(run 등록)·`338-346`(적합성 스텁 목록에 `http.request`)

**Interfaces:**
- Produces: `pub struct HttpRunner` · `impl HttpRunner { pub fn new() -> Self }` · `impl NodeRunner for HttpRunner`. 출력 `{statusCode: 정수, body: null|객체|문자열}`. 앱·`ieum-dev run`에 `"http.request"`로 등록.

- [ ] **Step 1: 실패하는 테스트** — `tests/http.rs`, 시험 서버는 `tokio::net::TcpListener`로 직접(레포의 기존 로컬 시험 서버 도우미가 있으면 그걸 쓴다). `BE/.../HttpNodeExecutorTest.java`의 본문(346-402)·리다이렉트(466-603)·자격 헤더 제거(636-660) 케이스를 옮긴다(SSRF 418-455·멱등성 110-200 제외). 반드시:

```rust
#[tokio::test] async fn string_body_is_sent_as_json_string()   // body "abc" → 서버가 받은 글 "\"abc\"", Content-Type application/json
#[tokio::test] async fn object_body_is_json()                   // body {"a":1} → {"a":1}
#[tokio::test] async fn empty_string_body_is_sent()             // body "" → "\"\""
#[tokio::test] async fn output_shapes()                         // 응답 "" 와 "  " → body null, {"a":1} → 객체, [1] → 문자열 "[1]", "hi" → "hi"
#[tokio::test] async fn redirect_303_becomes_get_and_drops_body()
#[tokio::test] async fn redirect_307_keeps_method_and_body()
#[tokio::test] async fn fourth_redirect_fails()                 // message "리다이렉트가 3회를 넘었습니다 — 마지막 Location: ", code HTTP_UNREACHABLE
#[tokio::test] async fn cross_origin_redirect_drops_auth_and_never_restores()  // A→B→A, A의 두 번째 요청에도 Authorization·Cookie 없음
#[tokio::test] async fn redirect_without_location_succeeds()    // 302, Location 없음 → statusCode 302
#[tokio::test] async fn status_404_fails()                      // message "HTTP 404: <본문>", code "HTTP_STATUS 404"
#[tokio::test] async fn localhost_and_private_are_allowed()     // 127.0.0.1 시험 서버에 그대로 요청
#[tokio::test] async fn bad_scheme_and_method()                 // "http·https URL만 허용됩니다: ftp://x", "지원하지 않는 HTTP 메서드: PATCH"(method "patch")
#[tokio::test] async fn webhook_credential_is_refused()         // webhookCredentialId 값이 있으면 "저장된 웹훅 주소는 데스크톱에서 쓸 수 없어요. URL 칸에 주소를 직접 넣어 주세요."
#[tokio::test] async fn body_over_1mb_is_truncated_not_failed() // 2MB 응답 → 성공, body 길이 1,048,576 이하
#[tokio::test] async fn non_utf8_body_is_lossy_string()         // 0xFF 0xFE … → 패닉 없이 문자열 (Review Focus 3)
#[tokio::test] async fn cancel_stops_request()                  // 응답 없는 서버 + cancel → 실패로 곧 끝남
#[tokio::test] async fn error_never_contains_header_values()    // Authorization: Bearer SECRET, 404 → message에 "SECRET" 없음
```

- [ ] **Step 2:** `cargo test -p ieum-local --test http` → FAIL.
- [ ] **Step 3: 구현** — `reqwest`(0.13) `redirect::Policy::none()`, `connect_timeout(10s)`, `read_timeout(30s)`(전체 마감 `.timeout()`은 쓰지 않는다). 칸은 `ctx.config`(치환된 사본)에서 읽는다. 본문은 null이 아니면 `serde_json::to_vec`. 리다이렉트 루프·출처 비교(기본 포트 80/443)·헤더 제거는 스펙 §5.4. 응답은 스트림으로 1MB까지만 읽고 `String::from_utf8_lossy`. `tokio::select!`로 `ctx.cancel`. 연결 실패·시간 초과 문구: `요청을 보내지 못했어요: <연결 실패|시간 초과|주소를 찾지 못함>`(주소·헤더 값은 넣지 않는다), code `HTTP_UNREACHABLE`. 필수 칸 `url`이 비면 `'URL' 칸이 비어 있어요`·`FIELD_EMPTY url`.
- [ ] **Step 4:** `cargo test --workspace` → PASS.
- [ ] **Step 5: 커밋** `feat: HTTP 요청 노드 러너`

---

### Task 10: 빌더 — 코드·값 변환·숨김·안내

**Files:**
- Modify: `crates/ieum-local/src/builder/{reply.rs,fields.rs,guide.rs,test.rs}`, `crates/ieum-local/src/chat/prompt.rs`
- Test: `crates/ieum-local/tests/builder_edit.rs`, `crates/ieum-local/tests/`의 test 관련 파일

**Interfaces:**
- Produces: `Code::{TooLarge, HttpStatus, HttpUnreachable, TestDeclined}`(직렬화 `TOO_LARGE`·`HTTP_STATUS`·`HTTP_UNREACHABLE`·`TEST_DECLINED`). 문구: `TooLarge` "입력이나 결과가 허용 크기를 넘음", `HttpStatus` "요청은 갔지만 서버가 오류 상태를 돌려줌. field가 상태 코드", `HttpUnreachable` "요청을 보내지 못함(연결·시간 초과·리다이렉트)", `TestDeclined` "사용자가 확인하지 않아 테스트를 돌리지 않음. 다시 시도하지 말고 사용자에게 알릴 것". exit_code는 앞 셋 1, `TestDeclined` 2.
- `set` 값 변환: 칸 type `integer` → 정수(정수로 못 읽으면 `FIELD_INVALID`), `dict` → 객체(글이 JSON 객체면 파싱, 아니면 `FIELD_INVALID`), `http.request`의 `body` → 글이 JSON 객체·배열로 읽히면 그 값, 아니면 문자열.
- `http.request`의 `webhookCredentialId`: `fields` 응답에서 빠지고 `set`은 `FIELD_UNKNOWN`.

- [ ] **Step 1: 실패하는 테스트**

```rust
#[tokio::test] async fn set_coerces_integer_dict_and_http_body() {
    // text_truncate maxLength=5 → json 5 ; lookup_table table='{"a":"b"}' → 객체 ; http body='{"a":1}' → 객체 ; http body='hi' → "hi"
}
#[tokio::test] async fn webhook_credential_field_is_hidden_and_rejected() // fields에 없음, set → FIELD_UNKNOWN
#[test] fn failure_maps_new_codes() {
    assert_eq!(failure(Some("HTTP_STATUS 404")), (Code::HttpStatus, Some("404")));
    assert_eq!(failure(Some("TOO_LARGE")), (Code::TooLarge, None));
    assert_eq!(failure(Some("FIELD_INVALID pattern")), (Code::FieldInvalid, Some("pattern")));
}
#[tokio::test] async fn test_reports_formatter_failure_code() // 포맷터 필수 칸 빈 그래프를 test → 해당 노드 line의 code FIELD_EMPTY, field 칸 key, 값·문구 원문 없음
```

- [ ] **Step 2:** `cargo test -p ieum-local` → FAIL.
- [ ] **Step 3: 구현** — 위 Interfaces. `guide.rs`·`prompt.rs`에 한 줄씩: "값을 다듬는 변환·포맷터 노드(formatter.*, transform.map)와 HTTP 요청 노드(http.request)가 있다. events로 본다." / "HTTP 노드가 낀 test는 사용자 확인 뒤에 돌거나 건너뛴다. TEST_DECLINED면 다시 시도하지 않는다." 트리거 안내("첫 노드는 local.folder_new_file")는 그대로.
- [ ] **Step 4:** `cargo test --workspace` → PASS.
- [ ] **Step 5: 커밋** `feat: 빌더가 변환·HTTP 노드의 칸과 실패 코드를 다룸`

---

### Task 11: HTTP 테스트 확인과 [켜기] 카드 (Rust)

**Files:**
- Modify: `crates/ieum-local/src/builder/{host.rs,test.rs,finish.rs}`, `crates/ieum-local/tests/builder_support/mod.rs`(MemHost), `src-tauri/src/{builder_host.rs,supervisor.rs,lib.rs,commands.rs}`
- Create: `src-tauri/src/http_confirm.rs`
- Test: `crates/ieum-local/tests/`(빌더 test·finish), `src-tauri/tests/`(게이트)

**Interfaces:**
- Produces:

```rust
// builder/host.rs
#[derive(Debug, Clone, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct HttpLine { pub node_id: String, pub label: String, pub method: String /*대문자, 없으면 GET*/, pub url: String /*config.url 원문(참조식 그대로)*/ }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpConfirm { Send, Skip, Cancelled }
// trait Host에 추가
async fn confirm_http(&self, workflow_id: &str, lines: &[HttpLine]) -> HttpConfirm;

// builder/finish.rs (test.rs와 같이 쓴다)
pub(super) fn http_lines(wf: &Workflow) -> Vec<HttpLine>;   // event == "http.request"인 노드, 배열 순서

// src-tauri/src/http_confirm.rs — pick.rs와 같은 방식(대기 5분)
pub struct HttpConfirmGate;                                  // Default, tauri State로 관리
impl HttpConfirmGate {
    pub async fn ask(&self, workflow_id: &str) -> HttpConfirm;        // 답 또는 300초 뒤 Cancelled. 같은 워크플로우의 앞선 대기는 Cancelled로 끝낸다
    pub fn answer(&self, workflow_id: &str, choice: HttpConfirm) -> bool;  // 대기 중이 아니면 false
}
// AppEvent::HttpTestConfirm { workflow_id: String, lines: Vec<HttpLine> } → 화면 이벤트 "http-test-confirm" {id, lines}
#[tauri::command] pub async fn answer_http_confirm(gate: State<'_, HttpConfirmGate>, workflow_id: String, send: bool) -> Result<bool, String>;
```

- `Builder::test` 동작: 자른 그래프에 `http.request` 노드가 있으면 `host.confirm_http`를 먼저 부른다.
  - `Send` → 저장본을 다시 읽어 `workflow_hash`가 그대로인지, `host.user_editing`이 아닌지 본 뒤 실행. 달라졌으면 `Code::Stale`/`Code::UserEditing`. 실행했으면 응답 data에 `"httpSent": true`를 싣고, 채팅 카드 `Card::Test`(`crates/ieum-local/src/chat/mod.rs`)에 `http_sent: bool`(기본 false)을 더해 그 값을 옮긴다.
  - `Skip` → 자른 그래프에서 HTTP 노드와 그 아래 노드마다 `TestEntry { hash: node_hash, status: TestStatus::Skipped }`를 기록하고 `Ok({"status":"skipped_by_user","nodes":[id…]})`. 요청은 보내지 않는다.
  - `Cancelled` → `Err(Code::TestDeclined)`.
- `finish`: `PendingFinish.actions`에 HTTP 노드 줄을 더한다 — `FinishAction { node_id, event: "http.request", label, target: Some("<METHOD> <url 원문>") }`.

- [ ] **Step 1: 실패하는 테스트** — MemHost에 `http_answer: Mutex<HttpConfirm>`와 `http_asked: Mutex<Vec<Vec<HttpLine>>>`, 그리고 확인 도중 워크플로우를 바꾸는 훅을 둔다.

```rust
#[tokio::test] async fn test_with_http_asks_first_even_for_get()      // GET 노드, 대상이 그 뒤 포맷터여도 confirm_http가 불리고 lines에 그 HTTP 노드가 있다
#[tokio::test] async fn test_without_http_never_asks()
#[tokio::test] async fn skip_marks_http_and_downstream_and_finish_is_ready()  // Skip → run_test 호출 0번, 두 노드 tests = skipped, readiness에 UNTESTED 없음
#[tokio::test] async fn cancelled_is_test_declined()                  // code TEST_DECLINED, tests 변화 없음
#[tokio::test] async fn send_after_edit_is_stale()                    // 확인 대기 중 저장본이 바뀜 → STALE, run_test 호출 0번 (Review Focus 4)
#[tokio::test] async fn send_runs_and_marks_http_sent()               // Send → run_test 1번, data.httpSent == true, Card::Test.http_sent == true
#[tokio::test] async fn finish_lists_http_action()                    // actions에 {event:"http.request", target:"POST https://x/{{nodes.a.output.id}}"}
// src-tauri: gate
#[tokio::test] async fn gate_answer_resolves_and_second_ask_cancels_first()
#[tokio::test(start_paused = true)] async fn gate_times_out_as_cancelled()   // 300초 뒤 Cancelled
```

- [ ] **Step 2:** `cargo test --workspace` → FAIL.
- [ ] **Step 3: 구현** — `AppHost::confirm_http`는 `AppEvent::HttpTestConfirm`을 내고 `HttpConfirmGate::ask`를 기다린다. `lib.rs`에 State 등록·이벤트 중계·명령 등록. 편집 화면이 앞에 없으면 기존 `finish_notice`와 같은 방식의 시스템 알림은 넣지 않는다(이번 범위 밖 — 5분 뒤 `Cancelled`).
- [ ] **Step 4:** `cargo test --workspace` → PASS.
- [ ] **Step 5: 커밋** `feat: HTTP 노드가 낀 테스트는 사람 확인 뒤에 돌거나 건너뜀`

---

### Task 12: 화면 — 정의·팔레트·아이콘·i18n

**Files:**
- Modify: `src/nodes/defs.ts`, `src/nodes/meta.tsx`, `src/editor/Palette.tsx`, `src/editor/ConfigPanel.tsx:236`(선택지 키), `src/i18n.ts`, `src/i18n.test.ts`
- Test: `src/nodes/defs.test.ts`, `src/nodes/meta.test.ts`, `src/editor/Palette.test.tsx`(새), `src/i18n.test.ts`

**Interfaces:**
- Produces: `NodeDef.node_type`에 `"TRANSFORM" | "HTTP"`, `NodeDef.outputsFrom?: string`. `DEFS`·`PALETTE`에 24개. `outputKeys(node)`가 `outputsFrom: "config.mappings"`면 `Object.keys(node.config.mappings ?? {})`. 선택지 번역 조회 `choiceLabel(event, fieldKey, id)` = `choice.<event>.<key>.<id>` 먼저, 없으면 `choice.<key>.<id>`.
- 색·아이콘(이음 클라우드 편집기와 같게): TRANSFORM `#287fa4`/`#e2f2f8` lucide `Shuffle`, HTTP `#c75146`/`#fce8e5` lucide `Globe2`.
- 팔레트 그룹 순서: 트리거 · 내 PC 작업(ACTION) · AI · 조건 · **변환**(`transform.map`) · **글**(`formatter.text_*` 12) · **숫자**(`number_*` 4) · **날짜**(`date_*` 3) · **목록**(`list_*`·`lookup_table` 3) · **HTTP**. 그룹 안은 카탈로그 event 이름순. i18n 키 `palette.group.{transform,text,number,date,list,http}`.

- [ ] **Step 1: 실패하는 테스트**

```ts
test("defs_has_30_and_transform_map_outputs_follow_mappings", () => {
  expect(Object.keys(DEFS).length).toBe(30);
  expect(outputKeys({ id: "t", event: "transform.map", type: "TRANSFORM", config: { mappings: { a: "1", b: "2" } } })).toEqual(["a", "b"]);
  expect(newNode("http.request", "h").type).toBe("HTTP");
});
test("node_meta_builtin_colors", () => {
  expect(nodeMeta("formatter.text_trim").color).toBe("#287fa4");
  expect(nodeMeta("http.request").color).toBe("#c75146");
});
test("palette_groups_formatters_by_kind", () => { /* setLang("ko"); "글" 그룹 아래 12개, "숫자" 4, "날짜" 3, "목록" 3, "변환" 1, "HTTP" 1 */ });
// i18n.test.ts — 선택지 키는 event 포함 키 또는 옛 키 둘 중 하나가 있으면 통과하게, 그리고 ko·en 둘 다
```

- [ ] **Step 2:** `npx vitest run src/nodes src/editor/Palette.test.tsx src/i18n.test.ts` → FAIL.
- [ ] **Step 3: 구현** — 24개 JSON import. i18n: 한국어 `node.<event>.title/desc`·`field.<event>.<칸>`는 카탈로그 `title`·`description` 그대로, 영어는 뜻을 옮긴다. 선택지 이름은 카탈로그 `choices[].name`. `format`처럼 노드끼리 겹치는 칸만 event 포함 키로 넣고 나머지는 기존 키 규칙을 쓴다. `http.request`의 `webhookCredentialId` 칸 번역 키도 넣는다(Web.tsx 읽기 화면에서 보인다). 기존 `meta.test.ts`(`node_meta_app_nodes_are_pink`)는 그대로 통과해야 한다.
- [ ] **Step 4:** `npm test` → PASS, `npm run build` → 타입 오류 없음.
- [ ] **Step 5: 커밋** `feat: 팔레트에 변환·포맷터·HTTP 노드`

---

### Task 13: 화면 — 설정 패널 칸

**Files:**
- Create: `src/editor/DictField.tsx`
- Modify: `src/editor/ConfigPanel.tsx`(칸 종류 갈래 246-276)
- Test: `src/editor/ConfigPanel.test.tsx`

**Interfaces:**
- Produces: `DictField({ value: Record<string, unknown>, onChange(next), refPicker? })` — 키·값 줄, [줄 추가]·줄마다 삭제. 저장값은 객체. 빈 키 줄은 저장하지 않는다. 값 입력은 기존 글 칸과 같은 참조 넣기를 쓴다.
- `integer` 칸: 숫자 입력, 저장값은 정수(`3`), 비우면 키 삭제.
- `http.request`의 `body`: 글 칸. 저장할 때 `JSON.parse`가 객체·배열이면 그 값, 아니면 문자열. 값이 객체·배열이면 `JSON.stringify(v, null, 2)`로 보여 준다.
- `http.request`의 `webhookCredentialId`: 그리지 않는다.

- [ ] **Step 1: 실패하는 테스트**

```tsx
test("dict_field_saves_object", ...)          // transform.map: 줄 추가 → 키 "a", 값 "1" → onChange config.mappings = {a:"1"} ; 줄 삭제 → {}
test("integer_field_saves_number", ...)       // text_truncate maxLength "5" → 5 (typeof number)
test("http_body_json_is_saved_as_object", ...)// '{"a":1}' → {a:1} ; 'hi' → "hi" ; 객체 값이 들어오면 textarea에 들여쓴 JSON
test("http_webhook_field_is_hidden", ...)     // queryByTestId("field-webhookCredentialId") === null
test("lookup_table_and_headers_use_dict_field", ...)
```

- [ ] **Step 2:** `npx vitest run src/editor/ConfigPanel.test.tsx` → FAIL.
- [ ] **Step 3: 구현** — 칸 종류 갈래에 `dict`·`integer` 추가, 위 두 특례. 기존 13개 테스트는 그대로 통과.
- [ ] **Step 4:** `npm test`, `npm run build` → PASS.
- [ ] **Step 5: 커밋** `feat: 설정 패널에 키-값 칸과 숫자 칸`

---

### Task 14: 화면 — HTTP 확인 카드

**Files:**
- Create: `src/editor/chat/HttpTestCard.tsx`, `src/editor/chat/HttpTestCard.test.tsx`
- Modify: `src/api.ts`, `src/screens/Editor.tsx`, `src/editor/chat/FinishCard.tsx`, `src/editor/useTestRun.ts`와 호출처 두 곳(편집기 툴바, `src/…/RunView.tsx:15`), `src/i18n.ts`
- Test: 위 + `src/editor/chat/FinishCard.test.tsx`

**Interfaces:**
- Consumes: Task 11의 이벤트 `"http-test-confirm"` `{id, lines: {nodeId, label, method, url}[]}`와 명령 `answer_http_confirm(workflowId, send)`.
- Produces: `api.onHttpTestConfirm(cb)`, `api.answerHttpConfirm(workflowId, send: boolean)`. `showRefs(url: string): string` — `{{nodes.….output.…}}`를 `〈앞 노드 값〉`(en `‹earlier value›`)로 바꾼다.
- `HttpTestCard`(role="dialog", 제목 "테스트로 실제 요청을 보낼까요?"): 줄마다 `<METHOD> <showRefs(url)>`와 노드 이름, 안내 "요청은 되돌릴 수 없어요.", 버튼 [보내기]·[테스트 없이 넘어가기]. 5분 안에 답이 없으면 닫힌다(백엔드가 취소).
- 채팅의 테스트 카드(`src/editor/chat/ChatCards.tsx`, `ChatCard`의 `kind:"test"`에 `httpSent?: boolean`): `httpSent`면 "실제로 요청을 보냈어요"(en "A real request was sent") 한 줄을 보인다.
- `FinishCard`: 목록 제목을 "이 워크플로우가 하는 일"(en "What this workflow does")로 바꾸고 `finish.action.http.request` = "요청 보내기 → {target}".
- `useTestRun`: 워크플로우에 GET이 아닌 `http.request` 노드가 있으면 실행 전에 같은 모양의 확인(버튼 [보내기]·[취소])을 띄운다. GET만 있으면 바로 실행.

- [ ] **Step 1: 실패하는 테스트**

```tsx
test("http_card_shows_method_and_masked_url_and_answers", async () => {
  // lines [{nodeId:"h", label:"주문 넣기", method:"POST", url:"https://x.test/o/{{nodes.a.output.id}}"}]
  // "POST https://x.test/o/〈앞 노드 값〉" 보임 ; [보내기] → answerHttpConfirm("w1", true) ; [테스트 없이 넘어가기] → ("w1", false)
});
test("test_card_notes_real_request", ...)     // {kind:"test", httpSent:true} → "실제로 요청을 보냈어요"
test("finish_card_shows_http_action", ...)      // "요청 보내기 → POST https://x.test/o/{{nodes.a.output.id}}"
test("test_run_asks_before_non_get_http", ...)  // POST 노드 → 확인 뜸, [취소]면 runNow 호출 0번 ; GET만 → 바로 runNow
```

- [ ] **Step 2:** `npx vitest run src/editor` → FAIL.
- [ ] **Step 3: 구현** — 카드 틀은 `FinishCard`의 dialog 마크업·색을 그대로 따른다. `Editor.tsx`에서 이벤트를 구독해 현재 워크플로우 id와 같을 때만 띄운다.
- [ ] **Step 4:** `npm test`, `npm run build` → PASS.
- [ ] **Step 5: 커밋** `feat: HTTP 테스트 확인 카드와 [켜기] 카드의 요청 줄`

---

### Task 15: 끝-끝 확인과 마무리

**Files:** Modify `conformance/GAPS.md`(최종 점검), `README.md`(노드 수 언급이 있으면)

- [ ] **Step 1:** `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace && npm test && npm run build` → 전부 통과.
- [ ] **Step 2:** `ieum-dev run`으로 폴더 트리거 → `formatter.text_trim` → `transform.map` → `http.request`(로컬 시험 서버 `python3 -m http.server` 등) 워크플로우 JSON을 실행해 노드 4개 Succeeded와 서버 로그의 요청을 확인.
- [ ] **Step 3:** 개발 빌드로 앱을 띄워(`npm run tauri dev`, 설치된 IEUM은 먼저 종료) 확인: 팔레트 그룹 6개, 포맷터 설정(선택지·숫자 칸·키-값 칸), HTTP 본문 JSON 저장, 채팅 패널에서 HTTP 노드 테스트 → 확인 카드 → [테스트 없이 넘어가기] → [켜기] 카드에 요청 줄, 폴더에 파일을 넣어 실제 실행. 이음 클라우드 탭의 읽기 전용 캔버스에서 변환·HTTP 노드 색·아이콘. **사용자가 화면 앞에서 작업 중이면 창을 끌어오지 말고 사용자에게 넘긴다.**
- [ ] **Step 4:** `GAPS.md` "의도된 차이" 표가 event·칸 단위로 채워졌는지, 스펙 §5.3 밖의 차이가 없는지 확인.
- [ ] **Step 5: 커밋** `docs: 변환·포맷터·HTTP 노드의 엔진 차이 기록` → PR(base `dev`, 제목 `[feat] 변환·포맷터·HTTP 노드를 데스크톱에서 실행 (#5)`)은 사용자 요청 시.
