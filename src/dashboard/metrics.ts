import type { RunSummary, WebDashboardSummary, WorkflowEntry } from "../api";
import type { Live } from "../stores/runs";
import { toPercent } from "./format";

export type Source = "all" | "local" | "web";

export type Stats = { active: number; inactive: number; errored: number; running: number };
export type Side = {
  todayRuns: number;
  changePct: number | null;
  avgSeconds: number | null;
  successRate: number | null;
  hourly: Record<string, number>;
  stats: Stats;
};

const pad = (h: number | string) => String(h).padStart(2, "0");
// 로컬 시각대 날짜 — 웹 대시보드(KST)와 같은 날 경계.
const day = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;

export function localSide(runs: RunSummary[], workflows: WorkflowEntry[], live: Record<string, Live>, now: Date): Side {
  const today = day(now);
  const y = new Date(now);
  y.setDate(y.getDate() - 1);
  const yesterday = day(y);

  const todays = runs.filter((r) => day(new Date(r.startedAt)) === today);
  const yCount = runs.filter((r) => day(new Date(r.startedAt)) === yesterday).length;
  const finished = todays.filter((r) => r.finishedAt);
  const ok = finished.filter((r) => r.status === "succeeded");

  const hourly: Record<string, number> = {};
  for (const r of todays) {
    const h = pad(new Date(r.startedAt).getHours());
    hourly[h] = (hourly[h] ?? 0) + 1;
  }

  const latest = new Map<string, RunSummary>();
  for (const r of runs) {
    const cur = latest.get(r.workflowId);
    if (!cur || Date.parse(r.startedAt) > Date.parse(cur.startedAt)) latest.set(r.workflowId, r);
  }
  const active = workflows.filter((w) => w.enabled).length;

  return {
    todayRuns: todays.length,
    changePct: yCount ? ((todays.length - yCount) / yCount) * 100 : null,
    avgSeconds: ok.length
      ? ok.reduce((s, r) => s + (Date.parse(r.finishedAt) - Date.parse(r.startedAt)) / 1000, 0) / ok.length
      : null,
    successRate: finished.length ? ok.length / finished.length : null,
    hourly,
    stats: {
      active,
      inactive: workflows.length - active,
      errored: workflows.filter((w) => latest.get(w.id)?.status === "failed").length,
      running: Object.values(live).filter((l) => l.status === "running").length,
    },
  };
}

export function webSide(s: WebDashboardSummary): Side {
  const { todayRuns, percentageChange, averageDurationSeconds, successRate } = s.metrics;
  const { active, inactive, errored, running } = s.workflowStats;
  return {
    todayRuns,
    changePct: toPercent(percentageChange),
    avgSeconds: averageDurationSeconds,
    successRate,
    hourly: Object.fromEntries(Object.entries(s.hourlyCounts).map(([h, c]) => [pad(h), c])),
    stats: { active, inactive, errored, running },
  };
}

/** 건수만 합친다 — 비율(성공률·평균·어제 대비)은 출처별로 따로 둔다(스펙 3.2). */
export function merge(a: Side, b: Side | null) {
  const hourly = { ...a.hourly };
  for (const [h, c] of Object.entries(b?.hourly ?? {})) hourly[h] = (hourly[h] ?? 0) + c;
  const stats = { ...a.stats };
  if (b) for (const k of Object.keys(stats) as (keyof Stats)[]) stats[k] += b.stats[k];
  return { todayRuns: a.todayRuns + (b?.todayRuns ?? 0), hourly, stats, perSource: { local: a, web: b } };
}
