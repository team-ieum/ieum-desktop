import { Bot, ChevronDown, ChevronRight, GitBranch, Globe2, Laptop, Plus, Shuffle, Zap } from "lucide-react";
import { type ComponentType, Fragment, useId, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { t, tDyn } from "../i18n";
import { isLocal } from "../nodes/defs";
import { GithubMark, nodeMeta, toneStyle } from "../nodes/meta";
import { LocalBadge, nodeTitle } from "./Canvas";
import { type Category, type CategoryId, CATEGORIES, categoryEvents, isTriggerSection, type Section } from "./paletteGroups";

// 카테고리 아이콘과 색을 빌려 올 대표 노드 — 색은 캔버스 노드와 같은 nodeMeta에서 온다.
const LOOK: Record<CategoryId, [ComponentType<{ size?: number }>, string]> = {
  start: [Zap, "manual.run"],
  ai: [Bot, "local.ai"],
  flow: [GitBranch, "branch.if"],
  data: [Shuffle, "transform.map"],
  http: [Globe2, "http.request"],
  github: [GithubMark, "github.create_issue"],
  local: [Laptop, "local.file_move"],
};
const catTone = (id: CategoryId) => toneStyle(nodeMeta(LOOK[id][1], undefined));
const catName = (id: CategoryId) => t(`palette.cat.${id}`);
const secName = (s: Section) => tDyn(`palette.sec.${s.id}`) ?? "";
const SURFACE =
  "max-h-[min(40rem,80vh)] rounded-xl border border-neutral-200 bg-white shadow-[0_12px_32px_-8px_rgba(25,25,25,0.16),0_2px_6px_rgba(25,25,25,0.05)]";
const TRIGGER_GREEN = "text-[#2e8b68]";

/** 카테고리 색 아이콘 상자 — 부모가 `catTone`으로 색 변수를 건다. */
function CatBox({ id, big }: { id: CategoryId; big?: boolean }) {
  const Icon = LOOK[id][0];
  return (
    <span
      aria-hidden="true"
      className={`grid shrink-0 place-items-center bg-(--node-tint) text-(--node-color) ${id === "github" ? "border border-(--node-shadow)" : ""} ${big ? "size-8 rounded-lg" : "size-7 rounded-[7px]"}`}
    >
      <Icon size={big ? 16 : 15} />
    </span>
  );
}

/** 왼쪽에 카테고리 목록, 고르면 오른쪽 옆 창에 그 노드들 — 누르면 캔버스에 추가. */
export function Palette({ onAdd, hasTrigger }: { onAdd: (event: string) => void; hasTrigger: boolean }) {
  const [open, setOpen] = useState<CategoryId | null>(null);
  const flyoutId = useId();
  const cat = CATEGORIES.find((c) => c.id === open);
  return (
    <div className="flex items-start gap-2">
      <div className={`${SURFACE} flex w-58 shrink-0 flex-col overflow-y-auto p-2`}>
        <h2 className="px-1.5 pt-1.5 pb-2 text-sm font-semibold text-neutral-800">{t("editor.palette")}</h2>
        {(["basic", "apps"] as const).map((zone) => (
          <div key={zone}>
            <p className="px-2 pt-2.5 pb-1 text-[11.5px] font-medium text-neutral-400">{t(`palette.zone.${zone}`)}</p>
            {CATEGORIES.filter((c) => c.zone === zone).map((c) => {
              const events = categoryEvents(c);
              const single = events.length === 1;
              return (
                <button
                  key={c.id}
                  type="button"
                  style={catTone(c.id)}
                  // 노드가 하나뿐이면 옆 창 없이 바로 추가한다.
                  onClick={() => (single ? onAdd(events[0]) : setOpen(open === c.id ? null : c.id))}
                  aria-expanded={single ? undefined : open === c.id}
                  aria-controls={single || open !== c.id ? undefined : flyoutId}
                  title={single ? tDyn(`node.${events[0]}.desc`) : undefined}
                  className="group relative flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-lg pr-2 pl-1.5 text-left text-[13.5px] font-medium text-neutral-700 hover:bg-neutral-100 aria-expanded:bg-neutral-100 aria-expanded:text-neutral-800 aria-expanded:before:absolute aria-expanded:before:inset-y-[9px] aria-expanded:before:-left-2 aria-expanded:before:w-[3px] aria-expanded:before:rounded-r-[3px] aria-expanded:before:bg-(--node-color)"
                >
                  <CatBox id={c.id} />
                  <span className="flex-1 truncate">{catName(c.id)}</span>
                  {single ? (
                    <span aria-hidden="true" className="flex items-center gap-0.5 text-[11.5px] text-neutral-400 group-hover:text-main-deep-blue">
                      <Plus size={14} />
                      {t("palette.add")}
                    </span>
                  ) : (
                    <>
                      <span aria-hidden="true" className="text-xs text-neutral-400 tabular-nums">
                        {events.length}
                      </span>
                      <ChevronRight size={14} aria-hidden="true" className="text-neutral-300 group-aria-expanded:text-neutral-600" />
                    </>
                  )}
                </button>
              );
            })}
          </div>
        ))}
      </div>
      {cat && <Flyout key={cat.id} id={flyoutId} cat={cat} hasTrigger={hasTrigger} onAdd={onAdd} onJump={setOpen} />}
    </div>
  );
}

function Flyout({
  id,
  cat,
  hasTrigger,
  onAdd,
  onJump,
}: {
  id: string;
  cat: Category;
  hasTrigger: boolean;
  onAdd: (event: string) => void;
  onJump: (id: CategoryId) => void;
}) {
  // 캔버스에 시작 조건이 이미 있으면 앱 안 트리거는 맨 아래 한 줄로 접는다('start'엔 trigger 소제목이 없어 접히지 않는다).
  const fold = hasTrigger ? cat.sections.find(isTriggerSection) : undefined;
  const shown = cat.sections.filter((s) => s !== fold);
  const [unfolded, setUnfolded] = useState(false);
  const [current, setCurrent] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const chipSecs = categoryEvents(cat).length >= 20 ? [...shown.filter((s) => s.id !== "main"), ...(fold ? [fold] : [])] : [];
  const headOf = (s: Section) => scroller.current?.querySelector<HTMLElement>(`[data-sec="${s.id}"]`);
  const desc = tDyn(`palette.cat.${cat.id}.desc`);

  const jump = (s: Section) => {
    // 접힌 트리거 칩은 먼저 펼쳐 둬야 그 아래까지 스크롤할 수 있다.
    if (s === fold && !unfolded) flushSync(() => setUnfolded(true));
    const head = headOf(s);
    // jsdom엔 scrollTo가 없다.
    if (head) scroller.current?.scrollTo?.({ top: head.offsetTop - 4, behavior: "smooth" });
  };
  // 스크롤 위치에 맞춰 칩 하나를 강조한다 — 소제목 머리가 위에 닿았거나, 끝까지 내렸으면 마지막 칩.
  const syncChip = () => {
    const box = scroller.current;
    if (!box || !chipSecs.length) return;
    let cur = 0;
    chipSecs.forEach((s, i) => {
      const head = headOf(s);
      if (head && head.offsetTop <= box.scrollTop + 8) cur = i;
    });
    if (box.scrollTop + box.clientHeight >= box.scrollHeight - 2) cur = chipSecs.length - 1;
    setCurrent(cur);
  };

  return (
    <section id={id} aria-label={catName(cat.id)} style={catTone(cat.id)} className={`${SURFACE} flex w-80 shrink-0 flex-col overflow-hidden`}>
      <div className="flex items-center gap-2.5 border-b border-neutral-100 px-3.5 pt-3.5 pb-3">
        <CatBox id={cat.id} big />
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-neutral-800">{catName(cat.id)}</h3>
          {desc && <p className="mt-px text-xs text-neutral-500">{desc}</p>}
        </div>
      </div>
      {chipSecs.length > 0 && (
        <nav aria-label={t("palette.chips")} className="flex flex-wrap gap-1.5 border-b border-neutral-100 px-3 py-2">
          {chipSecs.map((s, i) => (
            <button
              key={s.id}
              type="button"
              aria-current={i === current ? "true" : undefined}
              onClick={() => jump(s)}
              className={`group cursor-pointer rounded-full border border-neutral-200 bg-white px-2.5 py-[3px] text-xs font-medium hover:border-neutral-300 hover:text-neutral-800 aria-[current=true]:border-neutral-800 aria-[current=true]:bg-neutral-800 aria-[current=true]:text-white ${isTriggerSection(s) ? TRIGGER_GREEN : "text-neutral-600"}`}
            >
              {secName(s)} <span className="text-neutral-400 tabular-nums group-aria-[current=true]:text-white/60">{s.events.length}</span>
            </button>
          ))}
        </nav>
      )}
      <div ref={scroller} data-scroll onScroll={syncChip} className="relative overflow-x-hidden overflow-y-auto overscroll-contain px-1.5 pt-1 pb-2">
        <div role="listbox" aria-label={catName(cat.id)}>
          {shown.map((s) => (
            <Fragment key={s.id}>
              {s.id !== "main" && <SectionHead s={s} />}
              {s.events.map((e) => (
                <Row key={e} event={e} trigger={isTriggerSection(s)} onAdd={onAdd} />
              ))}
            </Fragment>
          ))}
          {fold && (
            <>
              <button
                type="button"
                data-sec={fold.id}
                aria-expanded={unfolded}
                onClick={() => setUnfolded((u) => !u)}
                className="group mx-1.5 mt-3 mb-1 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-lg border border-dashed border-neutral-200 px-2.5 py-2 text-left text-xs text-neutral-500 hover:bg-neutral-50 hover:text-neutral-700"
              >
                <b className={`inline-flex shrink-0 items-center gap-1 font-semibold ${TRIGGER_GREEN}`}>
                  <Zap size={12} aria-hidden="true" />
                  {secName(fold)} {fold.events.length}
                </b>
                {t("palette.trigger.folded")}
                <ChevronDown size={14} aria-hidden="true" className="ml-auto shrink-0 transition-transform group-aria-expanded:rotate-180" />
              </button>
              {unfolded && fold.events.map((e) => <Row key={e} event={e} trigger onAdd={onAdd} />)}
            </>
          )}
        </div>
        {cat.id === "start" && (
          <div role="note" className="mx-2 mt-2.5 mb-1 rounded-lg bg-neutral-50 p-2.5 text-xs leading-normal text-neutral-600">
            {t("palette.start.note")}
            <span className="mt-1.5 flex gap-3">
              {(["github", "local"] as const).map((to) => (
                <button
                  key={to}
                  type="button"
                  onClick={() => onJump(to)}
                  className="inline-flex cursor-pointer items-center gap-0.5 font-medium text-main-blue hover:underline"
                >
                  {catName(to)}
                  <ChevronRight size={12} aria-hidden="true" />
                </button>
              ))}
            </span>
          </div>
        )}
      </div>
    </section>
  );
}

function SectionHead({ s }: { s: Section }) {
  if (isTriggerSection(s))
    return (
      <div data-sec={s.id} className={`flex items-center gap-1.5 px-2 pt-3 pb-1 text-[11.5px] font-semibold ${TRIGGER_GREEN}`}>
        <Zap size={12} aria-hidden="true" className="shrink-0" />
        {secName(s)}
        <span className="truncate font-normal text-neutral-400">
          <span aria-hidden="true">· </span>
          {t("palette.trigger.hint")}
        </span>
      </div>
    );
  return (
    <div data-sec={s.id} className="flex items-center gap-1.5 px-2 pt-3 pb-1 text-[11.5px] font-semibold text-neutral-500">
      {secName(s)}
      {s.events.length > 1 && <span className="font-normal text-neutral-400 tabular-nums">{s.events.length}</span>}
    </div>
  );
}

/** 노드 한 줄 — 앱 안 트리거면 아이콘에 번개 배지. */
function Row({ event, trigger, onAdd }: { event: string; trigger: boolean; onAdd: (event: string) => void }) {
  const m = nodeMeta(event, undefined);
  const desc = tDyn(`node.${event}.desc`);
  return (
    <button
      type="button"
      role="option"
      data-event={event}
      title={desc}
      onClick={() => onAdd(event)}
      style={toneStyle(m)}
      className="group flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-neutral-100 active:bg-neutral-200"
    >
      <span
        aria-hidden="true"
        className={`relative grid size-8 shrink-0 place-items-center rounded-lg bg-(--node-tint) text-(--node-color) ${m.head ? "border border-(--node-shadow)" : ""}`}
      >
        {m.icon}
        {trigger && (
          <span className="absolute -right-1 -bottom-1 grid size-[15px] place-items-center rounded-full border-2 border-white bg-[#2e8b68] text-white">
            <Zap size={8} strokeWidth={3} />
          </span>
        )}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5 text-[13px] font-semibold text-neutral-800">
          <span className="truncate">{nodeTitle(event)}</span>
          {isLocal(event) && <LocalBadge />}
        </span>
        {desc && <span className="mt-px block truncate text-xs text-neutral-500">{desc}</span>}
      </span>
      <Plus size={16} aria-hidden="true" className="shrink-0 text-main-deep-blue opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
    </button>
  );
}
