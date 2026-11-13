import { Bot, Check, Clock, LayoutGrid, Play, RefreshCw, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, type CredentialsView, type PlanUsage, type ProviderReport, type TokenParts, type UsageProvider, type UsageReport } from "../../api";
import { ProviderLogo } from "../../assets/providers";
import { formatRelative } from "../../dashboard/format";
import { locale, t, type Key } from "../../i18n";
import { formatCount, formatHm, formatShare, formatUsd } from "../../usage/format";
import { credName, Dropdown, WindowMeter, type DropItem } from "../../usage/parts";
import { accountsOf, BOXES, credLabel, planKey, type Plans } from "./AiPane";

const CARD = "rounded-brand-md border border-[#cde9f4] bg-white p-4";
const H4 = "typo-caption1_semibold text-neutral-700";
const EMPTY = "py-6 text-center typo-caption1_regular text-neutral-400";
const ICON_BTN = "grid size-7 place-items-center rounded-lg text-neutral-500 hover:bg-neutral-100";

/** 토큰 구성 4계열 — 이음 파랑 + dataviz 기본 팔레트 2~4번(validate_palette 통과, 대비 WARN → 범례에 숫자). */
const PARTS: { key: keyof TokenParts; label: Key; color: string }[] = [
  { key: "input", label: "usage.input", color: "#007ba7" },
  { key: "output", label: "usage.output", color: "#eb6834" },
  { key: "cacheRead", label: "usage.cacheRead", color: "#1baf7a" },
  { key: "cacheWrite", label: "usage.cacheWrite", color: "#eda100" },
];
const sum = (p: TokenParts) => p.input + p.output + p.cacheRead + p.cacheWrite;

