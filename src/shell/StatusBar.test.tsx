// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
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

const mount = async () => {
  await act(async () => {
    render(<StatusBar onOpen={() => {}} />);
  });
};

test("status_bar_shows_plan_and_today_cost", async () => {
  const plan: PlanUsage = {
    state: "ok",
    windows: [
      { kind: "five_hour", usedPct: 34, resetsAt: inHours(58) },
      { kind: "seven_day", usedPct: 10, resetsAt: inHours(100) },
    ],
    checkedAt: new Date().toISOString(),
  };
  vi.spyOn(api, "claudePlanUsage").mockResolvedValue(plan);
  const summary = vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await mount();
  expect(summary).toHaveBeenCalledWith("today");
  expect(screen.getByText("Claude 66% · 2일 10시간")).toBeTruthy();
  expect(screen.getByText("오늘 API 예상 $0.42")).toBeTruthy();
});

test("status_bar_hides_claude_when_not_logged_in", async () => {
  vi.spyOn(api, "claudePlanUsage").mockResolvedValue({ state: "not_logged_in", windows: [], checkedAt: "" });
  vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await mount();
  expect(screen.queryByText(/Claude/)).toBeNull();
  expect(screen.getByText("오늘 API 예상 $0.42")).toBeTruthy();
});

test("status_bar_unavailable_when_commands_fail", async () => {
  vi.spyOn(api, "claudePlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
  await mount();
  expect(screen.getByText("Claude 확인 불가")).toBeTruthy();
  expect(screen.getByText("오늘 API 예상 —")).toBeTruthy();
});
