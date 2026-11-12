// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type PlanUsage, type UsageSummary } from "../api";
import { setLang } from "../i18n";
import { StatusBar } from "./StatusBar";

const SUMMARY: UsageSummary = {
  totalUsd: 0.42,
  totalInput: 1000,
  totalOutput: 500,
  unpricedCalls: 0,
  bySource: [],
  byCredential: [],
  byWorkflow: [],
  byDay: [],
  asOf: "2026-11-01",
};
const inHours = (h: number) => new Date(Date.now() + h * 3600_000 + 30_000).toISOString();

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const mount = async (onUsage = () => {}, onSettings = () => {}) => {
  await act(async () => {
    render(<StatusBar onUsage={onUsage} onSettings={onSettings} />);
  });
};

const OK: PlanUsage = {
  state: "ok",
  windows: [{ kind: "five_hour", usedPct: 34, resetsAt: inHours(58) }],
  checkedAt: new Date().toISOString(),
  accountEmail: null,
};

/** 구독 종류마다 다른 답. */
const subs = (claude: () => Promise<PlanUsage>, codex: () => Promise<PlanUsage>) =>
  vi.spyOn(api, "subscriptionUsage").mockImplementation((k) => (k === "claude" ? claude() : codex()));

test("status_bar_opens_usage_popover_not_settings", async () => {
  const sub = subs(
    async () => OK,
    async () => ({ state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null }),
  );
  const codexCalls = () => sub.mock.calls.filter((c) => c[0] === "codex").length;
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockResolvedValue({ ...SUMMARY, bySource: [{ source: "openai", input: 10, output: 5, usd: 1.5 }] });
  const onSettings = vi.fn();
  await mount(() => {}, onSettings);

  await act(async () => fireEvent.click(screen.getByText("Claude 66% · 2일 10시간")));
  expect(onSettings).not.toHaveBeenCalled();
  const pop = within(screen.getByRole("dialog", { name: "AI 사용량" }));
  expect(pop.getByText("66% 남음 · 2일 10시간 뒤 초기화")).toBeTruthy();
  expect(pop.getByText("로그인 필요")).toBeTruthy();
  expect(pop.getByText("확인 불가")).toBeTruthy();
  expect(pop.getByText("이번 달 예상 $1.50")).toBeTruthy();
  expect(document.activeElement).toBe(pop.getByRole("button", { name: "새로 고침" }));

  const calls = codexCalls();
  await act(async () => fireEvent.click(pop.getByRole("button", { name: "새로 고침" })));
  expect(codexCalls()).toBe(calls + 1);

  // Esc·바깥 클릭으로 닫힌다.
  await act(async () => fireEvent.keyDown(document, { key: "Escape" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  await act(async () => fireEvent.click(screen.getByText("오늘 API 예상 $0.42")));
  expect(screen.getByRole("dialog")).toBeTruthy();
  await act(async () => fireEvent.mouseDown(document.body));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("popover_links_open_usage_and_ai_settings", async () => {
  subs(
    async () => OK,
    () => Promise.reject("no command"),
  );
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  const onUsage = vi.fn();
  const onSettings = vi.fn();
  await mount(onUsage, onSettings);

  await act(async () => fireEvent.click(screen.getByText("오늘 API 예상 $0.42")));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "사용량 자세히" })));
  expect(onUsage).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();

  await act(async () => fireEvent.click(screen.getByText("오늘 API 예상 $0.42")));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "AI 설정 열기" })));
  expect(onSettings).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("status_bar_shows_plan_and_today_cost", async () => {
  const plan: PlanUsage = {
    state: "ok",
    windows: [
      { kind: "five_hour", usedPct: 34, resetsAt: inHours(58) },
      { kind: "seven_day", usedPct: 10, resetsAt: inHours(100) },
    ],
    checkedAt: new Date().toISOString(),
    accountEmail: null,
  };
  vi.spyOn(api, "subscriptionUsage").mockResolvedValue(plan);
  const summary = vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await mount();
  expect(summary).toHaveBeenCalledWith("today");
  expect(screen.getByText("Claude 66% · 2일 10시간")).toBeTruthy();
  expect(screen.getByText("오늘 API 예상 $0.42")).toBeTruthy();
});

test("status_bar_hides_claude_when_not_logged_in", async () => {
  vi.spyOn(api, "subscriptionUsage").mockResolvedValue({ state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null });
  vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await mount();
  expect(screen.queryByText(/Claude/)).toBeNull();
  expect(screen.getByText("오늘 API 예상 $0.42")).toBeTruthy();
});

test("status_bar_unavailable_when_commands_fail", async () => {
  vi.spyOn(api, "subscriptionUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
  await mount();
  expect(screen.getByText("Claude 확인 불가")).toBeTruthy();
  expect(screen.getByText("오늘 API 예상 —")).toBeTruthy();
});