/** "2026-11-10" → 로컬 날짜. */
const day = (d: string) => {
  const [y, m, dd] = d.split("-").map(Number);
  return new Date(y, m - 1, dd);
};
const fmtDate = (d: Date, o: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat(locale(), o).format(d);
const monthDay = (d: string) => fmtDate(day(d), { month: "long", day: "numeric" });

type Sel = "overview" | UsageProvider;
const box = (p: UsageProvider) => BOXES.find((b) => b.id === p)!;

/** 그 회사 구독 계정마다 [계정, 상태]. Antigravity는 지금 로그인 하나. */
function subAccounts(p: UsageProvider, view: CredentialsView | null | undefined, plans: Plans) {
  const sub = box(p).sub;
  if (sub.id === "antigravity") {
    const plan = plans[planKey("antigravity")];
    return [{ key: "antigravity", name: plan?.accountEmail ?? sub.name, isDefault: true, plan }];
  }
  return accountsOf(view, sub.id).map((m) => ({ key: m.id, name: m.email ?? credLabel(m), isDefault: m.isDefault, plan: plans[planKey(sub.id, m.id)] }));
}
const okPlan = (p: PlanUsage | null | undefined): p is PlanUsage => p?.state === "ok" && p.windows.length > 0;

function Tile({ icon: Icon, label, value, note }: { icon?: LucideIcon; label: string; value: string; note?: string }) {
  return (
    <div role="group" aria-label={label} className={`${CARD} flex flex-col gap-1`}>
      {Icon && <Icon className="size-4 text-main-blue" aria-hidden="true" />}
      <span className="text-[22px] leading-tight font-semibold text-neutral-900">{value}</span>
      <span className="typo-caption1_medium text-neutral-500">{label}</span>
      {note && <span className="typo-caption2_regular text-neutral-400">{note}</span>}
    </div>
  );
}

/** 오른쪽 위 드롭다운 — 개요 / Claude / OpenAI / Google. */
function Picker({ sel, onSel }: { sel: Sel; onSel: (s: Sel) => void }) {
  const items: DropItem<Sel>[] = [
    { id: "overview", name: t("usage.overview"), icon: <LayoutGrid className="size-4 text-main-deep-blue" aria-hidden="true" /> },
    ...BOXES.map((b) => ({ id: b.id, name: b.name, icon: <ProviderLogo id={b.logo} size={18} /> })),
  ];
  return <Dropdown label={t("usage.analysis")} items={items} sel={sel} onSel={onSel} />;
}

/** 머리 — 제목·"N분 전 업데이트됨"·새로 고침. */
function Head({ title, updatedAt, onRefresh }: { title: string; updatedAt: string; onRefresh: () => void }) {
  return (
    <div className="flex items-center gap-2">
      <h3 className="typo-body2_semibold text-main-deep-blue">{title}</h3>
      <span className="typo-caption2_regular text-neutral-400">{t("usage.updated").replace("{time}", formatRelative(updatedAt))}</span>
      <button type="button" aria-label={t("usage.refresh")} title={t("usage.refresh")} className={`${ICON_BTN} ml-auto`} onClick={onRefresh}>
        <RefreshCw className="size-3.5" aria-hidden="true" />
      </button>
    </div>
  );
}

/** 일일 강도 — 최근 6주 칸(한 열 = 7일), 한 색 농도 4단계. */
function Heatmap({ heat }: { heat: UsageReport["overview"]["heat"] }) {
  const max = Math.max(0, ...heat.map((d) => d.tokens));
  const level = (n: number) => (n === 0 ? 0 : Math.min(4, Math.ceil((n / max) * 4)));
  const SHADE = ["#eef1f4", "rgba(0,123,167,.22)", "rgba(0,123,167,.45)", "rgba(0,123,167,.7)", "#007ba7"];
  const peak = heat.reduce((a, d) => (d.tokens > a.tokens ? d : a), heat[0]);
  return (
    <figure aria-label={t("usage.heat")} className={CARD}>
      <div className="mb-3 flex items-center gap-2">
        <h4 className={H4}>{t("usage.heat")}</h4>
        {max > 0 && <span className="ml-auto rounded-full bg-main-light-blue px-2 py-0.5 typo-caption2_semibold text-main-deep-blue">{t("usage.peak").replace("{date}", monthDay(peak.date))}</span>}
      </div>
      {max === 0 ? (
        <p className={EMPTY}>{t("usage.empty42")}</p>
      ) : (
        <>
          <div className="grid grid-flow-col grid-rows-7 justify-start gap-[3px]">
            {heat.map((d) => {
              const label = `${monthDay(d.date)} · ${formatCount(d.tokens)} ${t("usage.tokens")}`;
              return <span key={d.date} data-testid="heat-cell" title={label} aria-label={label} role="img" className="size-4 rounded-[4px]" style={{ background: SHADE[level(d.tokens)] }} />;
            })}
          </div>
          <div className="mt-3 flex items-center gap-1 typo-caption2_regular text-neutral-400">
            <span>{t("usage.less")}</span>
            {SHADE.map((c) => (
              <span key={c} aria-hidden="true" className="size-2.5 rounded-[3px]" style={{ background: c }} />
            ))}
            <span>{t("usage.more")}</span>
          </div>
        </>
      )}
    </figure>
  );
}

/** 범례 — 색 점 + 이름 + (있으면) 숫자. 글은 글 색, 계열 색은 점만. */
function Legend({ parts }: { parts?: TokenParts }) {
  return (
    <ul className="flex flex-wrap gap-x-4 gap-y-1.5">
      {PARTS.map((p) => (
        <li key={p.key} className="flex items-center gap-1.5 typo-caption1_regular text-neutral-600">
          <span aria-hidden="true" className="size-2 rounded-full" style={{ background: p.color }} />
          {t(p.label)}
          {parts && <span className="tabular-nums font-semibold text-neutral-800">{formatCount(parts[p.key])}</span>}
        </li>
      ))}
    </ul>
  );
}

/** 토큰 구성 — 한 줄 누적 막대(8px, 2px 틈) + 범례 숫자. */
function Composition({ c }: { c: TokenParts }) {
  const total = sum(c);
  return (
    <figure aria-label={t("usage.composition")} className={CARD}>
      <h4 className={`${H4} mb-3`}>{t("usage.composition")}</h4>
      {total === 0 ? (
        <p className={EMPTY}>{t("usage.empty30")}</p>
      ) : (
        <>
          <div className="flex h-2 gap-[2px]">
            {PARTS.filter((p) => c[p.key] > 0).map((p) => (
              <span
                key={p.key}
                title={`${t(p.label)} ${formatCount(c[p.key])} (${formatShare(c[p.key] / total)})`}
                className="h-full rounded-full"
                style={{ flexGrow: c[p.key], background: p.color, minWidth: 4 }}
              />
            ))}
          </div>
          <div className="mt-3">
            <Legend parts={c} />
          </div>
        </>
      )}
    </figure>
  );
}

/** 개요의 공급자 카드 — 활성 배지·상위 모델·토큰·호출·예상 비용, 구독이 있으면 기본 계정 창 막대. */
function ProviderCard({ p, plan }: { p: ProviderReport; plan: PlanUsage | null | undefined }) {
  const b = box(p.provider);
  const now = new Date();
  return (
    <article aria-label={b.name} className={`${CARD} flex flex-col gap-3`}>
      <div className="flex items-center gap-2">
        <ProviderLogo id={b.logo} size={22} />
        <span className="typo-body3_semibold text-neutral-800">{b.name}</span>
        {p.calls > 0 && <span className="ml-auto rounded-full bg-node-green/10 px-2 py-0.5 typo-caption2_semibold text-node-green">{t("usage.active")}</span>}
      </div>
      {p.calls === 0 ? (
        <p className="typo-caption1_regular text-neutral-400">{t("usage.noUse30")}</p>
      ) : (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 typo-caption1_regular">
          <dt className="text-neutral-500">{t("usage.topModel")}</dt>
          <dd className="truncate text-right text-neutral-800">{p.topModel || "—"}</dd>
          <dt className="text-neutral-500">{t("usage.tokens")}</dt>
          <dd className="text-right tabular-nums text-neutral-800">{formatCount(sum(p))}</dd>
          <dt className="text-neutral-500">{t("usage.calls")}</dt>
          <dd className="text-right tabular-nums text-neutral-800">{formatCount(p.calls)}</dd>
          <dt className="text-neutral-500">{t("usage.cost")}</dt>
          <dd className="text-right tabular-nums text-neutral-800">{formatUsd(p.usd)}</dd>
        </dl>
      )}
      {okPlan(plan) && (
        <div className="flex flex-col gap-2 border-t border-neutral-100 pt-3">
          {plan.windows.map((w) => (
            <WindowMeter key={w.kind} w={w} now={now} />
          ))}
        </div>
      )}
    </article>
  );
}

/** 일일 사용량 — 사용 있는 날마다 누적 세로 막대(≤24px, 2px 틈, 위 4px 둥글게). */
function DailyBars({ days }: { days: ProviderReport["byDay"] }) {
  const max = Math.max(0, ...days.map(sum));
  const H = 140;
  return (
    <figure aria-label={t("usage.daily")} className={CARD}>
      <h4 className={`${H4} mb-3`}>{t("usage.daily")}</h4>
      {days.length === 0 ? (
        <p className={EMPTY}>{t("usage.empty30")}</p>
      ) : (
        <>
          <div className="relative border-b border-neutral-200 pt-4">
            <span className="absolute top-0 left-0 typo-caption2_regular tabular-nums text-neutral-400">{formatCount(max)}</span>
            <div className="flex items-end gap-[2px]" style={{ height: H }}>
              {days.map((d) => {
                const tip = [monthDay(d.date), ...PARTS.map((p) => `${t(p.label)} ${formatCount(d[p.key])}`)].join(" · ");
                return (
                  <div key={d.date} data-testid="day-bar" title={tip} aria-label={tip} role="img" className="flex max-w-6 flex-1 flex-col-reverse gap-[2px] overflow-hidden rounded-t-[4px]">
                    {PARTS.filter((p) => d[p.key] > 0).map((p) => (
                      <span key={p.key} style={{ height: Math.max(1, (d[p.key] / max) * H), background: p.color }} />
                    ))}
                  </div>
                );
              })}
            </div>
          </div>
          <div className="mt-1 flex justify-between typo-caption2_regular text-neutral-400">
            <span>{monthDay(days[0].date)}</span>
            {days.length > 1 && <span>{monthDay(days[days.length - 1].date)}</span>}
          </div>
          <div className="mt-3">
            <Legend />
          </div>
        </>
      )}
    </figure>
  );
}

const TH = "py-1.5 pr-3 last:pr-0 typo-caption2_semibold text-neutral-500";
const TD = "py-1.5 pr-3 last:pr-0 typo-caption1_regular text-neutral-800 border-t border-neutral-100";
/** 글 칸은 왼쪽, `num`번째 칸부터는 숫자 — 오른쪽·고정폭 숫자. */
const align = (j: number, num: number) => (j >= num ? "text-right tabular-nums" : "text-left");

function Table({ label, cols, num, rows, className = "" }: { label: string; cols: Key[]; num: number; rows: ReactNode[][]; className?: string }) {
  return (
    <section className={`${CARD} min-w-0 ${className}`}>
      <h4 className={`${H4} mb-2`}>{label}</h4>
      {rows.length === 0 ? (
        <p className={EMPTY}>{t("usage.empty30")}</p>
      ) : (
        <div className="overflow-x-auto">
          <table aria-label={label} className="w-full">
            <thead>
              <tr>
                {cols.map((c, j) => (
                  <th key={c} className={`${TH} ${align(j, num)}`}>
                    {t(c)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i}>
                  {r.map((c, j) => (
                    <td key={j} className={`${TD} ${align(j, num)} ${j < num ? "max-w-40 truncate" : ""}`}>
                      {c}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

/** 공급자 하나 — 타일 8개·구독 사용량·일일 사용량·모델별·워크플로우별·최근 실행. */
function ProviderDetail({ p, report, view, plans, onRefresh }: { p: ProviderReport; report: UsageReport; view: CredentialsView | null | undefined; plans: Plans; onRefresh: () => void }) {
  const b = box(p.provider);
  const now = new Date();
  const subs = subAccounts(p.provider, view, plans).filter((a) => okPlan(a.plan));
  const wfName = (name: string | null) => name ?? t("usage.deletedWorkflow");
  const time = (iso: string) => fmtDate(new Date(iso), { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
  return (
    <div className="flex flex-col gap-3">
      <div>
        <Head title={t("usage.providerTitle").replace("{name}", b.name)} updatedAt={report.updatedAt} onRefresh={onRefresh} />
        <p className="typo-caption2_regular text-neutral-400">{t("usage.last30")}</p>
      </div>
      <div className="grid grid-cols-4 gap-2">
        <Tile label={t("usage.inputTokens")} value={formatCount(p.input)} />
        <Tile label={t("usage.outputTokens")} value={formatCount(p.output)} />
        <Tile label={t("usage.cacheRead")} value={formatCount(p.cacheRead)} />
        <Tile label={t("usage.cacheWrite")} value={formatCount(p.cacheWrite)} />
        <Tile label={t("usage.cacheReuse")} value={formatShare(p.cacheReuse)} />
        <Tile label={t("usage.noCacheCalls")} value={formatShare(p.noCacheCallShare)} />
        <Tile label={t("usage.runsCalls")} value={`${formatCount(p.runs)} / ${formatCount(p.calls)}`} />
        <Tile label={t("usage.cost")} value={formatUsd(p.usd)} note={p.unpricedCalls > 0 ? t("usage.costNote") : undefined} />
      </div>
      {subs.length > 0 && (
        <section aria-label={t("usage.subUsage")} className={CARD}>
          <h4 className={`${H4} mb-3`}>{t("usage.subUsage")}</h4>
          <ul className="flex flex-col gap-4">
            {subs.map((a) => (
              <li key={a.key} className="flex flex-col gap-2">
                <span className="flex items-center gap-1.5 typo-caption1_semibold text-neutral-800">
                  {a.isDefault ? <Check className="size-3.5 text-main-blue" aria-label={t("settings.defaultBadge")} /> : <span className="size-3.5" />}
                  {a.name}
                </span>
                {a.plan!.windows.map((w) => (
                  <WindowMeter key={w.kind} w={w} now={now} inline />
                ))}
              </li>
            ))}
          </ul>
        </section>
      )}
      <DailyBars days={p.byDay} />
      <div className="grid grid-cols-2 gap-3">
        <Table label={t("usage.byModel")} cols={["usage.col.model", "usage.col.tokens", "usage.col.calls"]} num={1} rows={p.byModel.map((m) => [m.model || "—", formatCount(m.tokens), formatCount(m.calls)])} />
        <Table
          label={t("usage.byWorkflow")}
          cols={["usage.col.workflow", "usage.col.tokens", "usage.col.calls", "usage.col.runs"]}
          num={1}
          rows={p.byWorkflow.map((w) => [wfName(w.name), formatCount(w.tokens), formatCount(w.calls), formatCount(w.runs)])}
        />
      </div>
      <Table
        label={t("usage.recent")}
        cols={["usage.col.time", "usage.col.workflow", "usage.col.model", "usage.col.cred", "usage.input", "usage.output", "usage.col.cache"]}
        num={4}
        rows={p.recent.map((r) => [time(r.at), wfName(r.workflowName), r.model || "—", credName(view, r.credential), formatCount(r.input), formatCount(r.output), formatCount(r.cache)])}
      />
    </div>
  );
}

/** 설정 › 통계 및 사용량 — 요약 타일·사용량 분석(개요 / 공급자별). */
export function UsagePane({ view, plans, refreshPlans }: { view: CredentialsView | null | undefined; plans: Plans; refreshPlans: () => void }) {
  // undefined = 읽는 중, null = 못 읽음
  const [report, setReport] = useState<UsageReport | null | undefined>(undefined);
  const [sel, setSel] = useState<Sel>("overview");
  const load = useCallback(() => api.usageReport().then(setReport, () => setReport(null)), []);
  useEffect(() => void load(), [load]);
  const refresh = () => {
    void load();
    refreshPlans();
  };

  if (report === undefined) return <p className="typo-body3_regular text-neutral-500">{t("common.loading")}</p>;
  if (report === null) return <p className="typo-body3_regular text-neutral-500">{t("usage.empty")}</p>;
  const o = report.overview;
  const defaultPlan = (p: UsageProvider) => subAccounts(p, view, plans).find((a) => a.isDefault)?.plan;
  const detail = sel === "overview" ? undefined : report.providers.find((p) => p.provider === sel);

  return (
    <div className="flex flex-col gap-3">
      <p className="-mt-2 mb-1 typo-caption1_regular text-neutral-500">{t("usage.pageDesc")}</p>
      <div className="grid grid-cols-3 gap-2">
        <Tile icon={Play} label={t("usage.runs")} value={formatCount(report.totals.runs)} />
        <Tile icon={Clock} label={t("usage.runTime")} value={formatHm(report.totals.runSeconds)} />
        <Tile icon={Bot} label={t("usage.aiCalls")} value={formatCount(report.totals.aiCalls)} />
      </div>
      {report.trackingSince && (
        <p className="typo-caption2_regular text-neutral-400">
          {t("usage.trackingSince").replace("{date}", fmtDate(new Date(report.trackingSince), { year: "numeric", month: "long", day: "numeric" }))}
        </p>
      )}

      <div className="mt-4 flex items-center justify-between">
        <h3 className="typo-body1_semibold text-neutral-800">{t("usage.analysis")}</h3>
        <Picker sel={sel} onSel={setSel} />
      </div>

      {detail ? (
        <ProviderDetail p={detail} report={report} view={view} plans={plans} onRefresh={refresh} />
      ) : (
        <div className="flex flex-col gap-3">
          <Head title={t("usage.overviewTitle")} updatedAt={report.updatedAt} onRefresh={refresh} />
          <div className="grid grid-cols-4 gap-2">
            <Tile label={t("usage.totalTokens")} value={formatCount(o.totalTokens)} />
            <Tile label={t("usage.cost")} value={formatUsd(o.usd)} note={t("usage.costNote")} />
            <Tile label={t("usage.activeDays")} value={String(o.activeDays)} />
            <Tile label={t("usage.cacheShare")} value={formatShare(o.cacheShare)} />
          </div>
          <div className="grid grid-cols-2 items-start gap-3">
            <Heatmap heat={o.heat} />
            <Composition c={o.composition} />
          </div>
          <h4 className={`${H4} mt-2`}>{t("usage.providers")}</h4>
          <div className="grid grid-cols-3 gap-3">
            {report.providers.map((p) => (
              <ProviderCard key={p.provider} p={p} plan={defaultPlan(p.provider)} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/** 설정 검색에 걸릴 문구. */
export const usageTexts = () =>
  (
    [
      "usage.pageDesc",
      "usage.runs",
      "usage.runTime",
      "usage.aiCalls",
      "usage.analysis",
      "usage.overviewTitle",
      "usage.totalTokens",
      "usage.cost",
      "usage.activeDays",
      "usage.cacheShare",
      "usage.heat",
      "usage.composition",
      "usage.providers",
      "usage.subUsage",
      "usage.daily",
      "usage.byModel",
      "usage.byWorkflow",
      "usage.recent",
    ] as Key[]
  ).map((k) => t(k));
