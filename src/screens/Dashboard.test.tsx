// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api, type WebDashboardSummary } from "../api";
import { setLang } from "../i18n";
import { useRuns } from "../stores/runs";
import { useWeb } from "../stores/web";
import { Dashboard } from "./Dashboard";

const now = new Date();
const SUMMARY: WebDashboardSummary = {
  metrics: { todayRuns: 4, percentageChange: 0, averageDurationSeconds: 3.8, successRate: 0.975 },
  hourlyCounts: {},
  workflowStats: { total: 2, active: 2, inactive: 0, errored: 0, running: 0 },
};

beforeAll(() => setLang("ko"));
beforeEach(() => {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([
    {
      runId: "7f3a21c9-0000",
      workflowId: "w1",
      status: "succeeded",
      startedAt: now.toISOString(),
      finishedAt: now.toISOString(),
      fileOps: 0,
      undone: false,
      trigger: "watch",
    },
  ]);
  vi.spyOn(api, "dashboardSummary").mockResolvedValue(SUMMARY);
  vi.spyOn(api, "dashboardExecutions").mockResolvedValue({
    content: [
      {
        id: "a41d09e2-1111",
        workflowId: "x",
        workflowName: "웹훅 질문",
        status: "SUCCESS",
        durationSeconds: 3.2,
        triggerType: "WEBHOOK",
        startedAt: now.toISOString(),
        finishedAt: now.toISOString(),
      },
    ],
    hasNext: false,
    nextCursor: null,
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useRuns.setState({ runs: [], live: {} });
  useWeb.setState({ offline: false });
});

const mount = async () => {
  await act(async () => {
    render(<Dashboard onOpenRun={() => {}} />);
  });
};

test("dashboard_tabs_filter_rows", async () => {
  await mount();
  expect(screen.getByText("RUN-7F3A21C9")).toBeTruthy();
  expect(screen.getByText("RUN-A41D09E2")).toBeTruthy();
  expect(screen.getByText("폴더 감시")).toBeTruthy();
  expect(screen.getByText("웹훅")).toBeTruthy();
  // 전체: 오늘 실행 = 1 + 4
  expect(screen.getByTestId("today-runs").textContent).toContain("5");

  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "내 PC" })));
  expect(screen.getByText("RUN-7F3A21C9")).toBeTruthy();
  expect(screen.queryByText("RUN-A41D09E2")).toBeNull();
  expect(screen.getByTestId("today-runs").textContent).toContain("1");
});

test("dashboard_web_offline_shows_badge_keeps_local", async () => {
  vi.mocked(api.dashboardSummary).mockRejectedValue({ code: "NETWORK" });
  vi.mocked(api.dashboardExecutions).mockRejectedValue({ code: "NETWORK" });
  await mount();
  expect(screen.getByText("웹 이음 연결 안 됨")).toBeTruthy();
  expect(screen.getByText("RUN-7F3A21C9")).toBeTruthy();
  expect(screen.getByTestId("today-runs").textContent).toContain("1");

  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "웹 이음" })));
  expect(screen.queryByTestId("today-runs")).toBeNull();
});
