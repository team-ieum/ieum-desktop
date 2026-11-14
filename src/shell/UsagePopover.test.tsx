// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type CliAccountMeta, type CredentialsView, type PlanUsage } from "../api";
import { setLang } from "../i18n";
import { useCredentials } from "../stores/credentials";
import { UsagePopover } from "./UsagePopover";

const inHours = (h: number) => new Date(Date.now() + h * 3_600_000).toISOString();
const plan = (usedPct: number): PlanUsage => ({
  state: "ok",
  windows: [{ kind: "five_hour", usedPct, resetsAt: inHours(2) }],
  checkedAt: new Date().toISOString(),
  accountEmail: null,
});
const OUT: PlanUsage = { state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null };
const SYS: CliAccountMeta = { id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: null };
const WORK: CliAccountMeta = { id: "abcd", label: "회사", isDefault: false, createdAt: "", email: null };
const CWORK: CliAccountMeta = { id: "cx01", label: "개인", isDefault: false, createdAt: "", email: null };
const VIEW: CredentialsView = { apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [SYS, WORK], codexAccounts: [SYS, CWORK], keychainError: null };

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
  useCredentials.setState({ view: null });
});

/** Claude: 이 Mac(기본) 100%, 회사 12%. Codex: 이 Mac(기본) 50%, 개인 30%. */
const mount = async () => {
  useCredentials.setState({ view: VIEW });
  vi.spyOn(api, "listCredentials").mockResolvedValue(VIEW);
  vi.spyOn(api, "subscriptionUsage").mockImplementation(async (k, id) =>
    k === "claude" ? plan(id === "system" ? 100 : 12) : plan(id === "system" ? 50 : 30),
  );
  vi.spyOn(api, "antigravityPlanUsage").mockResolvedValue(OUT);
  vi.spyOn(api, "usageSummary").mockResolvedValue({ totalUsd: 0, totalInput: 0, totalOutput: 0, unpricedCalls: 0, bySource: [], byCredential: [], byWorkflow: [], byDay: [], asOf: "" });
  await act(async () => {
    render(<UsagePopover onUsage={() => {}} onSettings={() => {}} />);
  });
};
const hoverRow = async (name: RegExp) => act(async () => fireEvent.mouseEnter(screen.getByRole("button", { name })));
const confirm = () => screen.queryByRole("dialog", { name: /바꿀까요/ });

test("popover_account_click_opens_confirm", async () => {
  await mount();
  const set = vi.spyOn(api, "setDefaultCredential").mockResolvedValue();
  await hoverRow(/^Claude/);
  const fly = within(screen.getByRole("region", { name: "Claude" }));
  await act(async () => fireEvent.click(fly.getByRole("button", { name: /회사/ })));
  const dlg = within(screen.getByRole("dialog", { name: "기본 Claude 계정을 바꿀까요?" }));
  expect(dlg.getByText("이 Mac의 로그인 → 회사 (12% 사용)")).toBeTruthy();
  expect(dlg.getByText(/'기본'으로 둔 AI 노드는 이제 이 계정을 써요/)).toBeTruthy();
  await act(async () => fireEvent.click(dlg.getByRole("button", { name: "바꾸기" })));
  expect(set).toHaveBeenCalledTimes(1);
  expect(set).toHaveBeenCalledWith("claude", "abcd");
  expect(confirm()).toBeNull();
});

test("expired_account_shows_login_expired", async () => {
  await mount();
  vi.mocked(api.subscriptionUsage).mockImplementation(async (k, id) =>
    k === "claude" && id === "abcd" ? { ...OUT, state: "expired" } : plan(id === "system" ? 100 : 30),
  );
  cleanup();
  await act(async () => {
    render(<UsagePopover onUsage={() => {}} onSettings={() => {}} />);
  });
  await hoverRow(/^Claude/);
  const work = within(screen.getByRole("region", { name: "Claude" })).getByRole("button", { name: /회사/ });
  expect(within(work).getByText("로그인 만료")).toBeTruthy();
  expect(within(work).queryByText(/%/)).toBeNull();
});

test("default_account_not_clickable", async () => {
  await mount();
  await hoverRow(/^Claude/);
  const fly = within(screen.getByRole("region", { name: "Claude" }));
  const me = fly.getByRole("button", { name: /이 Mac의 로그인/ });
  expect(me.getAttribute("aria-disabled")).toBe("true");
  await act(async () => fireEvent.click(me));
  expect(confirm()).toBeNull();
});

