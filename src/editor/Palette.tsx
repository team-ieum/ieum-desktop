import { ChevronRight, Laptop } from "lucide-react";
import { useState } from "react";
import { t } from "../i18n";
import { DEFS, isLocal, PALETTE } from "../nodes/defs";
import { NodeIcon, nodeMeta, toneStyle } from "../nodes/meta";
import { LocalBadge, nodeTitle } from "./Canvas";

const kind = (k: string) => (e: string) => DEFS[e].node_type === k;
// 포맷터는 모두 TRANSFORM이라 event 이름으로 나눈다(이음 클라우드 편집기와 같은 순서·이름).
const fmt = (...prefixes: string[]) => (e: string) => prefixes.some((p) => e.startsWith(`formatter.${p}`));

const GROUPS = [
  { id: "trigger", title: () => t("editor.kind.TRIGGER"), has: kind("TRIGGER") },
  { id: "local", title: () => t("editor.local"), has: (e: string) => kind("ACTION")(e) && isLocal(e) },
  { id: "github", title: () => t("palette.group.github"), has: (e: string) => e.startsWith("github.") },
  { id: "ai", title: () => t("editor.group.ai"), has: kind("AI") },
  { id: "flow", title: () => t("editor.group.flow"), has: kind("CONDITION") },
  { id: "transform", title: () => t("palette.group.transform"), has: (e: string) => e === "transform.map" },
  { id: "text", title: () => t("palette.group.text"), has: fmt("text_") },
  { id: "number", title: () => t("palette.group.number"), has: fmt("number_") },
  { id: "date", title: () => t("palette.group.date"), has: fmt("date_") },
  { id: "list", title: () => t("palette.group.list"), has: fmt("list_", "lookup_") },
  { id: "http", title: () => t("palette.group.http"), has: kind("HTTP") },
];

// 팔레트는 노드를 추가할 때마다 닫히며 언마운트되므로 펼친 묶음은 모듈에 둔다.
let openGroups = new Set<string>();

/** 노드를 종류별로 — 누르면 캔버스에 추가. */
export function Palette({ onAdd }: { onAdd: (event: string) => void }) {
  const [open, setOpen] = useState(openGroups);
  const toggle = (id: string) => {
    const next = new Set(open);
    if (!next.delete(id)) next.add(id);
    openGroups = next;
    setOpen(next);
  };
  return (
    <div className="flex h-full flex-col gap-5">
      <div>
        <h2 className="typo-body2_bold text-main-deep-blue">{t("editor.palette")}</h2>
        <p className="mt-0.5 typo-caption1_regular text-neutral-500">{t("editor.paletteHint")}</p>
      </div>
      <div className="flex flex-col gap-1">
      {GROUPS.map((g) => {
        const events = PALETTE.filter(g.has);
        if (!events.length) return null;
        const isOpen = open.has(g.id);
        return (
          <section key={g.id}>
            <h3>
              <button
                type="button"
                id={`palette-${g.id}`}
                aria-expanded={isOpen}
                aria-controls={`palette-${g.id}-list`}
                onClick={() => toggle(g.id)}
                className="flex w-full items-center gap-1.5 rounded-lg px-1 py-1.5 text-left text-[11px] font-semibold uppercase tracking-[.08em] text-main-gray hover:bg-neutral-50"
              >
                <ChevronRight className={`size-3.5 shrink-0 transition-transform ${isOpen ? "rotate-90" : ""}`} aria-hidden="true" />
                <span data-title className="flex-1">{g.title()}</span>
                <span className="tabular-nums text-neutral-400">{events.length}</span>
              </button>
            </h3>
            {isOpen && (
            <div id={`palette-${g.id}-list`} role="region" aria-labelledby={`palette-${g.id}`} className="mt-1 mb-2 flex flex-col gap-1.5">
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
            )}
          </section>
        );
      })}
      </div>
      <p className="mt-auto flex items-center gap-1.5 rounded-xl bg-main-light-blue px-3 py-2.5 typo-caption1_medium text-main-deep-blue">
        <Laptop className="size-3.5 shrink-0" aria-hidden="true" />
        {t("editor.localHint")}
      </p>
    </div>
  );
}
