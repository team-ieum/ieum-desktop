import { Check, ChevronDown, ListFilter, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { t } from "../i18n";

export type FilterGroup = {
  key: string;
  title: string;
  icon: LucideIcon;
  options: { value: string; label: string; count: number; mark: ReactNode }[];
};

/** 왼쪽 필터 — 묶음 안은 OR, 묶음끼리는 AND(거르는 쪽은 부르는 화면). 접기는 `<details>`. */
export function FilterPanel({ groups, selected, onToggle }: { groups: FilterGroup[]; selected: Record<string, Set<string>>; onToggle(group: string, value: string): void }) {
  return (
    <aside className="flex w-full flex-col overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-[0_1px_2px_rgba(16,24,40,.04)]">
      <div className="flex shrink-0 items-center gap-2 border-b border-neutral-100 px-4 py-3">
        <ListFilter className="size-[17px] text-main-deep-blue" aria-hidden="true" />
        <h2 className="typo-title3_semibold text-neutral-800">{t("list.filter")}</h2>
      </div>
      <div className="px-4 pb-2">
        {groups.map(({ key, title, icon: Icon, options }) => (
          <details key={key} open className="group border-b border-neutral-200 py-3 last:border-b-0">
            <summary className="flex w-full cursor-pointer list-none items-center gap-2 rounded-lg px-1 py-1 text-neutral-700 [&::-webkit-details-marker]:hidden">
              <Icon className="size-4 text-neutral-400" aria-hidden="true" />
              <span className="flex-1 typo-body3_semibold">{title}</span>
              <ChevronDown className="size-4 text-neutral-400 group-open:rotate-180" aria-hidden="true" />
            </summary>
            <div className="mt-2 flex flex-col gap-1">
              {options.map((o) => {
                const on = selected[key]?.has(o.value) ?? false;
                return (
                  <label key={o.value} className={`flex min-h-9 w-full cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 hover:bg-neutral-50 ${on ? "text-main-deep-blue" : "text-neutral-600"}`}>
                    <input type="checkbox" className="peer sr-only" checked={on} onChange={() => onToggle(key, o.value)} />
                    <span
                      className={`grid h-3.5 w-3.5 shrink-0 place-items-center rounded border peer-focus-visible:ring-2 peer-focus-visible:ring-main-blue ${
                        on ? "border-main-deep-blue bg-main-deep-blue text-white" : "border-neutral-300 bg-white"
                      }`}
                    >
                      {on && <Check className="size-2.5" strokeWidth={3} aria-hidden="true" />}
                    </span>
                    <span className="grid h-[18px] w-[18px] shrink-0 place-items-center">{o.mark}</span>
                    <span className="min-w-0 flex-1 truncate typo-body3_regular">{o.label}</span>
                    <span className="typo-caption1_regular text-neutral-400">{o.count}</span>
                  </label>
                );
              })}
            </div>
          </details>
        ))}
      </div>
    </aside>
  );
}
