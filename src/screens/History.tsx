import { ArrowLeft, ArrowRight, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type RunStatus, type StoredRun, type UndoReport } from "../api";
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
    <div className="mx-auto flex max-w-4xl flex-col gap-5 p-6">
      <header className="flex items-center gap-3">
        <button type="button" className={BUTTON} onClick={onBack} aria-label={t("common.back")}>
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
        <h1 className="min-w-0 flex-1 truncate typo-title3_bold text-neutral-800">{wf?.name ?? t("history.deletedWorkflow")}</h1>
        {run && <RunPill status={run.run.status} finishedAt={run.finishedAt} />}
      </header>
      {error && <p className="typo-body3_regular text-danger-700">{error}</p>}
      {!run ? (
        !error && <p className="typo-body3_regular text-neutral-500">{t("common.loading")}</p>
      ) : (
        <>
          <p className="typo-caption1_regular text-neutral-500">
            {when(run.startedAt)} → {when(run.finishedAt)}
          </p>
          {run.run.rejectReason && (
            <p className="rounded-brand-sm bg-danger-100 p-3 typo-body3_semibold text-danger-700">{t(`reject.${run.run.rejectReason.kind}`)}</p>
          )}
          {run.run.validationErrors.length > 0 && (
            <section className="flex flex-col gap-2">
              <h2 className="typo-body1_bold text-neutral-800">{t("history.validation")}</h2>
              <ul className="flex flex-col gap-1 typo-caption1_regular text-danger-700">
                {run.run.validationErrors.map((e, i) => (
                  <li key={i}>{e.message}</li>
                ))}
              </ul>
            </section>
          )}

          <section className="flex flex-col gap-2">
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
                  <li key={i} className="flex flex-col gap-1 rounded-brand-sm border border-neutral-200 bg-white p-3">
                    <span className="typo-caption1_semibold text-main-deep-blue">{t(`op.${op.kind}`)}</span>
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

          {run.undo && <UndoResult report={run.undo} />}

          <section className="flex flex-col gap-2">
            <h2 className="typo-body1_bold text-neutral-800">{t("history.nodes")}</h2>
            <ul className="flex flex-col gap-1.5">
              {Object.entries(run.run.nodes).map(([id, rec]) => (
                <li key={id} className="flex flex-col gap-1 rounded-brand-sm border border-neutral-200 bg-white p-3">
                  <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate typo-body3_semibold text-neutral-800">{nodeName(id)}</span>
                    <span className="typo-caption1_semibold text-neutral-600">{t(`status.${rec.status}`)}</span>
                  </div>
                  {rec.error && <p className="typo-caption1_regular text-danger-700">{rec.error}</p>}
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </div>
  );
}

/** `initialRun`이면 그 실행 상세로 연다(대시보드에서 들어올 때). */
export function History({ initialRun = null }: { initialRun?: string | null }) {
  const { runs, error, load } = useRuns();
  const names = useWorkflows((s) => s.list);
  const loadWorkflows = useWorkflows((s) => s.load);
  const [openId, setOpenId] = useState<string | null>(initialRun);

  useEffect(() => {
    void load();
    void loadWorkflows();
  }, [load, loadWorkflows]);

  if (openId) return <RunDetail runId={openId} onBack={() => setOpenId(null)} />;
  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
      <h1 className="typo-title2_bold text-neutral-800">{t("history.title")}</h1>
      {error && <p className="typo-body3_regular text-danger-700">{error}</p>}
      {runs.length === 0 ? (
        <p className="rounded-brand-md border border-dashed border-neutral-300 bg-white p-8 text-center typo-body2_regular text-neutral-500">
          {t("history.empty")}
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {runs.map((r) => (
            <li key={r.runId}>
              <button
                type="button"
                className="flex w-full items-center gap-3 rounded-brand-md border border-neutral-200 bg-white px-4 py-3 text-left hover:border-main-blue"
                onClick={() => setOpenId(r.runId)}
              >
                <RunPill status={r.status} finishedAt={r.finishedAt} />
                <span className="min-w-0 flex-1 truncate typo-body2_semibold text-neutral-800">
                  {names.find((w) => w.id === r.workflowId)?.name ?? t("history.deletedWorkflow")}
                </span>
                {r.fileOps > 0 && (
                  <span className="typo-caption1_regular text-neutral-500">
                    {t("history.fileOps")} {r.fileOps}
                  </span>
                )}
                {r.undone && <span className="typo-caption1_semibold text-main-deep-blue">{t("history.undone")}</span>}
                <span className="typo-caption1_regular text-neutral-500">{when(r.startedAt)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
