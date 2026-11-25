---
paths:
  - "src/**/*.ts"
  - "src/**/*.tsx"
---
- 화면 문자열은 하드코딩하지 않는다 — `src/i18n.ts`의 `ko`·`en` 양쪽에 키를 넣고 `t("키")`로 부른다
- Tauri 호출은 `src/api.ts`를 거친다 — 컴포넌트에서 `invoke`를 직접 부르지 않는다
- 여러 화면이 나눠 쓰는 상태는 `src/stores/`의 zustand 스토어에 둔다
