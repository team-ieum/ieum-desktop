---
paths:
  - "src/**/*.test.ts"
  - "src/**/*.test.tsx"
---
- 테스트 파일은 컴포넌트 옆 `*.test.tsx`
- Tauri 호출은 `vi.spyOn(api, "명령")`으로 `src/api.ts`를 대체한다 — `invoke`를 직접 모킹하지 않는다
- vitest는 `TZ=Asia/Seoul`로 돈다(`vite.config.ts`) — 날짜 경계 테스트가 여기에 의존한다
