// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api, type CliAccountMeta, type CredentialsView, type PlanUsage, type UsageSummary } from "../api";
import { setLang } from "../i18n";
import { StatusBar } from "../shell/StatusBar";
import { useCredentials } from "../stores/credentials";
import { usePlanStore } from "../stores/plans";
import { UsageCard } from "./UsageCard";

const SUMMARY: UsageSummary = {
  totalUsd: 1.85,
  totalInput: 100_000,
  totalOutput: 23_000,
  unpricedCalls: 0,
  bySource: [{ source: "anthropic", input: 100_000, output: 23_000, usd: 1.85 }],
  byCredential: [{ source: "anthropic", credential: "default", input: 100_000, output: 23_000, usd: 1.85 }],
  byWorkflow: [{ workflowId: "w1", input: 100_000, output: 23_000, usd: 1.85 }],
  byDay: [],
  asOf: "2026-11-01",
};

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useCredentials.setState({ view: null });
  usePlanStore.setState({ plans: {} });
});
// 이 환경의 localStorage는 온전하지 않다 — 테스트마다 빈 메모리 저장소를 꽂는다.
beforeEach(() => {
  const m = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) });
});

test("usage_card_hidden_on_web_tab", async () => {
  const summary = vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await act(async () => {
    render(<UsageCard hidden onUsage={() => {}} onAccounts={() => {}} />);
  });
  expect(screen.queryByText("AI 사용량")).toBeNull();
  expect(summary).not.toHaveBeenCalled();
});

test("usage_card_switches_range", async () => {
  const summary = vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await act(async () => {
    render(<UsageCard hidden={false} onUsage={() => {}} onAccounts={() => {}} />);
  });
  expect(summary).toHaveBeenLastCalledWith("today");
  expect(screen.getByText("$1.85")).toBeTruthy();
  expect(screen.getByText(/2026-11-01/)).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "이번 주" })));
  expect(summary).toHaveBeenLastCalledWith("week");
});

const inHours = (h: number, m = 0) => new Date(Date.now() + (h * 60 + m) * 60_000 + 30_000).toISOString();
const SYS: CliAccountMeta = { id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: "me@claude.dev" };
const VIEW: CredentialsView = { apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [SYS], codexAccounts: [], keychainError: null };
const OK: PlanUsage = {
  state: "ok",
  windows: [
    { kind: "five_hour", usedPct: 17, resetsAt: inHours(13, 20) },
    { kind: "seven_day", usedPct: 91, resetsAt: inHours(100) },
  ],
  checkedAt: new Date().toISOString(),
  accountEmail: "me@claude.dev",
};
const OUT: PlanUsage = { state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null };

/** 기본: Claude 기본 계정만 연결, Codex 없음, Antigravity 로그인 전. `codex`를 주면 Codex 기본 계정도 연결. */
const mockSubs = (claude: PlanUsage = OK, codex?: PlanUsage) => {
  const view = codex ? { ...VIEW, codexAccounts: [{ ...SYS, email: "me@openai.dev" }] } : VIEW;
  useCredentials.setState({ view });
  vi.spyOn(api, "listCredentials").mockResolvedValue(view);
  vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  const sub = vi.spyOn(api, "subscriptionUsage").mockImplementation(async (k) => (k === "claude" ? claude : (codex ?? OUT)));
  const agy = vi.spyOn(api, "antigravityPlanUsage").mockResolvedValue(OUT);
  return { sub, agy };
};
const mountCard = async (onUsage = () => {}, onAccounts = () => {}) => {
  await act(async () => {
    render(<UsageCard hidden={false} onUsage={onUsage} onAccounts={onAccounts} />);
  });
};

test("dashboard_usage_shows_subscription_windows", async () => {
  mockSubs();
  const onUsage = vi.fn();
  await mountCard(onUsage);
  expect(screen.getByText("API 사용량")).toBeTruthy();
  // API 칸과 구독 칸은 같은 행에 나란히
  const row = screen.getByRole("region", { name: "API 사용량" }).parentElement!;
  expect(row.dataset.layout).toBe("split");
  expect(screen.getByRole("region", { name: "구독 사용량" }).parentElement).toBe(row);
  const subs = within(screen.getByRole("region", { name: "구독 사용량" }));
  const claude = within(subs.getByRole("group", { name: "Claude" }));
  expect(claude.getByText("me@claude.dev")).toBeTruthy();
  expect(claude.getByText("17% 사용 · 13시간 20분 뒤 초기화")).toBeTruthy();
  // 80% 넘으면 빨강
  expect(claude.getByText(/^91% 사용/).className).toContain("text-danger-700");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "자세히" })));
  expect(onUsage).toHaveBeenCalledTimes(1);
});

