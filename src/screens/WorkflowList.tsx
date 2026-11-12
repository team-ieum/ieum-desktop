import { Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { t } from "../i18n";
import { ListPage } from "../list/ListPage";
import { localCard, WorkflowCard } from "../list/WorkflowCard";
import { useWorkflows } from "../stores/workflows";
import { BUTTON } from "./Editor";

export function WorkflowList({ onOpen }: { onOpen: (id: string | null) => void }) {
  const { list, loaded, error, load, remove } = useWorkflows();
  const [confirming, setConfirming] = useState<string | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  const msg = error ?? removeError;
  return (
    <ListPage
      title={t("list.title")}
      subtitle={t("list.subtitle")}
      actions={
        <button
          type="button"
          onClick={() => onOpen(null)}
          className="inline-flex h-10 items-center gap-2 rounded-[10px] bg-main-deep-blue px-4 typo-body2_semibold text-white shadow-[0_8px_18px_-10px_rgba(41,83,124,.7)] hover:bg-main-deep-blue/90"
        >
          <Plus className="size-4" aria-hidden="true" />
          <span>{t("list.new")}</span>
        </button>
      }
      banner={msg && <p className="typo-body3_regular text-danger-700">{msg}</p>}
      cards={loaded ? list.map(localCard) : null}
      empty={t("list.empty")}
      renderCard={(c) => (
        <WorkflowCard key={c.id} data={c} onOpen={() => onOpen(c.id)} menu={[{ label: t("common.delete"), danger: true, onSelect: () => setConfirming(c.id) }]}>
          {confirming === c.id && (
            <div className="flex items-center justify-end gap-2">
              <button type="button" className={BUTTON} onClick={() => setConfirming(null)}>
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className={`${BUTTON} border-danger-300 text-danger-700`}
                onClick={async () => {
                  setConfirming(null);
                  try {
                    await remove(c.id);
                    setRemoveError(null);
                  } catch (e) {
                    setRemoveError(String(e));
                  }
                }}
              >
                {t("common.confirmDelete")}
              </button>
            </div>
          )}
        </WorkflowCard>
      )}
    />
  );
}
