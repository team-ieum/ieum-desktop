import { ArrowLeft, ArrowRight, ChevronDown, History as HistoryIcon, Laptop, Loader2, Undo2, Workflow as WorkflowIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type RunStatus, type StoredRun, type UndoReport } from "../api";
import { localRow, PAGE_SIZE, Pager, RunsNoMatch, RunTable, type Row } from "../dashboard/RunTable";
import { nodeTitle } from "../editor/Canvas";
import { locale, t } from "../i18n";
import { useRuns } from "../stores/runs";
import { useWorkflows } from "../stores/workflows";
import { BUTTON, PRIMARY } from "./Editor";

const RUN_PILL: Record<RunStatus | "running", string> = {
  succeeded: "bg-main-light-blue text-main-deep-blue border-main-light-blue",
  failed: "bg-danger-100 text-danger-700 border-danger-300",
  rejected: "bg-danger-100 text-danger-700 border-danger-300",
  cancelled: "bg-neutral-white text-neutral-600 border-neutral-300",
  running: "bg-neutral-white text-main-deep-blue border-main-light-blue",
};

/** 끝 시각 없는 기록은 실행 도중 저장된 것 — 저장된 status(cancelled)가 아니라 "실행 중"으로 보인다. */
export function RunPill({ status, finishedAt }: { status: RunStatus; finishedAt: string }) {
  const s = finishedAt ? status : "running";
  return (
    <span className={`inline-flex shrink-0 items-center rounded-full border px-2.5 py-1 typo-caption1_semibold ${RUN_PILL[s]}`}>
      {t(`status.${s}`)}
    </span>
  );
}

const CARD = "rounded-xl border border-neutral-200 bg-white shadow-[0_1px_2px_rgba(16,24,40,.04)]";
const SECTION = `flex flex-col gap-3 p-5 ${CARD}`;
const ITEM = "flex flex-col gap-1 rounded-[10px] border border-neutral-200 bg-neutral-50 px-3.5 py-2.5";
const NODE_PILL: Record<string, string> = {
  succeeded: "border-main-light-blue bg-main-light-blue text-main-deep-blue",
  failed: "border-danger-300 bg-danger-100 text-danger-700",
};

const when = (iso: string) => (iso ? new Date(iso).toLocaleString(locale()) : "");