test("dashboard_usage_hides_unconnected_subscriptions", async () => {
  mockSubs();
  await mountCard();
  const subs = within(screen.getByRole("region", { name: "구독 사용량" }));
  expect(subs.getByRole("group", { name: "Claude" })).toBeTruthy();
  expect(subs.queryByRole("group", { name: "Codex" })).toBeNull();
  expect(subs.queryByRole("group", { name: "Antigravity" })).toBeNull();
  expect(subs.queryByText("연결된 구독이 없어요 —")).toBeNull();
});

test("dashboard_usage_no_subscription_links_to_accounts", async () => {
  mockSubs(OUT);
  const onAccounts = vi.fn();
  await mountCard(undefined, onAccounts);
  const subs = within(screen.getByRole("region", { name: "구독 사용량" }));
  expect(subs.getByText("연결된 구독이 없어요 —")).toBeTruthy();
  await act(async () => fireEvent.click(subs.getByRole("button", { name: "AI 제공자 계정에서 연결" })));
  expect(onAccounts).toHaveBeenCalledTimes(1);
});

test("subscription_usage_fetched_once_for_bar_and_dashboard", async () => {
  const { sub, agy } = mockSubs();
  await act(async () => {
    render(
      <>
        <StatusBar onUsage={() => {}} onSettings={() => {}} />
        <UsageCard hidden={false} onUsage={() => {}} onAccounts={() => {}} />
      </>,
    );
  });
  expect(within(screen.getByRole("region", { name: "구독 사용량" })).getByText("17% 사용 · 13시간 20분 뒤 초기화")).toBeTruthy();
  expect(sub).toHaveBeenCalledTimes(1);
  expect(agy).toHaveBeenCalledTimes(1);
});

const plan = (pct: number): PlanUsage => ({ ...OK, windows: [{ kind: "session", usedPct: pct, resetsAt: inHours(4) }], accountEmail: "me@openai.dev" });
const subTabs = () => within(screen.getByRole("tablist", { name: "구독 사용량" }));

test("dashboard_subscription_tabs_show_one_at_a_time", async () => {
  mockSubs(OK, plan(40));
  await mountCard();
  const subs = within(screen.getByRole("region", { name: "구독 사용량" }));
  // Claude(91%)가 기본, 카드는 하나만
  expect(subs.getByRole("group", { name: "Claude" })).toBeTruthy();
  expect(subs.queryByRole("group", { name: "Codex" })).toBeNull();
  await act(async () => fireEvent.click(subTabs().getByRole("tab", { name: /Codex/ })));
  expect(subs.getByRole("group", { name: "Codex" })).toBeTruthy();
  expect(subs.queryByRole("group", { name: "Claude" })).toBeNull();
  // 고른 탭은 다시 열어도 그대로
  cleanup();
  await mountCard();
  expect(subTabs().getByRole("tab", { name: /Codex/ }).getAttribute("aria-selected")).toBe("true");
});

test("default_tab_is_tightest_window", async () => {
  mockSubs(OK, plan(95));
  await mountCard();
  const codex = subTabs().getByRole("tab", { name: /Codex/ });
  expect(codex.getAttribute("aria-selected")).toBe("true");
  expect(subTabs().getByRole("tab", { name: /Claude/ }).getAttribute("aria-selected")).toBe("false");
  // 탭에도 최대 %, 80% 넘으면 빨강
  expect(within(codex).getByText("95%").className).toContain("text-danger-700");
  expect(within(screen.getByRole("region", { name: "구독 사용량" })).getByRole("group", { name: "Codex" })).toBeTruthy();
});

test("single_subscription_has_no_tabs", async () => {
  mockSubs();
  await mountCard();
  expect(screen.queryByRole("tablist", { name: "구독 사용량" })).toBeNull();
  expect(within(screen.getByRole("region", { name: "구독 사용량" })).getByRole("group", { name: "Claude" })).toBeTruthy();
});

test("usage_card_empty_when_unavailable", async () => {
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
  await act(async () => {
    render(<UsageCard hidden={false} onUsage={() => {}} onAccounts={() => {}} />);
  });
  expect(screen.getByText("사용량을 아직 불러오지 못했어요.")).toBeTruthy();
});
