// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type CliAccountMeta, type CredentialsView, type PlanUsage, type UsageSummary } from "../api";
import { setLang } from "../i18n";
import { useCredentials } from "../stores/credentials";
import { usePlanStore } from "../stores/plans";
import { StatusBar } from "./StatusBar";

const summary = (totalUsd: number): UsageSummary => ({
  totalUsd,
  totalInput: 1000,
  totalOutput: 500,
  unpricedCalls: 0,
  bySource: [],
  byCredential: [],
  byWorkflow: [],
  byDay: [],
  asOf: "2026-11-01",
});
const inHours = (h: number, m = 0) => new Date(Date.now() + (h * 60 + m) * 60_000 + 30_000).toISOString();
const SYS: CliAccountMeta = { id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: "me@claude.dev" };
const WORK: CliAccountMeta = { id: "abcd1234", label: "회사", isDefault: false, createdAt: "", email: "work@claude.dev" };
const VIEW: CredentialsView = { apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [SYS, WORK], codexAccounts: [], keychainError: null };

const OK: PlanUsage = {
  state: "ok",
  windows: [
    { kind: "five_hour", usedPct: 34, resetsAt: inHours(13, 21) },
    { kind: "seven_day", usedPct: 12, resetsAt: inHours(100) },
  ],
  checkedAt: new Date(Date.now() - 3 * 60_000 - 5_000).toISOString(),
  accountEmail: "me@claude.dev",
};
const WORK_PLAN: PlanUsage = { ...OK, windows: [{ kind: "five_hour", usedPct: 91, resetsAt: inHours(2) }], accountEmail: "work@claude.dev" };
const OUT: PlanUsage = { state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null };

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useCredentials.setState({ view: null });
  usePlanStore.setState({ plans: {} });
});

/** Claude 계정 둘(기본 = 이 Mac), Codex 없음, Antigravity 로그인 전. */
const mockAll = () => {
  vi.spyOn(api, "listCredentials").mockResolvedValue(VIEW);
  const sub = vi.spyOn(api, "subscriptionUsage").mockImplementation(async (k, id) => (k === "claude" ? (id === "system" ? OK : WORK_PLAN) : OUT));
  vi.spyOn(api, "antigravityPlanUsage").mockResolvedValue(OUT);
  vi.spyOn(api, "usageSummary").mockImplementation(async (r) => summary(r === "month" ? 1.5 : 0.42));
  return sub;
};

const mount = async (onUsage = () => {}, onSettings = () => {}) => {
  await act(async () => {
    render(<StatusBar onUsage={onUsage} onSettings={onSettings} />);
  });
};
const openPopover = async () => {
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /^사용/ })));
  return within(screen.getByRole("dialog", { name: "사용량" }));
};

test("status_bar_shows_subscription_percent_and_reset", async () => {
  const sub = mockAll();
  await mount();
  // 가장 빡빡한 창(34%) 기준, 연결된 구독만.
  const chip = screen.getByTitle("Claude 34% 사용 · 13시간 21분 뒤 초기화");
  expect(within(chip).getByText("34%")).toBeTruthy();
  expect(within(chip).getByText("13시간 21분")).toBeTruthy();
  expect(screen.queryByTitle(/Codex|Antigravity/)).toBeNull();
  expect(screen.queryByText(/오늘 API/)).toBeNull();
  // 새로 고침 아이콘
  const calls = sub.mock.calls.length;
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "새로 고침" })));
  expect(sub.mock.calls.length).toBeGreaterThan(calls);
});

test("status_bar_without_subscription_shows_today_api", async () => {
  vi.spyOn(api, "listCredentials").mockResolvedValue({ ...VIEW, claudeAccounts: [] });
  vi.spyOn(api, "subscriptionUsage").mockResolvedValue(OUT);
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockResolvedValue(summary(0.42));
  await mount();
  expect(screen.getByText("오늘 API 예상 $0.42")).toBeTruthy();
  expect(screen.queryByTitle(/Claude/)).toBeNull();
});