function UndoResult({ report }: { report: UndoReport }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="typo-body2_bold text-neutral-800">
        {t("history.restored")} {report.restored.length}
      </h3>
      {report.skipped.length > 0 && (
        <>
          <h3 className="typo-body2_bold text-danger-700">
            {t("history.skipped")} {report.skipped.length}
          </h3>
          <ul className="flex flex-col gap-1.5">
            {report.skipped.map(([op, skip], i) => (
              <li key={i} className="rounded-brand-sm border border-danger-300 bg-danger-50 p-2.5 typo-caption1_regular text-neutral-700">
                <span className="break-all">{op.to}</span>
                <p className="mt-1 typo-caption1_semibold text-danger-700">
                  {skip.reason === "failed" ? `${t("skip.failed")}: ${skip.message}` : t(`skip.${skip.reason}`)}
                </p>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

function RunDetail({ runId, onBack }: { runId: string; onBack: () => void }) {
  const [run, setRun] = useState<StoredRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [undoing, setUndoing] = useState(false);
  const wf = useWorkflows((s) => s.list.find((w) => w.id === run?.workflowId));
  const reloadRuns = useRuns((s) => s.load);

  useEffect(() => {
    api.getRun(runId).then(setRun, (e) => setError(String(e)));
  }, [runId]);

  const undo = async () => {
    if (!run) return;
    setUndoing(true);
    try {
      const report = await api.undoRun(runId);
      setRun({ ...run, undo: report });
      void reloadRuns();
    } catch (e) {
      setError(String(e));
    } finally {
      setUndoing(false);
    }
  };

  const nodeName = (id: string) => {
    const n = wf?.workflow.nodes.find((x) => x.id === id);
    return n ? n.label || nodeTitle(n.event) : id;
  };

  return (
    <section className="flex flex-col gap-4 px-6 py-6">
      <div className={`flex items-center gap-3.5 px-5 py-4 ${CARD}`}>
        <button type="button" className={BUTTON} onClick={onBack} aria-label={t("common.back")}>
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
        <div className="min-w-0 flex-1">
          <h1 className="truncate typo-title2_bold text-main-deep-blue">{wf?.name ?? t("history.deletedWorkflow")}</h1>
          {run && (
            <p className="typo-body2_regular mt-0.5 text-neutral-500">
              {when(run.startedAt)} → {when(run.finishedAt)}
            </p>
          )}
        </div>
        {run && <RunPill status={run.run.status} finishedAt={run.finishedAt} />}
      </div>
      {error && <p className="rounded-xl border border-danger-300 bg-danger-50 p-3 typo-body3_regular text-danger-700">{error}</p>}
      {!run ? (
        !error && <p className="typo-body3_regular text-neutral-500">{t("common.loading")}</p>
      ) : (
        <>
          {run.run.rejectReason && (
            <p className="rounded-xl border border-danger-300 bg-danger-100 px-4 py-3 typo-body3_semibold text-danger-700">
              {t(`reject.${run.run.rejectReason.kind}`)}
            </p>
          )}
          {run.run.validationErrors.length > 0 && (
            <section className={SECTION}>
              <h2 className="typo-body1_bold text-neutral-800">{t("history.validation")}</h2>
              <ul className="flex flex-col gap-1 typo-caption1_regular text-danger-700">
                {run.run.validationErrors.map((e, i) => (
                  <li key={i}>{e.message}</li>
                ))}
              </ul>
            </section>
          )}

          <section className={SECTION}>
            <div className="flex items-center gap-3">
              <h2 className="flex-1 typo-body1_bold text-neutral-800">{t("history.fileOps")}</h2>
              {run.fileOps.length > 0 && (
                <button type="button" className={PRIMARY} disabled={undoing || run.undo !== null} onClick={undo}>
                  <Undo2 size={16} aria-hidden="true" />
                  {run.undo ? t("history.undone") : t("history.undo")}
                </button>
              )}
            </div>
            {run.fileOps.length === 0 ? (
              <p className="typo-body3_regular text-neutral-500">{t("history.noFileOps")}</p>
            ) : (
              <ul className="flex flex-col gap-1.5">
                {run.fileOps.map((op, i) => (
                  <li key={i} className={ITEM}>
                    <span className="inline-flex w-fit items-center rounded-md border border-neutral-200 bg-white px-2 py-0.5 typo-caption1_semibold text-main-deep-blue">
                      {t(`op.${op.kind}`)}
                    </span>
                    <span className="flex flex-wrap items-center gap-1.5 break-all typo-caption1_regular text-neutral-700">
                      {op.from}
                      <ArrowRight size={12} className="shrink-0 text-neutral-400" aria-hidden="true" />
                      {op.to}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          {run.undo && (
            <div className={SECTION}>
              <UndoResult report={run.undo} />
            </div>
          )}

          <section className={SECTION}>
            <h2 className="typo-body1_bold text-neutral-800">{t("history.nodes")}</h2>
            <ul className="flex flex-col gap-1.5">
              {Object.entries(run.run.nodes).map(([id, rec]) => (
                <li key={id} className={ITEM}>
                  <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate typo-body3_semibold text-neutral-800">{nodeName(id)}</span>
                    <span
                      className={`inline-flex items-center rounded-full border px-2.5 py-1 typo-caption1_semibold ${
                        NODE_PILL[rec.status] ?? "border-neutral-300 bg-neutral-white text-neutral-600"
                      }`}
                    >
                      {t(`status.${rec.status}`)}
                    </span>
                  </div>
                  {rec.error && <p className="typo-caption1_regular text-danger-700">{rec.error}</p>}
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </section>
  );
}

type Filter = "all" | "success" | "error" | "running";
const FILTERS: Filter[] = ["all", "success", "error", "running"];
const DOT: Record<Filter, string> = { all: "", success: "bg-main-blue", error: "bg-danger-600", running: "" };

/** `initialRun`이면 그 실행 상세로 연다(대시보드·알림에서 들어올 때). */
export function History({ initialRun = null }: { initialRun?: string | null }) {
  const { runs, error, load } = useRuns();
  const names = useWorkflows((s) => s.list);
  const loadWorkflows = useWorkflows((s) => s.load);
  const [openId, setOpenId] = useState<string | null>(initialRun);
  const [filter, setFilter] = useState<Filter>("all");
  const [workflow, setWorkflow] = useState("");
  const [page, setPage] = useState(0);

  useEffect(() => {
    void load();
    void loadWorkflows();
  }, [load, loadWorkflows]);

  if (openId) return <RunDetail runId={openId} onBack={() => setOpenId(null)} />;

  const name = (id: string) => names.find((w) => w.id === id)?.name ?? t("history.deletedWorkflow");
  const byWorkflow = runs.filter((r) => !workflow || r.workflowId === workflow).map((r) => localRow(r, name(r.workflowId)));
  const count = (f: Filter) => (f === "all" ? byWorkflow.length : byWorkflow.filter((r) => match(r, f)).length);
  // 거부(rejected)는 빨간 알약이라 "오류"에, 중단(cancelled)은 "전체"에만.
  const match = (r: Row, f: Filter) => r.status === f || (f === "error" && r.status === "rejected");
  const rows = filter === "all" ? byWorkflow : byWorkflow.filter((r) => match(r, filter));
  const shown = rows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  return (
    <section className="flex flex-col gap-4 px-6 py-6">
      <div className={`flex items-center justify-between gap-4 px-5 py-4 ${CARD}`}>
        <div className="flex items-center gap-3.5">
          <div className="grid h-11 w-11 place-items-center rounded-xl bg-main-light-blue text-main-deep-blue">
            <HistoryIcon className="size-5" aria-hidden="true" />
          </div>
          <div>
            <h1 className="typo-title2_bold text-main-deep-blue">{t("history.title")}</h1>
            <p className="typo-body2_regular mt-0.5 text-neutral-500">{t("history.subtitle")}</p>
          </div>
        </div>
        <span className="inline-flex items-center gap-1.5 rounded-full border border-main-light-blue bg-main-light-blue px-2.5 py-1 typo-caption1_semibold text-main-deep-blue">
          <Laptop className="size-3" aria-hidden="true" />
          {t("dashboard.source.local")}
        </span>
      </div>

      <div className={`flex items-center gap-2 p-3 ${CARD}`}>
        {FILTERS.map((f) => {
          const on = filter === f;
          return (
            <button
              key={f}
              type="button"
              aria-pressed={on}
              className={`inline-flex h-9 items-center gap-1.5 rounded-full border px-3.5 typo-body3_semibold ${
                on ? "border-main-deep-blue bg-main-deep-blue text-white" : "border-neutral-200 bg-white text-neutral-600 hover:bg-neutral-50"
              }`}
              onClick={() => {
                setFilter(f);
                setPage(0);
              }}
            >
              {f === "running" ? (
                <Loader2 className={`size-3.5 animate-spin ${on ? "" : "text-neutral-500"}`} aria-hidden="true" />
              ) : (
                DOT[f] && <span className={`h-1.5 w-1.5 rounded-full ${DOT[f]}`} />
              )}
              {f === "all" ? t("dashboard.tab.all") : f === "error" ? t("history.filterError") : t(`dashboard.run.${f}`)}
              <span className={`rounded-full px-1.5 typo-caption2_semibold tabular-nums ${on ? "bg-white/15 text-white" : "bg-neutral-100 text-neutral-500"}`}>
                {count(f)}
              </span>
            </button>
          );
        })}
        <div className="flex-1" />
        <label className="relative inline-flex h-10 min-w-[220px] items-center rounded-[10px] border border-neutral-200 bg-white typo-body2_medium text-neutral-700">
          <WorkflowIcon className="pointer-events-none absolute left-3 size-4 text-neutral-400" aria-hidden="true" />
          <select
            aria-label={t("history.allWorkflows")}
            className="h-full w-full appearance-none bg-transparent pr-9 pl-9 outline-none"
            value={workflow}
            onChange={(e) => {
              setWorkflow(e.target.value);
              setPage(0);
            }}
          >
            <option value="">{t("history.allWorkflows")}</option>
            {names.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
          <ChevronDown className="pointer-events-none absolute right-3 size-4 text-neutral-400" aria-hidden="true" />
        </label>
      </div>

      {error && <p className="typo-body3_regular text-danger-700">{error}</p>}
      <div className="overflow-hidden rounded-brand-md border border-neutral-200 bg-neutral-white shadow-sm">
        {runs.length === 0 ? (
          <p className="px-5 py-8 text-center typo-body2_regular text-neutral-500">{t("history.empty")}</p>
        ) : (
          <>
            {rows.length === 0 ? (
              <RunsNoMatch
                onClear={() => {
                  setFilter("all");
                  setWorkflow("");
                  setPage(0);
                }}
              />
            ) : (
              <>
                <RunTable rows={shown} showSource={false} history onOpen={(r) => setOpenId(r.id)} />
                <Pager page={page} total={rows.length} onPage={setPage} />
              </>
            )}
          </>
        )}
      </div>
    </section>
  );
}
