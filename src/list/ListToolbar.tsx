import { ChevronDown, LayoutGrid, List, Search } from "lucide-react";
import { t } from "../i18n";

export type Sort = "updated" | "name";
export type ListView = "grid" | "list";

const VIEW_KEY = "ieum.listView";
/** 저장소를 못 쓰면 격자. */
export const savedView = (): ListView => {
  try {
    return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
};
export const saveView = (v: ListView) => {
  try {
    localStorage.setItem(VIEW_KEY, v);
  } catch {
    // 이번 실행에서만 유지.
  }
};

const VIEWS = [
  { v: "grid", icon: LayoutGrid, label: "list.view.grid" },
  { v: "list", icon: List, label: "list.view.list" },
] as const;

export function ListToolbar({
  query,
  onQuery,
  sort,
  onSort,
  view,
  onView,
  count,
}: {
  query: string;
  onQuery(q: string): void;
  sort: Sort;
  onSort(s: Sort): void;
  view: ListView;
  onView(v: ListView): void;
  count: number;
}) {
  return (
    <div className="relative flex flex-row items-center gap-3 rounded-xl border border-neutral-200 bg-white p-3 shadow-[0_1px_2px_rgba(16,24,40,.04)]">
      <label className="relative min-w-0 flex-1">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-[17px] -translate-y-1/2 text-neutral-400" aria-hidden="true" />
        <input
          type="search"
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          placeholder={t("list.search")}
          aria-label={t("list.search")}
          className="h-10 w-full rounded-[10px] border border-neutral-200 bg-neutral-50 pl-9 pr-3 typo-body2_regular text-neutral-700 outline-none placeholder:text-neutral-400 focus:border-main-blue"
        />
      </label>
      <span className="typo-caption1_regular text-neutral-500">{t("list.count").replace("{n}", String(count))}</span>
      <div className="ml-auto flex items-center gap-2">
        <label className="relative">
          <select
            value={sort}
            onChange={(e) => onSort(e.target.value as Sort)}
            aria-label={t("list.sort")}
            className="h-10 appearance-none rounded-[10px] border border-neutral-200 bg-white pl-3 pr-8 typo-body2_medium text-neutral-700 outline-none"
          >
            <option value="updated">{t("list.sort.updated")}</option>
            <option value="name">{t("list.sort.name")}</option>
          </select>
          <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 size-4 -translate-y-1/2 text-neutral-400" aria-hidden="true" />
        </label>
        <div className="inline-flex rounded-[10px] border border-neutral-200 bg-neutral-100 p-1">
          {VIEWS.map(({ v, icon: Icon, label }) => (
            <button
              key={v}
              type="button"
              aria-label={t(label)}
              title={t(label)}
              aria-pressed={view === v}
              onClick={() => onView(v)}
              className={`grid h-8 w-8 place-items-center rounded-lg ${view === v ? "bg-white text-main-deep-blue shadow-[0_1px_2px_rgba(16,24,40,.08)]" : "text-neutral-400"}`}
            >
              <Icon className="size-[17px]" aria-hidden="true" />
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
