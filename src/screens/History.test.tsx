// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type RunSummary, type StoredRun } from "../api";
import { setLang } from "../i18n";
import { History, RunPill } from "./History";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

// 재리뷰 N-2: 끝 시각 없는 기록(실행 도중 저장)은 "중단됨"이 아니라 "실행 중"
test("run_pill_unfinished_shows_running", () => {
  render(<RunPill status="cancelled" finishedAt="" />);
  expect(screen.getByText("실행 중")).toBeTruthy();
  cleanup();
  render(<RunPill status="cancelled" finishedAt="2026-11-11T00:00:00Z" />);
  expect(screen.getByText("중단됨")).toBeTruthy();
});

test("history_table_shows_trigger_column", async () => {
  const run = (runId: string, trigger: RunSummary["trigger"]): RunSummary => ({
    runId,
    workflowId: "w1",
    status: "succeeded",
    startedAt: "2026-11-11T00:00:00Z",
    finishedAt: "2026-11-11T00:00:02Z",
    fileOps: 3,
    undone: false,
    trigger,
  });
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([run("aaaa1111", "watch"), run("bbbb2222", null)]);
  await act(async () => {
    render(<History />);
  });
  expect(screen.getByText("RUN-AAAA1111")).toBeTruthy();
  expect(screen.getByText("폴더 감시")).toBeTruthy();
  expect(screen.getByText("—")).toBeTruthy();
  expect(screen.getAllByText("3개")).toHaveLength(2);
  expect(screen.getByText("1–2 / 총 2건")).toBeTruthy();
  vi.restoreAllMocks();
});

test("history_filter_failed_label_no_match_and_undone", async () => {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([
    { runId: "u1", workflowId: "w1", status: "succeeded", startedAt: "2026-11-11T00:00:00Z", finishedAt: "2026-11-11T00:00:01Z", fileOps: 2, undone: true },
  ]);
  await act(async () => {
    render(<History />);
  });
  expect(screen.getByText("되돌림")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /실패/ })));
  expect(screen.getByText("조건에 맞는 실행이 없어요.")).toBeTruthy();
  expect(screen.queryByText("아직 실행 기록이 없어요.")).toBeNull();
  vi.restoreAllMocks();
});

test("history_detail_shows_late_schedule_run", async () => {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  const stored = (startedAt: string) => ({
    run: { status: "succeeded", nodes: {}, validationErrors: [] },
    workflowId: "w1",
    startedAt,
    finishedAt: startedAt,
    fileOps: [],
    allowedRoots: [],
    undo: null,
    trigger: "schedule",
    scheduledFor: "2026-11-17T09:00:00",
  }) as unknown as StoredRun;
  // 09:00 KST = 00:00Z. 10:30 KST에 돌았다.
  vi.spyOn(api, "getRun").mockResolvedValue(stored("2026-11-17T01:30:00Z"));
  await act(async () => {
    render(<History initialRun="r1" />);
  });
  expect(screen.getByText(/스케줄/)).toBeTruthy();
  expect(screen.getByText(/예정 09:00, 늦게 실행/)).toBeTruthy();
  cleanup();
  // 제때 돈 실행은 "늦게"를 붙이지 않는다.
  vi.spyOn(api, "getRun").mockResolvedValue(stored("2026-11-17T00:00:10Z"));
  await act(async () => {
    render(<History initialRun="r1" />);
  });
  expect(screen.getByText(/스케줄/)).toBeTruthy();
  expect(screen.queryByText(/늦게 실행/)).toBeNull();
  vi.restoreAllMocks();
});
