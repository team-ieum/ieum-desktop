// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import App from "./App";
import { api } from "./api";

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  // @ts-expect-error jsdom에 없음
  globalThis.DOMMatrixReadOnly = class {
    m22 = 1;
  };
  vi.spyOn(api, "getSettings").mockResolvedValue({ language: "ko", autostart: false, firstRunDone: true, serverUrl: "" });
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "validateWorkflow").mockResolvedValue([]);
  vi.spyOn(api, "webAccount").mockResolvedValue(null);
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [], apiKeys: [] });
  vi.spyOn(api, "keychainStatus").mockResolvedValue({ anthropic: "missing", openai: "missing", gemini: "missing", "ieum-session": "missing" });
});
afterEach(cleanup);

test("leaving_editor_with_unsaved_changes_asks_in_app", async () => {
  await act(async () => {
    render(<App />);
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "새 워크플로우" }));
  });
  const history = screen.getByRole("button", { name: "기록" });

  // 고치기 전에는 묻지 않고 바로 간다 — 다시 편집기로.
  await act(async () => fireEvent.click(history));
  expect(screen.queryByRole("alert")).toBeNull();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "워크플로우" })));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "새 워크플로우" })));

  await act(async () => fireEvent.change(screen.getByLabelText("워크플로우 이름"), { target: { value: "고침" } }));

  // 탭 이동: 취소하면 편집기와 고친 내용이 남는다.
  await act(async () => fireEvent.click(history));
  expect(screen.getByRole("alert").textContent).toContain("저장하지 않은 변경");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "취소" })));
  expect(screen.queryByRole("alert")).toBeNull();
  expect((screen.getByLabelText("워크플로우 이름") as HTMLInputElement).value).toBe("고침");

  // 뒤로: 버리기를 눌러야 목록으로 간다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "뒤로" })));
  expect(screen.getByLabelText("워크플로우 이름")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "버리기" })));
  expect(screen.queryByLabelText("워크플로우 이름")).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

const ACCOUNT = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };

test("web_tab_hidden_when_logged_out", async () => {
  vi.spyOn(api, "webAccount").mockResolvedValueOnce(null);
  await act(async () => {
    render(<App />);
  });
  expect(screen.queryByRole("button", { name: "웹 이음" })).toBeNull();
  expect(screen.getByRole("button", { name: "설정" })).toBeTruthy();
  cleanup();

  vi.spyOn(api, "webAccount").mockResolvedValueOnce(ACCOUNT);
  await act(async () => {
    render(<App />);
  });
  expect(screen.getByRole("button", { name: "웹 이음" })).toBeTruthy();
});

test("auth_expired_goes_to_login", async () => {
  vi.spyOn(api, "webAccount").mockResolvedValueOnce(ACCOUNT);
  vi.spyOn(api, "webListWorkflows").mockRejectedValueOnce({ code: "AUTH_EXPIRED" });
  await act(async () => {
    render(<App />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "웹 이음" })));

  expect(screen.queryByRole("button", { name: "웹 이음" })).toBeNull();
  expect(screen.getByRole("button", { name: "설정" }).getAttribute("aria-current")).toBe("page");
  expect(screen.getByLabelText("비밀번호")).toBeTruthy();
  expect(screen.getByText(/로그인이 만료됐어요/)).toBeTruthy();
});
