import { t } from "../i18n";
import { DEFS, isLocal, PALETTE } from "../nodes/defs";
import { LocalBadge, NodeIcon, nodeTitle, TONE } from "./Canvas";

/** 노드 6개 — 누르면 캔버스에 추가. */
export function Palette({ onAdd }: { onAdd: (event: string) => void }) {
  return (
    <div className="flex flex-col gap-2">
      <h2 className="typo-body2_bold text-neutral-800">{t("editor.palette")}</h2>
      <p className="typo-caption1_regular text-neutral-500">{t("editor.paletteHint")}</p>
      {PALETTE.map((event) => (
        <button
          key={event}
          type="button"
          onClick={() => onAdd(event)}
          className={`flex items-center gap-2 rounded-brand-sm border border-neutral-200 bg-white px-2.5 py-2 text-left hover:border-(--node-color) ${TONE[DEFS[event].node_type]}`}
        >
          <span className="grid size-7 shrink-0 place-items-center rounded-[0.6rem] bg-(--node-color) text-white">
            <NodeIcon event={event} size={16} />
          </span>
          <span className="min-w-0 flex-1 truncate typo-body3_semibold text-neutral-800">{nodeTitle(event)}</span>
          {isLocal(event) && <LocalBadge />}
        </button>
      ))}
    </div>
  );
}
