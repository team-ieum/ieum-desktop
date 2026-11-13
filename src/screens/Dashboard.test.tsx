// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api, type RunSummary, type WebDashboardExecution, type WebDashboardSummary } from "../api";
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
    render(<Dashboard onOpenRun={() => {}} onUsage={() => {}} onAccounts={() => {}} />);
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
  // 전체 탭: 비율은 합치지 않고 출처별로 둘 다
  expect(document.body.textContent).toContain("100.0%");
  expect(document.body.textContent).toContain("97.5%");

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

test("dashboard_non_network_error_no_offline_badge", async () => {
  vi.mocked(api.dashboardSummary).mockRejectedValue({ code: "BAD_INPUT" });
  await mount();
  expect(screen.queryByText("웹 이음 연결 안 됨")).toBeNull();
});

test("dashboard_tabs_arrow_keys_and_panel", async () => {
  await mount();
  const all = screen.getByRole("tab", { name: "전체" });
  expect(all.getAttribute("aria-controls")).toBe(screen.getByRole("tabpanel").id);
  await act(async () => fireEvent.keyDown(all, { key: "ArrowRight" }));
  const local = screen.getByRole("tab", { name: "내 PC" });
  expect(local.getAttribute("aria-selected")).toBe("true");
  expect(document.activeElement).toBe(local);
  await act(async () => fireEvent.keyDown(local, { key: "ArrowLeft" }));
  await act(async () => fireEvent.keyDown(screen.getByRole("tab", { name: "전체" }), { key: "ArrowLeft" }));
  expect(screen.getByRole("tab", { name: "웹 이음" }).getAttribute("aria-selected")).toBe("true");
});

test("dashboard_row_space_opens_and_web_failure_logged", async () => {
  const onOpenRun = vi.fn();
  vi.spyOn(api, "openWebWorkflow").mockRejectedValue({ code: "NETWORK" });
  const log = vi.spyOn(console, "error").mockImplementation(() => {});
  await act(async () => {
    render(<Dashboard onOpenRun={onOpenRun} onUsage={() => {}} onAccounts={() => {}} />);
  });
  fireEvent.keyDown(screen.getByRole("button", { name: /RUN-7F3A21C9/ }), { key: " " });
  expect(onOpenRun).toHaveBeenCalledWith("7f3a21c9-0000");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /RUN-A41D09E2/ })));
  expect(log).toHaveBeenCalled();
});

// --- 최근 실행 로그: 페이지네이션·필터 ---
const ago = (min: number) => new Date(now.getTime() - min * 60_000).toISOString();
const localRun = (i: number, over: Partial<RunSummary> = {}): RunSummary => ({
  runId: `l${String(i).padStart(7, "0")}`,
  workflowId: "w1",
  status: "succeeded",
  startedAt: ago(i),
  finishedAt: ago(i),
  fileOps: 0,
  undone: false,
  trigger: "watch",
  ...over,
});
const webExec = (i: number): WebDashboardExecution => ({
  id: `e${String(i).padStart(7, "0")}`,
  workflowId: "x",
  workflowName: `웹 ${i}`,
  status: "SUCCESS",
  durationSeconds: 1,
  triggerType: "WEBHOOK",
  startedAt: ago(i),
  finishedAt: ago(i),
});
const tableRows = () => screen.queryAllByRole("button", { name: /^RUN-/ });
const pick = async (label: RegExp, option: string) => {
  await act(async () => fireEvent.click(screen.getByRole("button", { name: label })));
  await act(async () => fireEvent.click(screen.getByRole("option", { name: option })));
};

test("runs_table_paginates_10_per_page", async () => {
  vi.mocked(api.listRuns).mockResolvedValue(Array.from({ length: 23 }, (_, i) => localRun(i)));
  await mount();
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "내 PC" })));
  expect(tableRows()).toHaveLength(10);
  expect(screen.getByText("1–10 / 총 23건")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "2쪽" })));
  expect(screen.getByText("11–20 / 총 23건")).toBeTruthy();
  expect(screen.getByText("RUN-L0000010")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "다음" })));
  expect(tableRows()).toHaveLength(3);
  expect(screen.getByRole("button", { name: "다음" })).toHaveProperty("disabled", true);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "이전" })));
  expect(screen.getByText("11–20 / 총 23건")).toBeTruthy();
});

