// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type RunSummary } from "../api";
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
  expect(screen.getByText("2개 중 2개 표시")).toBeTruthy();
  vi.restoreAllMocks();
});