test("usage_popover_flyout_shows_accounts_and_windows", async () => {
  mockAll();
  await mount();
  const pop = await openPopover();
  // 연결된 구독 줄 + API 줄
  const claudeRow = pop.getByRole("button", { name: /^Claude/ });
  expect(within(claudeRow).getByText("34%")).toBeTruthy();
  expect(within(claudeRow).getByText("13시간 21분")).toBeTruthy();
  expect(pop.queryByRole("button", { name: /^Codex/ })).toBeNull();
  expect(pop.getByText("API 이번 달 예상 $1.50")).toBeTruthy();

  await act(async () => fireEvent.mouseEnter(claudeRow));
  const fly = within(screen.getByRole("region", { name: "Claude" }));
  expect(fly.getByText("3분 전 업데이트됨")).toBeTruthy();
  expect(fly.getByText("세션")).toBeTruthy();
  expect(fly.getByText("34% 사용")).toBeTruthy();
  expect(fly.getByText("13시간 21분 뒤 초기화")).toBeTruthy();
  expect(fly.getByText("주간")).toBeTruthy();
  expect(fly.getByText("12% 사용")).toBeTruthy();
  // 계정 목록 — 체크 = 기본, 각 계정 사용 %(80% 넘으면 빨강). 보기 전용.
  const list = within(fly.getByRole("list", { name: "Claude 계정" }));
  const me = within(list.getByRole("listitem", { name: /me@claude\.dev/ }));
  expect(me.getByLabelText("기본")).toBeTruthy();
  expect(me.getByText("34%")).toBeTruthy();
  const work = within(list.getByRole("listitem", { name: /work@claude\.dev/ }));
  expect(work.queryByLabelText("기본")).toBeNull();
  expect(work.getByText("91%").className).toMatch(/danger/);
  expect(list.queryByRole("button")).toBeNull();
  expect(fly.getByRole("button", { name: "계정 관리…" })).toBeTruthy();
});

test("usage_popover_keyboard_moves_rows_and_opens_flyout", async () => {
  mockAll();
  await mount();
  const pop = await openPopover();
  const claudeRow = pop.getByRole("button", { name: /^Claude/ });
  await act(async () => fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" }));
  expect(document.activeElement).toBe(claudeRow);
  await act(async () => fireEvent.keyDown(claudeRow, { key: "ArrowRight" }));
  expect(screen.getByRole("region", { name: "Claude" })).toBeTruthy();
  await act(async () => fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" }));
  expect(screen.queryByRole("region", { name: "Claude" })).toBeNull();
  expect(document.activeElement).toBe(claudeRow);
  // Esc·바깥 클릭으로 닫힌다.
  await act(async () => fireEvent.keyDown(document, { key: "Escape" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  await openPopover();
  await act(async () => fireEvent.mouseDown(document.body));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("popover_links_to_usage_and_accounts", async () => {
  mockAll();
  const onUsage = vi.fn();
  const onSettings = vi.fn();
  await mount(onUsage, onSettings);

  let pop = await openPopover();
  await act(async () => fireEvent.click(pop.getByRole("button", { name: /사용량 세부 정보 및 기록/ })));
  expect(onUsage).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();

  pop = await openPopover();
  await act(async () => fireEvent.click(pop.getByRole("button", { name: /^계정 관리…/ })));
  expect(onSettings).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();

  // 두 번째 패널의 계정 관리도 같은 곳으로.
  pop = await openPopover();
  await act(async () => fireEvent.click(pop.getByRole("button", { name: /^Claude/ })));
  await act(async () => fireEvent.click(within(screen.getByRole("region", { name: "Claude" })).getByRole("button", { name: "계정 관리…" })));
  expect(onSettings).toHaveBeenCalledTimes(2);
});
