import { Check, LayoutGrid, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, type CredentialsView, type DayUse, type ModelUse, type PlanUsage, type ProviderReport, type UsageProvider, type UsageReport, type WorkflowUse } from "../../api";
import { ProviderLogo } from "../../assets/providers";
import { formatRelative } from "../../dashboard/format";
import { locale, t, type Key } from "../../i18n";
import { formatCount, formatUsd } from "../../usage/format";
import { credName, Dropdown, hot, resetText, tightest, usedOf, windowName, WindowMeter, type DropItem } from "../../usage/parts";
import { accountsOf, BOXES, credLabel, planKey, type Plans } from "./AiPane";

const CARD = "rounded-brand-md border border-[#cde9f4] bg-white p-4";
const H4 = "typo-caption1_semibold text-neutral-700";
const EMPTY = "py-6 text-center typo-caption1_regular text-neutral-400";
const ICON_BTN = "grid size-7 place-items-center rounded-lg text-neutral-500 hover:bg-neutral-100";

const sum = (p: { input: number; output: number; cacheRead: number; cacheWrite: number }) => p.input + p.output + p.cacheRead + p.cacheWrite;

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

function Tile({ label, value, note, valueClass = "" }: { label: string; value: string; note?: string; valueClass?: string }) {
  return (
    <div role="group" aria-label={label} className={`${CARD} flex flex-col gap-1`}>
      <span className={`text-[22px] leading-tight font-semibold ${valueClass || "text-neutral-900"}`}>{value}</span>
      <span className="typo-caption1_medium text-neutral-500">{label}</span>
      {note && <span className="typo-caption2_regular text-neutral-400">{note}</span>}
    </div>
  );
}

/** 구독 한도 — 연결된 모든 구독 계정 중 가장 빡빡한 창의 남은 비율. */
function SubLeftTile({ view, plans }: { view: CredentialsView | null | undefined; plans: Plans }) {
  const now = new Date();
  let worst: { name: string; w: NonNullable<ReturnType<typeof tightest>> } | undefined;
  for (const b of BOXES) {
    for (const a of subAccounts(b.id, view, plans)) {
      const w = tightest(a.plan);
      if (w && (!worst || w.usedPct > worst.w.usedPct)) worst = { name: b.name, w };
    }
  }
  if (!worst) return <Tile label={t("usage.subLeft")} value="—" note={t("usage.subNone")} />;
  const used = usedOf(worst.w);
  return (
    <Tile
      label={t("usage.subLeft")}
      value={`${100 - used}%`}
      valueClass={hot(used)}
      note={`${worst.name} · ${windowName(worst.w)} · ${resetText(worst.w, now)}`}
    />
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
            <WindowMeter key={`${w.kind}:${w.label ?? ""}`} w={w} now={now} />
          ))}
        </div>
      )}
    </article>
  );
}

