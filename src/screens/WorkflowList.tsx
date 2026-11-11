import { Eye, Pencil, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import type { WorkflowEntry } from "../api";
import { t } from "../i18n";
import { useWorkflows } from "../stores/workflows";
import { BUTTON, PRIMARY } from "./Editor";

const WATCH_PILL: Record<WorkflowEntry["watch"], string> = {
  off: "bg-neutral-white text-neutral-600 border-neutral-300",
  watching: "bg-main-light-blue text-main-deep-blue border-main-light-blue",
  watch_error: "bg-danger-100 text-danger-700 border-danger-300",
};

export function WorkflowList({ onOpen, onOpenRuns }: { onOpen: (id: string | null) => void; onOpenRuns: (id: string) => void }) {
  const { list, loaded, error, load, remove } = useWorkflows();
  const [confirming, setConfirming] = useState<string | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
      <header className="flex items-center justify-between">
        <h1 className="typo-title2_bold text-neutral-800">{t("list.title")}</h1>
        <button type="button" className={PRIMARY} onClick={() => onOpen(null)}>
          <Plus size={16} aria-hidden="true" />
          {t("list.new")}
        </button>
      </header>
      {(error || removeError) && <p className="typo-body3_regular text-danger-700">{error ?? removeError}</p>}
      {!loaded ? (
        <p className="typo-body3_regular text-neutral-500">{t("common.loading")}</p>
      ) : list.length === 0 ? (
        <p className="rounded-brand-md border border-dashed border-neutral-300 bg-white p-8 text-center typo-body2_regular text-neutral-500">
          {t("list.empty")}
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {list.map((w) => (
            <li key={w.id} className="flex items-center gap-3 rounded-brand-md border border-neutral-200 bg-white px-4 py-3">
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <span className="truncate typo-body1_semibold text-neutral-800">{w.name}</span>
                <span className="flex flex-wrap items-center gap-2 typo-caption1_regular text-neutral-500">
                  <span className={`inline-flex items-center rounded-full border px-2.5 py-1 typo-caption1_semibold ${WATCH_PILL[w.watch]}`} title={w.watchMessage ?? undefined}>
                    {w.enabled ? t(`list.watch.${w.watch}`) : t("list.disabled")}
                  </span>
                  {w.updatedAt && (
                    <span>
                      {t("list.updated")} {new Date(w.updatedAt).toLocaleString()}
                    </span>
                  )}
                </span>
                {w.watch === "watch_error" && w.watchMessage && <span className="typo-caption1_regular text-danger-700">{w.watchMessage}</span>}
              </div>
              <button type="button" className={BUTTON} onClick={() => onOpenRuns(w.id)}>
                <Eye size={14} aria-hidden="true" />
                {t("list.runView")}
              </button>
              <button type="button" className={BUTTON} onClick={() => onOpen(w.id)}>
                <Pencil size={14} aria-hidden="true" />
                {t("list.open")}
              </button>
              {confirming === w.id ? (
                <>
                  <button
                    type="button"
                    className={`${BUTTON} border-danger-300 text-danger-700`}
                    onClick={async () => {
                      setConfirming(null);
                      try {
                        await remove(w.id);
                        setRemoveError(null);
                      } catch (e) {
                        setRemoveError(String(e));
                      }
                    }}
                  >
                    {t("common.confirmDelete")}
                  </button>
                  <button type="button" className={BUTTON} onClick={() => setConfirming(null)}>
                    {t("common.cancel")}
                  </button>
                </>
              ) : (
                <button type="button" className={BUTTON} onClick={() => setConfirming(w.id)} aria-label={t("common.delete")} title={t("common.delete")}>
                  <Trash2 size={14} aria-hidden="true" />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
