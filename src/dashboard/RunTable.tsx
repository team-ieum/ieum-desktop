import { CheckCircle2, CircleX, Globe, Laptop, Loader2 } from "lucide-react";
import type { RunSummary, WebDashboardExecution } from "../api";
import { t, type Key } from "../i18n";
import { formatDuration, formatRelative } from "./format";

export type Row = {
  source: "local" | "web";
  id: string;
  /** 웹 행을 열 때 쓴다. */
  workflowId: string;
  workflowName: string;
  status: "success" | "error" | "running";
  durationSeconds: number | null;
  trigger: string | null;
  at: string;
};

/** 끝 시각 없는 기록은 실행 중(History RunPill과 같은 규칙). */
export const localRow = (r: RunSummary, name: string): Row => ({
  source: "local",
  id: r.runId,
  workflowId: r.workflowId,
  workflowName: name,
  status: !r.finishedAt ? "running" : r.status === "succeeded" ? "success" : "error",
  durationSeconds: r.finishedAt ? (Date.parse(r.finishedAt) - Date.parse(r.startedAt)) / 1000 : null,
  trigger: r.trigger ?? null,
  at: r.startedAt,
});

// 웹 mapWorkflowDashboardExecution
const webStatus = (s: string): Row["status"] => {
  const u = s.toUpperCase();
  if (["SUCCESS", "SUCCEEDED", "COMPLETED"].includes(u)) return "success";
  if (["FAILED", "FAILURE", "ERROR", "CANCELLED", "CANCELED"].includes(u)) return "error";
  return "running";
};
export const webRow = (e: WebDashboardExecution): Row => ({
  source: "web",
  id: e.id,
  workflowId: e.workflowId,
  workflowName: e.workflowName,
  status: webStatus(e.status),
  durationSeconds: e.durationSeconds,
  trigger: e.triggerType,
  at: e.finishedAt ?? e.startedAt ?? "",
});

const TRIGGER: Record<string, Key> = {
  watch: "dashboard.trigger.watch",
  manual: "dashboard.trigger.manual",
  MANUAL: "dashboard.trigger.manual",
  SCHEDULE: "dashboard.trigger.schedule",
  CRON: "dashboard.trigger.schedule",
  WEBHOOK: "dashboard.trigger.webhook",
  EVENT: "dashboard.trigger.webhook",
};
const triggerText = (v: string | null) => (v === null ? "—" : TRIGGER[v] ? t(TRIGGER[v]) : v);

const displayId = (id: string) => `RUN-${id.replace(/-/g, "").slice(0, 8).toUpperCase()}`;

const STATUS = {
  success: { cls: "border-main-light-blue bg-main-light-blue text-main-deep-blue", Icon: CheckCircle2 },
  error: { cls: "border-danger-300 bg-danger-100 text-danger-700", Icon: CircleX },
  running: { cls: "border-neutral-300 bg-neutral-white text-neutral-600", Icon: Loader2 },
};
const SOURCE = {
  local: { cls: "border-main-light-blue bg-main-light-blue text-main-deep-blue", Icon: Laptop },
  web: { cls: "border-neutral-200 bg-neutral-white text-main-blue", Icon: Globe },
};
const PILL = "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 typo-caption1_semibold";
const TH = "border-b border-neutral-200 px-5 py-2.5 text-left typo-caption1_semibold tracking-wide text-neutral-400";
const COLS: Key[] = ["dashboard.col.id", "dashboard.col.workflow", "dashboard.col.status", "dashboard.col.duration", "dashboard.col.trigger", "dashboard.col.at"];

export function RunTable({ rows, showSource = true, onOpen }: { rows: Row[]; showSource?: boolean; onOpen: (r: Row) => void }) {
  if (rows.length === 0) return <p className="px-5 py-8 text-center typo-body2_regular text-neutral-500">{t("dashboard.recentEmpty")}</p>;
  return (
    <table className="w-full border-collapse">
      <thead>
        <tr className="bg-neutral-50">
          {(showSource ? (["dashboard.col.source", ...COLS] as Key[]) : COLS).map((k) => (
            <th key={k} className={TH}>
              {t(k)}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => {
          const s = STATUS[r.status];
          const src = SOURCE[r.source];
          return (
            <tr
              key={`${r.source}:${r.id}`}
              tabIndex={0}
              className="cursor-pointer border-b border-neutral-100 hover:bg-neutral-50 focus-visible:bg-neutral-50 focus-visible:outline-none"
              onClick={() => onOpen(r)}
              onKeyDown={(e) => e.key === "Enter" && onOpen(r)}
            >
              {showSource && (
                <td className="px-5 py-3">
                  <span className={`${PILL} ${src.cls}`}>
                    <src.Icon className="size-3" aria-hidden="true" />
                    {t(`dashboard.source.${r.source}`)}
                  </span>
                </td>
              )}
              <td className="px-5 py-3 font-mono typo-body3_medium text-neutral-600">{displayId(r.id)}</td>
              <td className="px-5 py-3 typo-body2_regular text-neutral-900">{r.workflowName}</td>
              <td className="px-5 py-3">
                <span className={`${PILL} ${s.cls}`}>
                  <s.Icon className={`size-3 ${r.status === "running" ? "animate-spin" : ""}`} aria-hidden="true" />
                  {t(`dashboard.run.${r.status}`)}
                </span>
              </td>
              <td className="px-5 py-3 font-mono typo-body3_medium text-neutral-600">
                {r.durationSeconds === null || (r.status === "running" && r.durationSeconds <= 0) ? "—" : formatDuration(r.durationSeconds)}
              </td>
              <td className="px-5 py-3">
                <span className="inline-flex items-center rounded-md border border-neutral-200 bg-neutral-50 px-2 py-0.5 typo-caption1_medium text-neutral-600">
                  {triggerText(r.trigger)}
                </span>
              </td>
              <td className="px-5 py-3 typo-caption1_regular text-neutral-400">{r.at ? formatRelative(r.at) : "—"}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
