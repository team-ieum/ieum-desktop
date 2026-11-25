import { beforeAll, expect, test } from "vitest";
import type { RunSummary } from "../api";
import { setLang } from "../i18n";
import { filterRows, localRow, NO_FILTERS, triggerOptions } from "./RunTable";

beforeAll(() => setLang("ko"));

const run = (runId: string, trigger: RunSummary["trigger"]): RunSummary => ({
  runId,
  workflowId: "w1",
  status: "succeeded",
  startedAt: "2026-11-17T00:00:00Z",
  finishedAt: "2026-11-17T00:00:01Z",
  fileOps: 0,
  undone: false,
  trigger,
});

test("schedule_trigger_named_in_filter", () => {
  const rows = [localRow(run("a", "schedule"), "x"), localRow(run("b", "watch"), "x")];
  expect(triggerOptions(rows)).toEqual(["스케줄", "폴더 감시"]);
  expect(filterRows(rows, { ...NO_FILTERS, trigger: "스케줄" }).map((r) => r.id)).toEqual(["a"]);
});

test("github_trigger_named_github", () => {
  expect(triggerOptions([localRow(run("a", "github"), "x")])).toEqual(["GitHub"]);
});
