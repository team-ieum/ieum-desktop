// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api } from "../../api";
import { setLang } from "../../i18n";
import { AssistantPanel, availableMode, MODE_KEY } from "./AssistantPanel";

// 이 환경의 localStorage는 온전하지 않다 — 메모리 저장소를 꽂는다(Sidebar.test.tsx와 같은 방식).
const memoryStorage = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};
beforeAll(() => setLang("ko"));
beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
  vi.spyOn(api, "termTools").mockResolvedValue({ tools: ["claude"], clis: [{ tool: "claude", state: "ready" }], running: null });
  vi.spyOn(api, "listModels").mockResolvedValue([]);
  // 채팅 자리가 실제 invoke를 타지 않게.
  vi.spyOn(api, "getChat").mockResolvedValue({ messages: [], state: { outline: [], baseline: null, tests: {}, pendingFinish: null }, usage: [] });
  vi.spyOn(api, "onChatStage").mockResolvedValue(() => {});
  vi.spyOn(api, "listCredentials").mockResolvedValue({
    apiKeys: { anthropic: [], openai: [], gemini: [] },
    claudeAccounts: [],
    codexAccounts: [],
    keychainError: null,
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
const open = () => act(async () => fireEvent.click(screen.getByRole("button", { name: "IEUM Assistant 열기" })));
// 숨긴 요소는 접근 이름이 빈 문자열로 계산돼 role + name으로는 못 찾는다 — 속성으로 찾는다.
const panel = () => document.querySelector<HTMLElement>('aside[aria-label="IEUM Assistant"]');

test("assistant_opens_and_stays_mounted_when_closed", async () => {
  const r = render(<AssistantPanel workflowId="w1" />);
  expect(panel()).toBeNull(); // 열기 전엔 없음
  await open();
  expect(screen.getByRole("complementary", { name: "IEUM Assistant" })).toBe(panel());
  expect(panel()!.hidden).toBe(false);
  expect(r.container.querySelector('[data-slot="terminal"]')).toBeTruthy();
  expect(r.container.querySelector('[data-slot="chat"]')).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "IEUM Assistant 닫기" })));
  expect(panel()!.hidden).toBe(true); // 숨기기만
  expect(r.container.querySelector('[data-slot="terminal"]')).toBeTruthy();
});

test("assistant_remembers_last_mode", async () => {
  render(<AssistantPanel workflowId="w1" />);
  await open();
  expect(screen.getByRole("tab", { name: "채팅" }).getAttribute("aria-selected")).toBe("true"); // 기본
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "터미널" })));
  expect(localStorage.getItem(MODE_KEY)).toBe("terminal");
  expect(panel()!.style.width).toBe("560px");
  cleanup();
  render(<AssistantPanel workflowId="w1" />);
  await open();
  expect(screen.getByRole("tab", { name: "터미널" }).getAttribute("aria-selected")).toBe("true");
});

test("assistant_uses_web_chat_look", async () => {
  render(<AssistantPanel workflowId="w1" />);
  const button = screen.getByRole("button", { name: "IEUM Assistant 열기" });
  expect(button.className).toContain("bg-main-deep-blue");
  expect(button.style.boxShadow).toContain("rgba(41, 83, 124, 0.45)");
  await open();
  expect(panel()!.className).toContain("rounded-2xl");
  expect(panel()!.style.boxShadow).toContain("rgba(16, 24, 40, 0.18)");
});

test("assistant_has_no_slots_before_first_save", async () => {
  const r = render(<AssistantPanel workflowId="" />);
  await open();
  expect(r.container.querySelector("[data-slot]")).toBeNull();
});

test("available_mode_falls_back_to_what_works", () => {
  expect(availableMode("terminal", { terminal: true, chat: true })).toBe("terminal");
  expect(availableMode("terminal", { terminal: false, chat: true })).toBe("chat");
  expect(availableMode(null, { terminal: true, chat: false })).toBe("terminal");
  expect(availableMode("zzz", { terminal: true, chat: true })).toBe("chat");
  expect(availableMode("chat", { terminal: false, chat: false })).toBeNull();
});

test("assistant_hides_terminal_tab_without_usable_cli", async () => {
  vi.mocked(api.termTools).mockResolvedValue({ tools: [], clis: [], running: null });
  localStorage.setItem(MODE_KEY, "terminal");
  render(<AssistantPanel workflowId="w1" />);
  await open();
  await waitFor(() => expect(screen.queryByRole("tab", { name: "터미널" })).toBeNull());
  expect(screen.getByRole("tab", { name: "채팅" }).getAttribute("aria-selected")).toBe("true");
  expect(localStorage.getItem(MODE_KEY)).toBe("terminal"); // 기억은 사용자가 고를 때만 바뀐다
});

test("assistant_keeps_terminal_tab_for_a_running_session", async () => {
  vi.mocked(api.termTools).mockResolvedValue({ tools: [], clis: [], running: "claude" });
  render(<AssistantPanel workflowId="w1" />);
  await open();
  expect(screen.getByRole("tab", { name: "터미널" })).toBeTruthy();
});

test("assistant_reports_how_much_canvas_it_covers", async () => {
  const cover = vi.fn();
  render(<AssistantPanel workflowId="w1" onCoverChange={cover} />);
  expect(cover).toHaveBeenLastCalledWith(0);
  await open();
  expect(cover).toHaveBeenLastCalledWith(380 + 16); // 채팅 폭 + 오른쪽 여백
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "터미널" })));
  expect(cover).toHaveBeenLastCalledWith(560 + 16);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "IEUM Assistant 닫기" })));
  expect(cover).toHaveBeenLastCalledWith(0);
});
