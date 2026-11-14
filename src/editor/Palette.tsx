import { Laptop } from "lucide-react";
import { t } from "../i18n";
import { DEFS, isLocal, PALETTE } from "../nodes/defs";
import { NodeIcon, nodeMeta, toneStyle } from "../nodes/meta";
import { LocalBadge, nodeTitle } from "./Canvas";

const GROUPS = [
  { title: () => t("editor.kind.TRIGGER"), kinds: ["TRIGGER"] },
  { title: () => t("editor.local"), kinds: ["ACTION"] },
  { title: () => t("editor.group.ai"), kinds: ["AI"] },
  { title: () => t("editor.group.flow"), kinds: ["CONDITION"] },
];

/** 노드 6개를 종류별로 — 누르면 캔버스에 추가. */
export function Palette({ onAdd }: { onAdd: (event: string) => void }) {
  return (
    <div className="flex h-full flex-col gap-5">
      <div>
        <h2 className="typo-body2_bold text-main-deep-blue">{t("editor.palette")}</h2>
        <p className="mt-0.5 typo-caption1_regular text-neutral-500">{t("editor.paletteHint")}</p>
      </div>
      {GROUPS.map((g) => {
        const events = PALETTE.filter((e) => g.kinds.includes(DEFS[e].node_type));
        if (!events.length) return null;
        return (
          <section key={g.kinds[0]}>
            <h3 className="mb-2 px-1 text-[11px] font-semibold uppercase tracking-[.08em] text-main-gray">{g.title()}</h3>
            <div className="flex flex-col gap-1.5">
              {events.map((event) => (
                <button
                  key={event}
                  type="button"
                  onClick={() => onAdd(event)}
                  style={toneStyle(nodeMeta(event, undefined))}
                  className="flex items-center gap-2.5 rounded-xl border border-neutral-200 bg-white p-2.5 text-left shadow-[0_2px_6px_rgba(43,72,86,0.05)] transition-colors hover:border-(--node-color)"
                >
                  <span className="grid size-8 shrink-0 place-items-center rounded-[0.65rem] bg-(--node-color) text-white">
                    <NodeIcon event={event} size={16} />
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col items-start">
                    <span className="max-w-full truncate typo-body3_semibold text-neutral-800">{nodeTitle(event)}</span>
                    {isLocal(event) && (
                      <span className="mt-1">
                        <LocalBadge />
                      </span>
                    )}
                  </span>
                </button>
              ))}
            </div>
          </section>
        );
      })}
      <p className="mt-auto flex items-center gap-1.5 rounded-xl bg-main-light-blue px-3 py-2.5 typo-caption1_medium text-main-deep-blue">
        <Laptop className="size-3.5 shrink-0" aria-hidden="true" />
        {t("editor.localHint")}
      </p>
    </div>
  );
}
