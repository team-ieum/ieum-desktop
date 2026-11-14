import { expect, test } from "vitest";
import type { RunSummary, WorkflowEntry } from "../api";
import { emptyLive } from "../stores/runs";
import { localSide, merge, webSide, type Side } from "./metrics";

const NOW = new Date(2026, 10, 12, 15, 0, 0);
const at = (day: number, hour: number, sec = 0) => new Date(2026, 10, day, hour, 0, sec).toISOString();
const run = (p: Partial<RunSummary> & { startedAt: string }): RunSummary => ({
  runId: Math.random().toString(36).slice(2),
  workflowId: "w1",
  status: "succeeded",
  finishedAt: p.startedAt,
  fileOps: 0,
  undone: false,
  ...p,
});
const wf = (id: string, enabled: boolean) =>
  ({ id, name: id, enabled, workflow: { nodes: [], edges: [] }, updatedAt: "", watch: "off", watchMessage: null }) as WorkflowEntry;

test("local_side_counts_today_only", () => {
  const runs = [
    run({ startedAt: at(12, 9), finishedAt: at(12, 9, 2) }),
    run({ startedAt: at(12, 9), finishedAt: at(12, 9, 4) }),
    run({ startedAt: at(12, 13), status: "failed" }),
    run({ startedAt: at(11, 10) }),
    run({ startedAt: at(11, 11) }),
  ];
  const s = localSide(runs, [], {}, NOW);
  expect(s.todayRuns).toBe(3);
  expect(s.successRate).toBeCloseTo(200 / 3);
  expect(s.changePct).toBe(50);
  expect(s.avgSeconds).toBe(3);
  expect(s.hourly).toEqual({ "09": 2, "13": 1 });
});

// 로컬(KST) 자정 경계: 00:30은 UTC로 전날, 23:50은 UTC로 같은 날 14:50 — UTC 날짜로 세면 둘 다 틀린다.
test("local_side_day_boundary_uses_local_time", () => {
  expect(new Date().getTimezoneOffset()).toBe(-540);
  const runs = [
    run({ startedAt: new Date(2026, 10, 12, 0, 30).toISOString() }),
    run({ startedAt: new Date(2026, 10, 11, 23, 50).toISOString() }),
    run({ startedAt: new Date(2026, 10, 11, 0, 30).toISOString() }),
  ];
  const s = localSide(runs, [], {}, new Date(2026, 10, 12, 8, 0));
  expect(s.todayRuns).toBe(1);
  expect(s.hourly).toEqual({ "00": 1 });
  expect(s.changePct).toBe(-50);
});

test("local_side_no_finished_runs_rate_null", () => {
  const s = localSide([run({ startedAt: at(12, 9), finishedAt: "" })], [], {}, NOW);
  expect(s.todayRuns).toBe(1);
  expect(s.successRate).toBeNull();
  expect(s.avgSeconds).toBeNull();
  expect(s.changePct).toBeNull();
});

// 채팅 빌더의 노드 테스트 실행은 대시보드 합계에 넣지 않는다.
test("local_side_ignores_test_runs", () => {
  const runs = [
    run({ startedAt: at(12, 9), finishedAt: at(12, 9, 2) }),
    run({ startedAt: at(12, 10), status: "failed", trigger: "test" }),
  ];
  const s = localSide(runs, [wf("w1", true)], {}, NOW);
  expect(s.todayRuns).toBe(1);
  expect(s.successRate).toBe(100);
  expect(Object.values(s.hourly).reduce((a, b) => a + b, 0)).toBe(1);
  expect(s.stats.errored).toBe(0);
});

test("local_side_errored_uses_latest_run", () => {
  const runs = [
    run({ workflowId: "a", startedAt: at(12, 8), status: "failed" }),
    run({ workflowId: "a", startedAt: at(12, 9) }),
    run({ workflowId: "b", startedAt: at(10, 9) }),
    run({ workflowId: "b", startedAt: at(11, 9), status: "failed" }),
  ];
  const live = { b: { ...emptyLive("b"), status: "running" as const } };
  const s = localSide(runs, [wf("a", true), wf("b", true), wf("c", false)], live, NOW);
  expect(s.stats).toEqual({ active: 2, inactive: 1, errored: 1, running: 1 });
});

const side = (todayRuns: number, hourly: Record<string, number>): Side => ({
  todayRuns,
  changePct: 10,
  avgSeconds: 2,
  successRate: 1,
  hourly,
  stats: { active: 1, inactive: 2, errored: 0, running: 1 },
});

test("merge_adds_counts_not_rates", () => {
  const m = merge(side(3, { "09": 1 }), side(4, { "09": 2, "10": 1 }));
  expect(m.todayRuns).toBe(7);
  expect(m.hourly).toEqual({ "09": 3, "10": 1 });
  expect(m.stats).toEqual({ active: 2, inactive: 4, errored: 0, running: 2 });
  expect("successRate" in m).toBe(false);
});

test("merge_web_offline_is_local_only", () => {
  const local = side(3, { "09": 3 });
  const m = merge(local, null);
  expect(m.todayRuns).toBe(local.todayRuns);
  expect(m.perSource.web).toBeNull();
});

test("web_side_pads_hours_and_percent", () => {
  const s = webSide({
    // BE 단위 그대로 0~100 — 0.5%는 0.5%다.
    metrics: { todayRuns: 4, percentageChange: 0.5, averageDurationSeconds: 3.8, successRate: 97.5 },
    hourlyCounts: { "9": 2 },
    workflowStats: { total: 3, active: 2, inactive: 1, errored: 0, running: 0 },
  });
  expect(s.hourly).toEqual({ "09": 2 });
  expect(s.changePct).toBe(0.5);
  expect(s.successRate).toBe(97.5);
  expect(s.stats.active).toBe(2);
});