test("codex_confirm_text", async () => {
  await mount();
  await hoverRow(/^Codex/);
  const fly = within(screen.getByRole("region", { name: "Codex" }));
  await act(async () => fireEvent.click(fly.getByRole("button", { name: /개인/ })));
  const dlg = within(screen.getByRole("dialog", { name: "기본 Codex 계정을 바꿀까요?" }));
  expect(dlg.getByText("IEUM에서 Codex 구독을 쓸 때 이 계정을 써요.")).toBeTruthy();
  expect(dlg.queryByText(/AI 노드/)).toBeNull();
  // 취소하면 바꾸지 않고 닫힌다.
  const set = vi.spyOn(api, "setDefaultCredential");
  await act(async () => fireEvent.click(dlg.getByRole("button", { name: "취소" })));
  expect(set).not.toHaveBeenCalled();
  expect(confirm()).toBeNull();
});

test("hover_switch_waits_150ms", async () => {
  await mount();
  vi.useFakeTimers();
  await hoverRow(/^Claude/);
  expect(screen.getByRole("region", { name: "Claude" })).toBeTruthy();
  // 대각선으로 패널에 가다 Codex 줄을 스쳐도 패널에 들어가면 그대로.
  await hoverRow(/^Codex/);
  await act(async () => vi.advanceTimersByTime(100));
  await act(async () => fireEvent.mouseEnter(screen.getByRole("region", { name: "Claude" })));
  await act(async () => vi.advanceTimersByTime(200));
  expect(screen.getByRole("region", { name: "Claude" })).toBeTruthy();
  // 줄에 머물면 150ms 뒤 바뀐다.
  await hoverRow(/^Codex/);
  await act(async () => vi.advanceTimersByTime(149));
  expect(screen.getByRole("region", { name: "Claude" })).toBeTruthy();
  await act(async () => vi.advanceTimersByTime(1));
  expect(screen.getByRole("region", { name: "Codex" })).toBeTruthy();
});

test("confirm_close_returns_focus_to_account", async () => {
  await mount();
  vi.spyOn(api, "setDefaultCredential").mockResolvedValue();
  await hoverRow(/^Claude/);
  const fly = within(screen.getByRole("region", { name: "Claude" }));
  const work = fly.getByRole("button", { name: /회사/ });
  const dlg = () => within(screen.getByRole("dialog", { name: /바꿀까요/ }));
  // 취소
  await act(async () => fireEvent.click(work));
  await act(async () => fireEvent.click(dlg().getByRole("button", { name: "취소" })));
  expect(document.activeElement).toBe(work);
  // Esc
  await act(async () => fireEvent.click(work));
  await act(async () => fireEvent.keyDown(dlg().getByRole("button", { name: "취소" }), { key: "Escape" }));
  expect(confirm()).toBeNull();
  expect(document.activeElement).toBe(work);
  // 바꾸기 — 그 줄이 기본(aria-disabled)이 돼도 포커스는 받는다.
  vi.spyOn(api, "listCredentials").mockResolvedValue({ ...VIEW, claudeAccounts: [{ ...SYS, isDefault: false }, { ...WORK, isDefault: true }] });
  await act(async () => fireEvent.click(work));
  await act(async () => fireEvent.click(dlg().getByRole("button", { name: "바꾸기" })));
  expect(confirm()).toBeNull();
  const now = within(screen.getByRole("region", { name: "Claude" })).getByRole("button", { name: /회사/ });
  expect(now.getAttribute("aria-disabled")).toBe("true");
  expect(document.activeElement).toBe(now);
});

test("switch_failure_keeps_dialog_and_busy_blocks_close", async () => {
  await mount();
  let fail!: (e: unknown) => void;
  vi.spyOn(api, "setDefaultCredential").mockReturnValue(new Promise<void>((_, rej) => (fail = rej)));
  await hoverRow(/^Claude/);
  await act(async () => fireEvent.click(within(screen.getByRole("region", { name: "Claude" })).getByRole("button", { name: /회사/ })));
  const dlg = within(screen.getByRole("dialog", { name: /바꿀까요/ }));
  await act(async () => fireEvent.click(dlg.getByRole("button", { name: "바꾸기" })));
  // 바꾸는 중에는 Esc·취소·배경 클릭으로 닫히지 않는다.
  await act(async () => fireEvent.keyDown(dlg.getByRole("button", { name: "취소" }), { key: "Escape" }));
  await act(async () => fireEvent.click(dlg.getByRole("button", { name: "취소" })));
  await act(async () => fireEvent.mouseDown(screen.getByRole("dialog", { name: /바꿀까요/ }).parentElement!));
  expect(confirm()).toBeTruthy();
  // 실패하면 창은 남고 오류 문구.
  await act(async () => fail("keychain"));
  expect(confirm()).toBeTruthy();
  expect(dlg.getByRole("alert").textContent).toBe("문제가 생겼어요");
});
