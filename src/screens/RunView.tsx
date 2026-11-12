import { ArrowLeft, FileUp, Pencil } from "lucide-react";
import { useEffect } from "react";
import { Canvas, nodeTitle, StatusIcon } from "../editor/Canvas";
import { useTestRun } from "../editor/useTestRun";
import { t } from "../i18n";
import { emptyLive, useRuns } from "../stores/runs";
import { useWorkflows } from "../stores/workflows";
import { BUTTON, PRIMARY } from "./Editor";

/** `exec-event` 생중계 — 구독은 App이 한 번 건다(`subscribeExecEvents`). */
export function RunView({ workflowId, onBack, onEdit }: { workflowId: string; onBack: () => void; onEdit: (id: string) => void }) {
  const entry = useWorkflows((s) => s.list.find((w) => w.id === workflowId));
  const load = useWorkflows((s) => s.load);
  const live = useRuns((s) => s.live[workflowId]) ?? emptyLive(workflowId);
  const { running, error, run: testRun } = useTestRun(workflowId);

  useEffect(() => {
    if (!entry) void load();
  }, [entry, load]);

  const nodes = entry?.workflow.nodes ?? [];
  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-neutral-200 bg-white px-4 py-3">
        <button type="button" className={BUTTON} onClick={onBack} aria-label={t("common.back")}>
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
        <h1 className="min-w-0 flex-1 truncate typo-title3_bold text-neutral-800">
          {t("run.title")} · {entry?.name}
        </h1>
        {live.status && (
          <span className="inline-flex items-center gap-1.5 rounded-full border border-neutral-300 bg-white px-2.5 py-1 typo-caption1_semibold text-neutral-700">
            {t(`status.${live.status}`)}
          </span>
        )}
        <button type="button" className={BUTTON} onClick={() => onEdit(workflowId)}>
          <Pencil size={16} aria-hidden="true" />
          {t("list.open")}
        </button>
        <button type="button" className={PRIMARY} disabled={running || !entry} onClick={testRun}>
          <FileUp size={16} aria-hidden="true" />
          {t("run.testWithFile")}
        </button>
      </header>
      {error && <p className="border-b border-danger-300 bg-danger-100 px-4 py-2 typo-caption1_semibold text-danger-700">{error}</p>}
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">{entry && <Canvas workflow={entry.workflow} readOnly statuses={live.nodes} />}</div>
        <aside className="w-96 shrink-0 overflow-y-auto border-l border-neutral-200 bg-white p-4">
          {!live.runId ? (
            <p className="typo-body3_regular text-neutral-500">{t("run.idle")}</p>
          ) : (
            <ul className="flex flex-col gap-3">
              {nodes.map((n) => (
                <li key={n.id} className="flex flex-col gap-1.5 rounded-brand-sm border border-neutral-200 p-3">
                  <div className="flex items-center gap-2">
                    <StatusIcon status={live.nodes[n.id]} />
                    <span className="min-w-0 flex-1 truncate typo-body3_semibold text-neutral-800">{n.label || nodeTitle(n.event)}</span>
                  </div>
                  {live.errors[n.id] && <p className="typo-caption1_regular text-danger-700">{live.errors[n.id]}</p>}
                  {live.chunks[n.id] && (
                    <div className="flex flex-col gap-1">
                      <span className="typo-caption2_semibold text-neutral-500">{t("run.aiOutput")}</span>
                      <pre className="max-h-64 overflow-y-auto whitespace-pre-wrap rounded-brand-sm bg-neutral-100 p-2 typo-caption1_regular text-neutral-700">
                        {live.chunks[n.id]}
                      </pre>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </aside>
      </div>
    </div>
  );
}