/** 일일 사용량 — 30일 칸마다 토큰 세로 막대(2px 틈, 위 4px 둥글게, 빈 날은 빈칸), 툴팁에 예상 비용. */
function DailyBars({ days }: { days: DayUse[] }) {
  const max = Math.max(0, ...days.map((d) => d.tokens));
  const H = 140;
  return (
    <figure aria-label={t("usage.daily")} className={CARD}>
      <h4 className={`${H4} mb-3`}>{t("usage.daily")}</h4>
      {max === 0 ? (
        <p className={EMPTY}>{t("usage.empty30")}</p>
      ) : (
        <>
          <div className="relative border-b border-neutral-200 pt-4">
            <span className="absolute top-0 left-0 typo-caption2_regular tabular-nums text-neutral-400">{formatCount(max)}</span>
            <div className="flex items-end gap-[2px]" style={{ height: H }}>
              {days.map((d) => {
                const tip = `${monthDay(d.date)} · ${formatCount(d.tokens)} ${t("usage.tokens")} · ${formatUsd(d.usd)}`;
                return (
                  <span
                    key={d.date}
                    data-testid="day-bar"
                    title={tip}
                    aria-label={tip}
                    role="img"
                    className="flex-1 rounded-t-[4px] bg-main-blue"
                    style={{ height: d.tokens && Math.max(1, (d.tokens / max) * H) }}
                  />
                );
              })}
            </div>
          </div>
          <div className="mt-1 flex justify-between typo-caption2_regular text-neutral-400">
            <span>{monthDay(days[0].date)}</span>
            {days.length > 1 && <span>{monthDay(days[days.length - 1].date)}</span>}
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

const wfName = (name: string | null) => name ?? t("usage.deletedWorkflow");

/** 모델별 표 — 개요는 회사 로고를 같이 보인다. */
function ModelTable({ rows, logo = false }: { rows: ModelUse[]; logo?: boolean }) {
  return (
    <Table
      label={t("usage.byModel")}
      cols={["usage.col.model", "usage.col.tokens", "usage.col.cost", "usage.col.calls"]}
      num={1}
      rows={rows.map((m) => [
        logo ? (
          <span className="flex items-center gap-1.5">
            <ProviderLogo id={box(m.provider).logo} size={14} />
            {m.model || "—"}
          </span>
        ) : (
          m.model || "—"
        ),
        formatCount(m.tokens),
        formatUsd(m.usd),
        formatCount(m.calls),
      ])}
    />
  );
}

function WorkflowTable({ rows }: { rows: WorkflowUse[] }) {
  return (
    <Table
      label={t("usage.byWorkflow")}
      cols={["usage.col.workflow", "usage.col.tokens", "usage.col.cost", "usage.col.calls", "usage.col.runs"]}
      num={1}
      rows={rows.map((w) => [wfName(w.name), formatCount(w.tokens), formatUsd(w.usd), formatCount(w.calls), formatCount(w.runs)])}
    />
  );
}

/** 공급자 하나 — 타일 4개·구독 사용량·일일 사용량·모델별·워크플로우별·최근 실행. */
function ProviderDetail({ p, report, view, plans, onRefresh }: { p: ProviderReport; report: UsageReport; view: CredentialsView | null | undefined; plans: Plans; onRefresh: () => void }) {
  const b = box(p.provider);
  const now = new Date();
  const subs = subAccounts(p.provider, view, plans).filter((a) => okPlan(a.plan));
  const time = (iso: string) => fmtDate(new Date(iso), { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
  return (
    <div className="flex flex-col gap-3">
      <div>
        <Head title={t("usage.providerTitle").replace("{name}", b.name)} updatedAt={report.updatedAt} onRefresh={onRefresh} />
        <p className="typo-caption2_regular text-neutral-400">{t("usage.last30")}</p>
      </div>
      <div className="grid grid-cols-4 gap-2">
        <Tile label={t("usage.cost")} value={formatUsd(p.usd)} note={p.unpricedCalls > 0 ? t("usage.costNote") : undefined} />
        <Tile label={t("usage.inputTokens")} value={formatCount(p.input)} />
        <Tile label={t("usage.outputTokens")} value={formatCount(p.output)} />
        <Tile label={t("usage.runsCalls")} value={`${formatCount(p.runs)} / ${formatCount(p.calls)}`} />
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
                  <WindowMeter key={`${w.kind}:${w.label ?? ""}`} w={w} now={now} inline />
                ))}
              </li>
            ))}
          </ul>
        </section>
      )}
      <DailyBars days={p.byDay} />
      <div className="grid grid-cols-2 gap-3">
        <WorkflowTable rows={p.byWorkflow} />
        <ModelTable rows={p.byModel} />
      </div>
      <Table
        label={t("usage.recent")}
        cols={["usage.col.time", "usage.col.workflow", "usage.col.model", "usage.col.cred", "usage.input", "usage.output"]}
        num={4}
        rows={p.recent.map((r) => [time(r.at), wfName(r.workflowName), r.model || "—", credName(view, r.credential), formatCount(r.input), formatCount(r.output)])}
      />
    </div>
  );
}

/** 설정 › 통계 및 사용량 — AI 사용량 분석(개요 / 공급자별). 실행 통계는 대시보드 몫. */
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
      <p className="-mt-2 mb-1 typo-caption1_regular text-neutral-500">
        {t("usage.pageDesc")}
        {report.trackingSince && ` ${t("usage.trackingSince").replace("{date}", fmtDate(new Date(report.trackingSince), { year: "numeric", month: "long", day: "numeric" }))}`}
      </p>

      <div className="flex items-center justify-between">
        <h3 className="typo-body1_semibold text-neutral-800">{t("usage.analysis")}</h3>
        <Picker sel={sel} onSel={setSel} />
      </div>

      {detail ? (
        <ProviderDetail p={detail} report={report} view={view} plans={plans} onRefresh={refresh} />
      ) : (
        <div className="flex flex-col gap-3">
          <div>
            <Head title={t("usage.overviewTitle")} updatedAt={report.updatedAt} onRefresh={refresh} />
            <p className="typo-caption2_regular text-neutral-400">{t("usage.last30")}</p>
          </div>
          <div className="grid grid-cols-4 gap-2">
            <Tile label={t("usage.cost")} value={formatUsd(o.usd)} note={t("usage.costNote")} />
            <Tile label={t("usage.totalTokens")} value={formatCount(o.totalTokens)} />
            <Tile label={t("usage.aiCalls")} value={formatCount(o.calls)} />
            <SubLeftTile view={view} plans={plans} />
          </div>
          <DailyBars days={o.byDay} />
          <div className="grid grid-cols-2 gap-3">
            <WorkflowTable rows={o.byWorkflow} />
            <ModelTable rows={o.byModel} logo />
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
      "usage.aiCalls",
      "usage.analysis",
      "usage.overviewTitle",
      "usage.totalTokens",
      "usage.cost",
      "usage.subLeft",
      "usage.providers",
      "usage.subUsage",
      "usage.daily",
      "usage.byModel",
      "usage.byWorkflow",
      "usage.recent",
    ] as Key[]
  ).map((k) => t(k));