test("runs_table_filters_status_and_trigger", async () => {
  vi.mocked(api.listRuns).mockResolvedValue([
    localRun(1),
    localRun(2, { status: "failed" }),
    localRun(3, { status: "failed", trigger: "manual" }),
    localRun(4, { status: "cancelled" }),
  ]);
  await mount();
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "내 PC" })));
  await pick(/^상태:/, "실패");
  expect(tableRows().map((r) => r.textContent)).toHaveLength(2);
  await pick(/^트리거:/, "수동");
  expect(tableRows()).toHaveLength(1);
  expect(screen.getByText("RUN-L0000003")).toBeTruthy();
  // 웹 탭: 트리거는 받은 값(웹훅)만, 상태엔 취소·거부가 없다
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "웹 이음" })));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /^트리거:/ })));
  expect(screen.getByRole("option", { name: "웹훅" })).toBeTruthy();
  expect(screen.queryByRole("option", { name: "폴더 감시" })).toBeNull();
});

test("runs_table_search_by_name", async () => {
  vi.mocked(api.listWorkflows).mockResolvedValue([
    { id: "w1", name: "사진 정리" },
    { id: "w2", name: "문서 백업" },
  ] as never);
  vi.mocked(api.listRuns).mockResolvedValue([localRun(1), localRun(2, { workflowId: "w2" })]);
  await mount();
  await act(async () => fireEvent.change(screen.getByRole("searchbox", { name: "워크플로우 이름 검색" }), { target: { value: "백업" } }));
  expect(screen.getByText("RUN-L0000002")).toBeTruthy();
  expect(screen.queryByText("RUN-L0000001")).toBeNull();
  expect(screen.queryByText("RUN-A41D09E2")).toBeNull();
});

test("filter_change_resets_to_page_one", async () => {
  vi.mocked(api.listRuns).mockResolvedValue(Array.from({ length: 25 }, (_, i) => localRun(i)));
  await mount();
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "내 PC" })));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "3쪽" })));
  expect(screen.getByText("21–25 / 총 25건")).toBeTruthy();
  await pick(/^상태:/, "성공");
  expect(screen.getByText("1–10 / 총 25건")).toBeTruthy();
});

test("web_runs_fetch_next_cursor_only_when_needed", async () => {
  vi.mocked(api.dashboardExecutions)
    .mockResolvedValueOnce({ content: Array.from({ length: 20 }, (_, i) => webExec(i)), hasNext: true, nextCursor: "c2" })
    .mockResolvedValueOnce({ content: Array.from({ length: 5 }, (_, i) => webExec(20 + i)), hasNext: false, nextCursor: null });
  await mount();
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "웹 이음" })));
  expect(api.dashboardExecutions).toHaveBeenCalledTimes(1);
  expect(screen.getByText("1–10 / 20건+ · 더 있음")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "다음" })));
  expect(api.dashboardExecutions).toHaveBeenCalledTimes(1);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "다음" })));
  expect(api.dashboardExecutions).toHaveBeenCalledTimes(2);
  expect(api.dashboardExecutions).toHaveBeenLastCalledWith("c2");
  expect(screen.getByText("21–25 / 총 25건")).toBeTruthy();
  // 이미 받은 쪽으로 돌아가도 다시 받지 않는다
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "1쪽" })));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "3쪽" })));
  expect(api.dashboardExecutions).toHaveBeenCalledTimes(2);
});

test("empty_filter_result_has_clear_button", async () => {
  await mount();
  await act(async () => fireEvent.change(screen.getByRole("searchbox", { name: "워크플로우 이름 검색" }), { target: { value: "없는이름" } }));
  expect(tableRows()).toHaveLength(0);
  expect(screen.getByText("조건에 맞는 실행이 없어요.")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "필터 지우기" })));
  expect(tableRows()).toHaveLength(2);
  expect((screen.getByRole("searchbox", { name: "워크플로우 이름 검색" }) as HTMLInputElement).value).toBe("");
});

test("dashboard_local_cancelled_rejected_match_run_pill", async () => {
  const base = { workflowId: "w1", startedAt: now.toISOString(), finishedAt: now.toISOString(), fileOps: 0, undone: false };
  vi.mocked(api.listRuns).mockResolvedValue([
    { ...base, runId: "c0000000", status: "cancelled" },
    { ...base, runId: "r0000000", status: "rejected" },
  ]);
  await mount();
  expect(screen.getByRole("button", { name: /RUN-C0000000.*중단됨/ })).toBeTruthy();
  expect(screen.getByRole("button", { name: /RUN-R0000000.*거부됨/ })).toBeTruthy();
});
