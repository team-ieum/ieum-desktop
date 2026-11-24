import { Bot, ChevronDown, ChevronRight, GitBranch, Globe2, Laptop, Plus, Search, Shuffle, X, Zap } from "lucide-react";
import { type ComponentType, type KeyboardEvent, type MouseEvent, useEffect, useId, useRef, useState } from "react";
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

/** 줄 선택 상태 — 키·마우스로 고른 줄과 그 줄을 추가하는 법. */
type Pick = { cur: string | null; hover: (event: string) => void; id: (event: string) => string; add: (event: string) => void };
type Hit = { event: string; cat: CategoryId; trigger: boolean };

/** 제목 + 설명에 검색어가 들어간 노드를 분류표 순서로 — 접기 규칙은 적용하지 않는다. */
const searchNodes = (needle: string): Hit[] =>
  CATEGORIES.flatMap((c) => c.sections.flatMap((s) => s.events.map((event) => ({ event, cat: c.id, trigger: isTriggerSection(s) })))).filter(
    ({ event }) => `${nodeTitle(event)} ${tDyn(`node.${event}.desc`) ?? ""}`.toLowerCase().includes(needle),
  );

// WKWebView에선 버튼을 눌러도 포커스가 버튼으로 가지 않고 body로 빠진다 — 검색 칸에 붙잡아 둬야 ↑↓·Esc가 계속 먹는다.
// 검색 칸 자체는 막지 않는다(커서 옮기기).
const keepFocus = (e: MouseEvent) => {
  if ((e.target as Element).closest("button")) e.preventDefault();
};

