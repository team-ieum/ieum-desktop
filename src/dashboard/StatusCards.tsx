import { t, type Key } from "../i18n";
import type { Stats } from "./metrics";

// 웹 DashboardStatusPill + pillSkins
const SKINS: { key: keyof Stats; card: string; num: string; label: string; dot: string }[] = [
  {
    key: "active",
    card: "border-main-light-blue bg-main-light-blue",
    num: "border-main-light-blue text-main-deep-blue",
    label: "text-main-deep-blue",
    dot: "bg-main-blue",
  },
  { key: "inactive", card: "border-neutral-200 bg-neutral-white", num: "border-neutral-200 text-neutral-900", label: "text-neutral-600", dot: "bg-neutral-400" },
  { key: "errored", card: "border-danger-300 bg-danger-100", num: "border-danger-300 text-danger-700", label: "text-danger-700", dot: "bg-danger-600" },
  { key: "running", card: "border-neutral-200 bg-neutral-white", num: "border-neutral-200 text-main-deep-blue", label: "text-neutral-600", dot: "bg-main-blue" },
];

export function StatusCards({ stats }: { stats: Stats }) {
  return (
    <div className="flex shrink-0 flex-col gap-2.5">
      <h2 className="typo-title3_semibold text-neutral-900">{t("dashboard.statusTitle")}</h2>
      <div className="grid grid-cols-4 gap-3">
        {SKINS.map((s) => (
          <article key={s.key} className={`flex items-center gap-3.5 rounded-brand-md border px-4 py-3.5 ${s.card}`}>
            <div className={`grid h-11 w-11 shrink-0 place-items-center rounded-xl border bg-neutral-white text-xl font-bold tabular-nums ${s.num}`}>
              {stats[s.key]}
            </div>
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className={`inline-flex items-center gap-1.5 typo-body2_semibold ${s.label}`}>
                <span className={`h-1.5 w-1.5 rounded-full ${s.dot}`} />
                {t(`dashboard.stat.${s.key}` as Key)}
              </span>
              <span className="typo-caption1_regular text-neutral-400">{t(`dashboard.stat.${s.key}Sub` as Key)}</span>
            </div>
          </article>
        ))}
      </div>
    </div>
  );
}
