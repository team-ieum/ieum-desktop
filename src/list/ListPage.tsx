import { Cable, Circle } from "lucide-react";
import { useState, type ReactNode } from "react";
import { t } from "../i18n";
import { FilterPanel, type FilterGroup } from "./FilterPanel";
import { ListToolbar, savedView, saveView, type ListView, type Sort } from "./ListToolbar";
import { ServiceIcon, serviceLabel } from "./ServiceIcons";
import { STATUS_META, type CardData, type CardStatus } from "./WorkflowCard";

const STATUSES: CardStatus[] = ["active", "paused", "error"];
const time = (iso: string | null) => (iso ? Date.parse(iso) || 0 : 0);

/** 내 PC·웹 이음 목록의 공통 틀 — 제목 카드, 필터, 툴바, 카드 격자. */
export function ListPage({
  title,
  subtitle,
  actions,
  banner,
  cards,
  empty,
  renderCard,
}: {
  title: string;
  subtitle: string;
  actions: ReactNode;
  /** 제목 아래 한 줄(오류 등). */
  banner?: ReactNode;
  /** null = 읽는 중. */
  cards: CardData[] | null;
  empty: string;
  renderCard(c: CardData): ReactNode;
}) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("updated");
  const [view, setView] = useState<ListView>(savedView);
  const [selected, setSelected] = useState<Record<string, Set<string>>>({});

  const all = cards ?? [];
  const services = [...new Set(all.flatMap((c) => c.services))];
  const groups: FilterGroup[] = [
    {
      key: "service",
      title: t("list.filter.services"),
      icon: Cable,
      options: services.map((s) => ({
        value: s,
        label: serviceLabel(s),
        count: all.filter((c) => c.services.includes(s)).length,
        mark: <ServiceIcon service={s} size="size-4" />,
      })),
    },
    {
      key: "status",
      title: t("list.filter.status"),
      icon: Circle,
      options: STATUSES.map((s) => ({
        value: s,
        label: t(`list.status.${s}`),
        count: all.filter((c) => c.status === s).length,
        mark: <span className={`inline-block h-2 w-2 rounded-full ${STATUS_META[s].dot}`} />,
      })),
    },
  ];

  const q = query.trim().toLowerCase();
  const pick = (key: string, ok: (v: string) => boolean) => !selected[key]?.size || [...selected[key]].some(ok);
  const shown = all
    .filter(
      (c) =>
        pick("service", (v) => c.services.includes(v)) &&
        pick("status", (v) => c.status === v) &&
        (!q || c.name.toLowerCase().includes(q) || c.services.some((s) => serviceLabel(s).toLowerCase().includes(q))),
    )
    .sort((a, b) => (sort === "name" ? a.name.localeCompare(b.name) : time(b.updatedAt) - time(a.updatedAt)));

  const toggle = (group: string, value: string) =>
    setSelected((cur) => {
      const next = new Set(cur[group]);
      if (!next.delete(value)) next.add(value);
      return { ...cur, [group]: next };
    });

  return (
    <section className="flex flex-col gap-6 px-6 py-6">
      <header className="flex flex-row items-center justify-between gap-4 rounded-xl border border-neutral-200 bg-white px-5 py-4 shadow-[0_1px_2px_rgba(16,24,40,.04)]">
        <div>
          <h1 className="typo-title2_bold text-main-deep-blue">{title}</h1>
          <p className="mt-1 typo-body2_regular text-neutral-500">{subtitle}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">{actions}</div>
      </header>
      {banner}
      <div className="grid grid-cols-[256px_minmax(0,1fr)] items-start gap-5">
        <FilterPanel groups={groups} selected={selected} onToggle={toggle} />
        <div className="min-w-0">
          <ListToolbar
            query={query}
            onQuery={setQuery}
            sort={sort}
            onSort={setSort}
            view={view}
            onView={(v) => {
              setView(v);
              saveView(v);
            }}
            count={shown.length}
          />
          {!cards ? (
            <p className="mt-4 typo-body3_regular text-neutral-500">{t("common.loading")}</p>
          ) : shown.length === 0 ? (
            (all.length > 0 || empty) && <p className="mt-4 rounded-xl border border-dashed border-neutral-300 bg-white p-8 text-center typo-body2_regular text-neutral-500">
              {all.length === 0 ? empty : t("list.noMatch")}
            </p>
          ) : (
            <div className={`mt-4 grid gap-3 ${view === "grid" ? "grid-cols-2" : "grid-cols-1"}`}>{shown.map(renderCard)}</div>
          )}
        </div>
      </div>
    </section>
  );
}
