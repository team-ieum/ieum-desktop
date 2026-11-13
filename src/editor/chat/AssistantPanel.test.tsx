// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api } from "../../api";
import { setLang } from "../../i18n";
import { AssistantPanel, MODE_KEY } from "./AssistantPanel";

// 이 환경의 localStorage는 온전하지 않다 — 메모리 저장소를 꽂는다(Sidebar.test.tsx와 같은 방식).
const memoryStorage = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};
beforeAll(() => setLang("ko"));
beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
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
  expect(panel()!.className).toContain("w-[560px]");
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
