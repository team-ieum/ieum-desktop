import { Ban, CheckCircle2, ChevronLeft, ChevronRight, CircleX, File, Globe, Laptop, Loader2, Search, ShieldX, Undo2 } from "lucide-react";
import type { RunSummary, WebDashboardExecution } from "../api";
import { t, type Key } from "../i18n";
import { Dropdown } from "../usage/parts";
import { formatDuration, formatRelative } from "./format";

export type Row = {
  source: "local" | "web";
  id: string;
  /** 웹 행을 열 때 쓴다. */
  workflowId: string;
  workflowName: string;
  /** cancelled·rejected는 내 PC만 — History RunPill과 같은 구분. */
  status: "success" | "error" | "running" | "cancelled" | "rejected";
  durationSeconds: number | null;
  trigger: string | null;
  at: string;
  /** 내 PC만 — 실행 기록 표의 파일 작업 열. */
  fileOps?: number;
  /** 내 PC만 — 되돌린 실행. */
  undone?: boolean;
};

const LOCAL_STATUS = { succeeded: "success", failed: "error", cancelled: "cancelled", rejected: "rejected" } as const;

/** 끝 시각 없는 기록은 실행 중(History RunPill과 같은 규칙). */
export const localRow = (r: RunSummary, name: string): Row => ({
  source: "local",
  id: r.runId,
  workflowId: r.workflowId,
  workflowName: name,
  status: !r.finishedAt ? "running" : LOCAL_STATUS[r.status],
  durationSeconds: r.finishedAt ? (Date.parse(r.finishedAt) - Date.parse(r.startedAt)) / 1000 : null,
  trigger: r.trigger ?? null,
  at: r.startedAt,
  fileOps: r.fileOps,
  undone: r.undone,
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
const triggerText = (v: string) => (TRIGGER[v] ? t(TRIGGER[v]) : v);

// --- 필터·쪽 나누기(대시보드 최근 실행 로그·실행 기록 공용). 화면 상태만, 저장 안 함. ---
export const PAGE_SIZE = 10;
export type StatusFilter = "all" | Row["status"];
/** trigger는 화면 이름(MANUAL·manual이 같은 "수동")으로 고른다. */
export type Filters = { status: StatusFilter; trigger: string; q: string };
export const NO_FILTERS: Filters = { status: "all", trigger: "all", q: "" };
/** cancelled·rejected는 내 PC만 나온다. */
export const statusOptions = (withLocal: boolean): StatusFilter[] =>
  withLocal ? ["all", "success", "error", "running", "cancelled", "rejected"] : ["all", "success", "error", "running"];
/** 받은 행에 있는 트리거만. */
export const triggerOptions = (rows: Row[]) => [...new Set(rows.flatMap((r) => (r.trigger === null ? [] : [triggerText(r.trigger)])))];
export const filterRows = (rows: Row[], f: Filters) => {
  const q = f.q.trim().toLowerCase();
  return rows.filter(
    (r) =>
      (f.status === "all" || r.status === f.status) &&
      (f.trigger === "all" || (r.trigger !== null && triggerText(r.trigger) === f.trigger)) &&
      (!q || r.workflowName.toLowerCase().includes(q)),
  );
};
const filterStatusText = (s: StatusFilter) => (s === "all" ? t("runs.allStatus") : s === "error" ? t("history.filterError") : statusText(s));

/** 표 머리 오른쪽 — [상태 ⌄] [트리거 ⌄] [이름 검색]. */
export function RunFilters({ value, onChange, statuses, triggers }: { value: Filters; onChange: (f: Filters) => void; statuses: StatusFilter[]; triggers: string[] }) {
  const trig = ["all", ...triggers.filter((x) => x !== "all"), ...(value.trigger !== "all" && !triggers.includes(value.trigger) ? [value.trigger] : [])];
  return (
    <div className="flex flex-wrap items-center justify-end gap-2">
      <Dropdown
        label={t("dashboard.col.status")}
        items={statuses.map((id) => ({ id, name: filterStatusText(id), icon: null }))}
        sel={value.status}
        onSel={(status) => onChange({ ...value, status })}
      />
      <Dropdown
        label={t("dashboard.col.trigger")}
        items={trig.map((id) => ({ id, name: id === "all" ? t("runs.allTriggers") : id, icon: null }))}
        sel={value.trigger}
        onSel={(trigger) => onChange({ ...value, trigger })}
      />
      <label className="relative inline-flex h-8 w-48 items-center">
        <Search className="pointer-events-none absolute left-2.5 size-3.5 text-neutral-400" aria-hidden="true" />
        <input
          type="search"
          aria-label={t("runs.search")}
          placeholder={t("runs.search")}
          value={value.q}
          onChange={(e) => onChange({ ...value, q: e.target.value })}
          className="h-full w-full rounded-lg border border-[#cde9f4] bg-white pr-2.5 pl-8 typo-caption1_medium text-neutral-700 outline-none placeholder:text-neutral-400 focus:border-main-blue"
        />
      </label>
    </div>
  );
}

/** 필터 결과 0건. */
export function RunsNoMatch({ onClear }: { onClear: () => void }) {
  return (
    <div className="flex flex-col items-center gap-3 px-5 py-8">
      <p className="typo-body2_regular text-neutral-500">{t("history.noMatch")}</p>
      <button type="button" className="rounded-lg border border-neutral-200 bg-white px-3 py-1.5 typo-body3_semibold text-neutral-700 hover:bg-neutral-50" onClick={onClear}>
        {t("runs.clear")}
      </button>
    </div>
  );
}

/** 표 아래 — "N–M / 총 K건" [이전] 쪽 번호 [다음]. `more` = 커서에 더 있음(총수 모름). page는 0부터. */
export function Pager({ page, total, more = false, onPage }: { page: number; total: number; more?: boolean; onPage: (p: number) => void }) {
  const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const last = more ? Math.max(pages, page + 2) : pages;
  const from = Math.max(0, Math.min(page - 2, last - 5));
  const nums = Array.from({ length: Math.min(5, last) }, (_, i) => from + i);
  const range = (more ? t("runs.rangeMore") : t("runs.range"))
    .replace("{from}", String(total === 0 ? 0 : page * PAGE_SIZE + 1))
    .replace("{to}", String(Math.min(total, (page + 1) * PAGE_SIZE)))
    .replace("{total}", String(total));
  const btn = "inline-flex h-8 min-w-8 items-center justify-center gap-1 rounded-lg px-2 typo-body3_semibold disabled:opacity-40";
  return (
    <div className="flex items-center justify-between gap-4 border-t border-neutral-200 bg-neutral-50 px-5 py-2.5">
      <span className="typo-caption1_medium tabular-nums text-neutral-500">{range}</span>
      <nav className="flex items-center gap-1" aria-label={t("runs.pages")}>
        <button type="button" className={`${btn} text-neutral-600 hover:bg-white`} disabled={page === 0} onClick={() => onPage(page - 1)}>
          <ChevronLeft className="size-4" aria-hidden="true" />
          {t("runs.prev")}
        </button>
        {nums.map((p) => (
          <button
            key={p}
            type="button"
            aria-label={t("runs.page").replace("{n}", String(p + 1))}
            aria-current={p === page ? "page" : undefined}
            className={`${btn} tabular-nums ${p === page ? "bg-main-deep-blue text-white" : "text-neutral-600 hover:bg-white"}`}
            onClick={() => onPage(p)}
          >
            {p + 1}
          </button>
        ))}
        <button type="button" className={`${btn} text-neutral-600 hover:bg-white`} disabled={page >= last - 1} onClick={() => onPage(page + 1)}>
          {t("runs.next")}
          <ChevronRight className="size-4" aria-hidden="true" />
        </button>
      </nav>
    </div>
  );
}

const displayId = (id: string) => `RUN-${id.replace(/-/g, "").slice(0, 8).toUpperCase()}`;

const STATUS = {
  success: { cls: "border-main-light-blue bg-main-light-blue text-main-deep-blue", Icon: CheckCircle2 },
  error: { cls: "border-danger-300 bg-danger-100 text-danger-700", Icon: CircleX },
  running: { cls: "border-neutral-300 bg-neutral-white text-neutral-600", Icon: Loader2 },
  cancelled: { cls: "border-neutral-300 bg-neutral-white text-neutral-600", Icon: Ban },
  rejected: { cls: "border-danger-300 bg-danger-100 text-danger-700", Icon: ShieldX },
};
const statusText = (s: Row["status"]) => (s === "cancelled" || s === "rejected" ? t(`status.${s}`) : t(`dashboard.run.${s}`));
const SOURCE = {
  local: { cls: "border-main-light-blue bg-main-light-blue text-main-deep-blue", Icon: Laptop },
  web: { cls: "border-neutral-200 bg-neutral-white text-main-blue", Icon: Globe },
};
const PILL = "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 typo-caption1_semibold";
const TH = "border-b border-neutral-200 px-5 py-2.5 text-left typo-caption1_semibold tracking-wide text-neutral-400";
const COLS: Key[] = ["dashboard.col.id", "dashboard.col.workflow", "dashboard.col.status", "dashboard.col.duration", "dashboard.col.trigger", "dashboard.col.at"];

/** `history` = 실행 기록 표(파일 작업 열·화살표·실행 중 강조). */
export function RunTable({
  rows,
  showSource = true,
  history = false,
  onOpen,
}: {
  rows: Row[];
  showSource?: boolean;
  history?: boolean;
  onOpen: (r: Row) => void;
}) {
  const cols: Key[] = [...(showSource ? ["dashboard.col.source" as Key] : []), ...COLS];
  if (history) cols.splice(cols.length - 1, 0, "history.fileOps");
  if (rows.length === 0) return <p className="px-5 py-8 text-center typo-body2_regular text-neutral-500">{t("dashboard.recentEmpty")}</p>;
  return (
    <table className="w-full border-collapse">
      <thead>
        <tr className="bg-neutral-50">
          {cols.map((k) => (
            <th key={k} className={TH}>
              {t(k)}
            </th>
          ))}
          {history && <th className="border-b border-neutral-200" />}
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
              role="button"
              aria-label={`${displayId(r.id)} ${r.workflowName} ${statusText(r.status)}`}
              className={`cursor-pointer border-b border-neutral-100 hover:bg-neutral-50 focus-visible:bg-neutral-50 focus-visible:outline-none ${
                history && r.status === "running" ? "bg-main-light-blue/30" : ""
              }`}
              onClick={() => onOpen(r)}
              onKeyDown={(e) => {
                if (e.key !== "Enter" && e.key !== " ") return;
                e.preventDefault();
                onOpen(r);
              }}
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
                  {statusText(r.status)}
                </span>
              </td>
              <td className="px-5 py-3 font-mono typo-body3_medium text-neutral-600">
                {r.durationSeconds === null || (r.status === "running" && r.durationSeconds <= 0) ? "—" : formatDuration(r.durationSeconds)}
              </td>
              <td className="px-5 py-3">
                {r.trigger === null ? (
                  <span className="typo-body3_medium text-neutral-400">—</span>
                ) : (
                  <span className="inline-flex items-center rounded-md border border-neutral-200 bg-neutral-50 px-2 py-0.5 typo-caption1_medium text-neutral-600">
                    {triggerText(r.trigger)}
                  </span>
                )}
              </td>
              {history && (
                <td className="px-5 py-3 typo-body3_medium tabular-nums text-neutral-600">
                  <span className="inline-flex items-center gap-1.5">
                    <File className="size-3.5 text-neutral-400" aria-hidden="true" />
                    {t("history.fileCount").replace("{n}", String(r.fileOps ?? 0))}
                    {r.undone && (
                      <span className="inline-flex items-center gap-1 rounded-md border border-main-light-blue bg-main-light-blue px-1.5 py-0.5 typo-caption2_semibold text-main-deep-blue">
                        <Undo2 className="size-3" aria-hidden="true" />
                        {t("history.undone")}
                      </span>
                    )}
                  </span>
                </td>
              )}
              <td className="px-5 py-3 typo-caption1_regular text-neutral-400">{r.at ? formatRelative(r.at) : "—"}</td>
              {history && (
                <td className="w-10 py-3 pr-4 text-right">
                  <ChevronRight className="inline size-4 text-neutral-300" aria-hidden="true" />
                </td>
              )}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
