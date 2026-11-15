import { TrendingDown, TrendingUp } from "lucide-react";
import { t } from "../i18n";
import { AreaChart } from "./AreaChart";
import { formatDuration, formatSuccessRate, hourlySeries } from "./format";
import type { Side } from "./metrics";

/** 출처별 값 — 하나면 이름 없이, 둘("전체")이면 이름과 나란히. side null은 "—". */
export type HeroSide = { label: string; side: Side | null };

const dash = <T,>(v: T | null | undefined, f: (v: T) => string) => (v === null || v === undefined ? "—" : f(v));
const change = (v: number) => `${v > 0 ? "+" : ""}${Math.round(v)}%`;

function Values({ title, sides, pick }: { title: string; sides: HeroSide[]; pick: (s: Side) => string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="typo-caption1_medium text-main-light-blue">{title}</span>
      {sides.map((s) => (
        <div key={s.label} className="flex items-baseline justify-between">
          {sides.length > 1 && <span className="typo-caption2_medium text-main-light-blue/80">{s.label}</span>}
          <span className="typo-body1_bold tabular-nums">{s.side ? pick(s.side) : "—"}</span>
        </div>
      ))}
    </div>
  );
}

function ChangePill({ label, pct }: { label: string | null; pct: number | null | undefined }) {
  const Icon = (pct ?? 0) < 0 ? TrendingDown : TrendingUp;
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-main-blue px-2.5 py-1 typo-caption1_semibold text-neutral-white">
      {label && <span className="text-main-light-blue">{label}</span>}
      {pct !== null && pct !== undefined && <Icon className="size-3" aria-hidden="true" />}
      {dash(pct, change)}
    </span>
  );
}

export function Hero({ todayRuns, hourly, sides }: { todayRuns: number; hourly: Record<string, number>; sides: HeroSide[] }) {
  const many = sides.length > 1;
  return (
    <div className="relative grid shrink-0 grid-cols-[300px_1fr] items-stretch overflow-hidden rounded-brand-md bg-main-deep-blue p-6 text-neutral-white shadow-md">
      <div className="flex flex-col gap-3 border-r border-main-blue pr-8">
        <span className="typo-caption1_semibold tracking-wide text-main-light-blue">{t("dashboard.todayRuns")}</span>
        <div className="flex items-end justify-between gap-3">
          <div data-testid="today-runs" className="typo-display3_bold leading-none text-neutral-white tabular-nums">
            {todayRuns}
            <span className="typo-title2_medium ml-2 text-main-light-blue">{t("dashboard.runsUnit")}</span>
          </div>
          <div className="flex flex-col items-end gap-1">
            {sides.map((s) => (
              <ChangePill key={s.label} label={many ? s.label : null} pct={s.side?.changePct} />
            ))}
            <span className="typo-caption1_medium text-main-light-blue">{t("dashboard.vsYesterday")}</span>
          </div>
        </div>
        <div className="mt-auto grid grid-cols-2 gap-4 border-t border-white/10 pt-3">
          <Values title={t("dashboard.successRate")} sides={sides} pick={(s) => dash(s.successRate, formatSuccessRate)} />
          <Values title={t("dashboard.avgTime")} sides={sides} pick={(s) => dash(s.avgSeconds, formatDuration)} />
        </div>
      </div>
      <div className="flex min-w-0 flex-col pl-8">
        <span className="mb-2 typo-caption1_semibold tracking-wide text-main-light-blue">{t("dashboard.hourly")}</span>
        <div className="text-main-light-blue">
          <AreaChart series={hourlySeries(hourly)} />
        </div>
      </div>
    </div>
  );
}