/** 왼쪽에 검색 칸과 카테고리 목록, 고르면 오른쪽 옆 창에 그 노드들(검색어가 있으면 결과) — 누르면 캔버스에 추가. */
export function Palette({ onAdd, onClose, hasTrigger }: { onAdd: (event: string) => void; onClose: () => void; hasTrigger: boolean }) {
  const [open, setOpen] = useState<CategoryId | null>(null);
  const [q, setQ] = useState("");
  const [active, setActive] = useState<string | null>(null);
  const [unfolded, setUnfolded] = useState(false);
  const uid = useId();
  const flyoutId = `${uid}flyout`;
  const listId = `${uid}list`;
  const needle = q.trim().toLowerCase();
  const results = needle ? searchNodes(needle) : null;
  const cat = results ? undefined : CATEGORIES.find((c) => c.id === open);
  // 캔버스에 시작 조건이 이미 있으면 앱 안 트리거는 맨 아래 한 줄로 접는다('start'엔 trigger 소제목이 없어 접히지 않는다).
  const fold = cat && hasTrigger ? cat.sections.find(isTriggerSection) : undefined;
  // 지금 화면에 보이는 줄 — 옆 창은 그려지는 순서대로(접힌 트리거는 펼쳤을 때만 맨 끝).
  const visible = results
    ? results.map((h) => h.event)
    : cat
      ? [...cat.sections.filter((s) => s !== fold), ...(fold && unfolded ? [fold] : [])].flatMap((s) => s.events)
      : [];
  // 접기 등으로 사라진 줄은 선택으로 치지 않는다. 검색 결과면 첫 줄이 처음부터 선택돼 있다.
  const cur = active && visible.includes(active) ? active : results ? (visible[0] ?? null) : null;
  const listShown = !!cat || !!results?.length;
  const pick: Pick = { cur, hover: setActive, id: (e) => `${uid}opt-${e}`, add: onAdd };

  const show = (id: CategoryId | null) => {
    setOpen(id);
    setActive(null);
    setUnfolded(false);
  };
  const search = (v: string) => {
    setQ(v);
    setActive(null);
  };
  const move = (d: 1 | -1) => {
    if (!visible.length) return;
    const i = cur ? visible.indexOf(cur) : -1;
    setActive(visible[i < 0 ? 0 : Math.min(Math.max(i + d, 0), visible.length - 1)]);
  };
  const onKey = (e: KeyboardEvent) => {
    // 한글 조합 중의 Enter·Esc는 조합을 끝내는 키다.
    if (e.nativeEvent.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      move(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      if (cur) onAdd(cur);
    } else if (e.key === "Escape") {
      // 한 단계씩만 닫는다 — 바깥(document)의 Esc 처리(SidePanel 등)까지 가지 않게.
      e.stopPropagation();
      if (q) search("");
      else if (open) show(null);
      else onClose();
    }
  };

  return (
    <div onMouseDown={keepFocus} className="flex items-start gap-2">
      <div className={`${SURFACE} flex w-58 shrink-0 flex-col overflow-y-auto p-2`}>
        <h2 className="px-1.5 pt-1.5 pb-2 text-sm font-semibold text-neutral-800">{t("editor.palette")}</h2>
        <div className="relative flex items-center">
          <Search size={14} aria-hidden="true" className="pointer-events-none absolute left-2.5 text-neutral-400" />
          <input
            autoFocus
            role="combobox"
            aria-label={t("palette.search")}
            aria-expanded={listShown}
            aria-controls={listShown ? listId : undefined}
            aria-autocomplete="list"
            aria-activedescendant={cur ? pick.id(cur) : undefined}
            placeholder={t("palette.search")}
            value={q}
            onChange={(e) => search(e.target.value)}
            onKeyDown={onKey}
            className="h-8 w-full rounded-lg border border-neutral-200 bg-neutral-50 pr-8 pl-8 text-[13px] text-neutral-800 outline-none placeholder:text-neutral-400 focus:border-main-blue focus:bg-white"
          />
          {q ? (
            <button
              type="button"
              aria-label={t("palette.search.clear")}
              onClick={() => search("")}
              className="absolute right-1.5 grid size-5 cursor-pointer place-items-center rounded text-neutral-400 hover:text-neutral-700"
            >
              <X size={14} aria-hidden="true" />
            </button>
          ) : (
            <kbd aria-hidden="true" className="pointer-events-none absolute right-2 rounded border border-neutral-200 bg-white px-1.5 font-sans text-[11px] leading-4 text-neutral-400">
              /
            </kbd>
          )}
        </div>
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
                  onClick={() => {
                    // 노드가 하나뿐이면 옆 창 없이 바로 추가한다.
                    if (single) return onAdd(events[0]);
                    // 검색 중이면 접지 말고 그 옆 창으로 — 검색어는 비운다.
                    show(results || open !== c.id ? c.id : null);
                    search("");
                  }}
                  aria-expanded={single ? undefined : !!cat && open === c.id}
                  aria-controls={single || !cat || open !== c.id ? undefined : flyoutId}
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
        <p aria-hidden="true" className="mt-2 flex gap-1.5 border-t border-neutral-100 px-1.5 pt-2 pb-0.5 text-[11px] text-neutral-400">
          {(
            [
              ["↑↓", "move"],
              ["↵", "add"],
              ["esc", "close"],
            ] as const
          ).map(([k, w], i) => (
            <span key={w}>
              {i > 0 && "· "}
              <kbd className="font-sans font-medium text-neutral-500">{k}</kbd> {t(`palette.keys.${w}`)}
            </span>
          ))}
        </p>
      </div>
      {results && <Results id={flyoutId} listId={listId} q={q.trim()} hits={results} pick={pick} />}
      {cat && (
        <Flyout key={cat.id} id={flyoutId} listId={listId} cat={cat} fold={fold} unfolded={unfolded} setUnfolded={setUnfolded} pick={pick} onJump={show} />
      )}
    </div>
  );
}

function Results({ id, listId, q, hits, pick }: { id: string; listId: string; q: string; hits: Hit[]; pick: Pick }) {
  const head = t("palette.search.results").replace("{q}", () => q);
  return (
    <section id={id} aria-label={head} className={`${SURFACE} flex w-80 shrink-0 flex-col overflow-hidden`}>
      <div className="border-b border-neutral-100 px-3.5 pt-3.5 pb-3">
        <h3 className="truncate text-sm font-semibold text-neutral-800">{head}</h3>
        <p className="mt-px text-xs text-neutral-500">
          {hits.length ? t("palette.search.count").replace("{n}", String(hits.length)) : t("palette.search.none")}
        </p>
      </div>
      {hits.length ? (
        <div className="overflow-x-hidden overflow-y-auto overscroll-contain px-1.5 py-1.5">
          <div role="listbox" id={listId} aria-label={head}>
            {hits.map((h) => (
              <Row key={h.event} event={h.event} trigger={h.trigger} cat={h.cat} pick={pick} />
            ))}
          </div>
        </div>
      ) : (
        <p className="px-3.5 py-6 text-center text-xs text-neutral-500">{t("palette.search.noneHint")}</p>
      )}
    </section>
  );
}

function Flyout({
  id,
  listId,
  cat,
  fold,
  unfolded,
  setUnfolded,
  pick,
  onJump,
}: {
  id: string;
  listId: string;
  cat: Category;
  fold: Section | undefined;
  unfolded: boolean;
  setUnfolded: (u: boolean) => void;
  pick: Pick;
  onJump: (id: CategoryId) => void;
}) {
  const shown = cat.sections.filter((s) => s !== fold);
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
    if (head) scroller.current?.scrollTo?.({ top: Math.max(0, head.offsetTop - 4), behavior: "smooth" });
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
        <div role="listbox" id={listId} aria-label={catName(cat.id)}>
          {/* 펼친 트리거도 listbox 안 맨 끝에 — 접기 버튼은 option이 아니라 listbox 밖(아래)에 둔다. */}
          {[...shown, ...(fold && unfolded ? [fold] : [])].map((s) => (
            <div key={s.id} role="group" aria-label={secName(s)}>
              {s.id !== "main" && <SectionHead s={s} />}
              {s.events.map((e) => (
                <Row key={e} event={e} trigger={isTriggerSection(s)} pick={pick} />
              ))}
            </div>
          ))}
        </div>
        {fold && (
          <button
            type="button"
            // 펼치면 칩이 위의 소제목 머리로 스크롤하도록 표시를 넘긴다.
            data-sec={unfolded ? undefined : fold.id}
            aria-expanded={unfolded}
            onClick={() => setUnfolded(!unfolded)}
            className="group mx-1.5 mt-3 mb-1 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-lg border border-dashed border-neutral-200 px-2.5 py-2 text-left text-xs text-neutral-500 hover:bg-neutral-50 hover:text-neutral-700"
          >
            <b className={`inline-flex shrink-0 items-center gap-1 font-semibold ${TRIGGER_GREEN}`}>
              <Zap size={12} aria-hidden="true" />
              {secName(fold)} {fold.events.length}
            </b>{" "}
            {t("palette.trigger.folded")}
            <ChevronDown size={14} aria-hidden="true" className="ml-auto shrink-0 transition-transform group-aria-expanded:rotate-180" />
          </button>
        )}
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

/** 노드 한 줄 — 앱 안 트리거면 아이콘에 번개 배지. 검색 결과(`cat`)면 카테고리·트리거 칩. */
function Row({ event, trigger, cat, pick }: { event: string; trigger: boolean; cat?: CategoryId; pick: Pick }) {
  const m = nodeMeta(event, undefined);
  const desc = tDyn(`node.${event}.desc`);
  const selected = pick.cur === event;
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    // 키로 고른 줄이 스크롤 밖이면 보이게. jsdom엔 scrollIntoView가 없다.
    if (selected) ref.current?.scrollIntoView?.({ block: "nearest" });
  }, [selected]);
  return (
    <button
      ref={ref}
      type="button"
      role="option"
      id={pick.id(event)}
      aria-selected={selected}
      // 포커스는 검색 칸에 두고 aria-activedescendant로 가리킨다 — Tab으로 줄마다 서지 않게.
      tabIndex={-1}
      data-event={event}
      title={desc}
      onClick={() => pick.add(event)}
      onMouseEnter={() => pick.hover(event)}
      style={toneStyle(m)}
      className="group flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-left active:bg-neutral-200 aria-selected:bg-neutral-100"
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
          {cat && (
            <span className="shrink-0 rounded-full bg-neutral-100 px-1.5 text-[11px] font-medium text-neutral-500">{catName(cat)}</span>
          )}
          {cat && trigger && (
            <span className={`shrink-0 rounded-full bg-[#2e8b68]/10 px-1.5 text-[11px] font-medium ${TRIGGER_GREEN}`}>{t("palette.sec.trigger")}</span>
          )}
        </span>
        {desc && <span className="mt-px block truncate text-xs text-neutral-500">{desc}</span>}
      </span>
      <Plus size={16} aria-hidden="true" className="shrink-0 text-main-deep-blue opacity-0 transition-opacity group-aria-selected:opacity-100" />
    </button>
  );
}
