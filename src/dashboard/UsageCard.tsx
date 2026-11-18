import { ChevronRight, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type UsageRange, type UsageSummary } from "../api";
import { ProviderLogo } from "../assets/providers";
import { formatRelative } from "./format";
import { t } from "../i18n";
import { apiName, PROVIDERS, type SubId } from "../screens/settings/AiPane";
import { connectedSubs } from "../shell/UsagePopover";
import { useCredentials } from "../stores/credentials";
import { useKeyLimits } from "../stores/keyLimits";
import { useDefaultPlans } from "../stores/plans";
import { useWorkflows } from "../stores/workflows";
import { formatTokens, formatUsd } from "../usage/format";
import { credName, Dropdown, hot, LimitMeter, usedOf, WindowMeter } from "../usage/parts";

const RANGES: UsageRange[] = ["today", "week", "month"];
const tokens = (x: { input: number; output: number }) => x.input + x.output;
const PICK_KEY = "ieum.dashboardSub";

/** 대시보드 "AI 사용량" — 왼쪽 API 칸(내 PC AI 노드가 쓴 토큰·예상 금액, 기간 탭), 오른쪽 구독 칸(기본 계정 창별 사용, 상태 줄과 같은 스토어). 웹 이음 탭에서는 그리지 않는다. */
export function UsageCard({ hidden, onUsage, onAccounts }: { hidden: boolean; onUsage: () => void; onAccounts: () => void }) {
  const [range, setRange] = useState<UsageRange>("today");
  const view = useCredentials((s) => s.view);
  const { plans, refreshNow } = useDefaultPlans(view);
  const [picked, setPicked] = useState<string | null>(() => {
    try {
      return localStorage.getItem(PICK_KEY);
    } catch {
      return null;
    }
  });
  // undefined = 읽는 중, null = 못 읽음
  const [sum, setSum] = useState<UsageSummary | null | undefined>(undefined);
  const [refreshing, setRefreshing] = useState(false);
  // 키별로 펼친 공급원 — 새로 고침·기간 전환에도 유지
  const [open, setOpen] = useState<Record<string, boolean>>({});
  // 기간이 바뀌면 올라가 — 늦게 온 옛 새로고침 결과가 새 기간을 덮지 않게
  const gen = useRef(0);
  const workflows = useWorkflows((s) => s.list);
  const limitView = useKeyLimits((s) => s.view);

  // 한도 막대는 기간 탭과 무관하게 이번 달 — 사용량을 다시 읽을 때 같이 읽어 금액과 어긋나지 않게. 못 읽으면 막대만 없다.
  const reloadLimits = () => useKeyLimits.getState().load().catch(() => {});

  const refreshApi = () => {
    if (refreshing) return;
    const mine = gen.current;
    setRefreshing(true);
    reloadLimits();
    api.usageSummary(range).then(
      (s) => {
        if (gen.current === mine) {
          setSum(s);
          setRefreshing(false);
        }
      },
      () => gen.current === mine && setRefreshing(false), // 실패하면 이전 결과를 둔다
    );
  };

  useEffect(() => {
    if (hidden) return;
    let live = true;
    gen.current++;
    setRefreshing(false);
    setSum(undefined);
    reloadLimits();
    api.usageSummary(range).then(
      (s) => live && setSum(s),
      () => live && setSum(null),
    );
    return () => {
      live = false;
    };
  }, [hidden, range]);

  if (hidden) return null;
  const now = new Date();
  const subs = connectedSubs(view, plans);
  // 고른 구독, 없거나 사라졌으면 가장 빡빡한 창을 가진 구독
  const current = subs.find((x) => x.sub.id === picked) ?? subs.reduce<(typeof subs)[number] | undefined>((a, x) => (!a || x.w.usedPct > a.w.usedPct ? x : a), undefined);
  const pick = (id: SubId) => {
    setPicked(id);
    try {
      localStorage.setItem(PICK_KEY, id);
    } catch {
      // 기억 못 해도 이번 화면에서는 고른 대로.
    }
  };
  const total = sum ? tokens({ input: sum.totalInput, output: sum.totalOutput }) : 0;
  const name = (id: string) => workflows.find((w) => w.id === id)?.name ?? t("history.deletedWorkflow");

  return (
    <div className="overflow-hidden rounded-brand-md border border-neutral-200 bg-neutral-white shadow-sm">
      <div className="flex items-center justify-between gap-4 border-b border-neutral-200 px-5 py-3.5">
        <h3 className="typo-body2_semibold text-neutral-900">{t("usage.title")}</h3>
        <button type="button" className="inline-flex items-center gap-0.5 typo-caption1_semibold text-main-blue hover:underline" onClick={onUsage}>
          {t("usage.seeMore")}
          <ChevronRight className="size-3.5" aria-hidden="true" />
        </button>
      </div>
      {/* 왼쪽 API 칸, 오른쪽 구독 칸 — 가운데 세로 구분선 */}
      <div data-layout="split" className="grid grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)] divide-x divide-neutral-200">
        <section aria-label={t("usage.apiSection")}>
          <div className="flex items-center justify-between gap-4 px-5 pt-3">
            <h4 className="typo-caption1_semibold tracking-[.04em] text-main-gray">{t("usage.apiSection")}</h4>
            <button
              type="button"
              aria-label={t("usage.refreshApi")}
              title={t("usage.refreshApi")}
              className="mr-auto -ml-2 grid size-7 shrink-0 place-items-center rounded-lg text-neutral-500 hover:bg-neutral-100"
              onClick={refreshApi}
            >
              <RefreshCw className={`size-3.5 ${refreshing ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
            </button>
            <div role="tablist" className="inline-flex rounded-lg border border-neutral-200 bg-neutral-100 p-0.5">
              {RANGES.map((r) => (
                <button
                  key={r}
                  type="button"
                  role="tab"
                  aria-selected={range === r}
                  className={`h-7 rounded-md px-3 typo-caption1_semibold ${range === r ? "bg-main-deep-blue text-white" : "text-neutral-600"}`}
                  onClick={() => setRange(r)}
                >
                  {t(`usage.range.${r}`)}
                </button>
              ))}
            </div>
          </div>
          {sum === undefined ? (
            <p className="px-5 py-8 text-center typo-body3_regular text-neutral-400">{t("common.loading")}</p>
          ) : sum === null ? (
            <p className="px-5 py-8 text-center typo-body3_regular text-neutral-500">{t("usage.empty")}</p>
          ) : (
            <div className={`grid grid-cols-[minmax(0,0.7fr)_minmax(0,1fr)] gap-6 px-5 py-4 ${refreshing ? "opacity-50" : ""}`}>
              <div>
                <p className="typo-caption1_semibold text-neutral-500">{t("usage.estimated")}</p>
                <p className="mt-1 text-[28px] font-bold tabular-nums text-main-deep-blue">{formatUsd(sum.totalUsd)}</p>
                <p className="typo-caption1_regular text-neutral-500">
                  {formatTokens(total)} {t("usage.tokens")}
                </p>
              </div>
              <div className="flex flex-col gap-5">
                <div>
                  <p className="mb-2 typo-caption1_semibold text-neutral-500">{t("usage.bySource")}</p>
                  <ul className="flex flex-col gap-2">
                    {PROVIDERS.map((p) => {
                      const s = sum.bySource.find((x) => x.source === p.id);
                      const n = s ? tokens(s) : 0;
                      const keys = sum.byCredential.filter((c) => c.source === p.id).sort((a, b) => tokens(b) - tokens(a));
                      // 이 기간에 안 쓴 한도 키도 0 줄로 더한다
                      for (const lk of Object.keys(limitView?.limits ?? {})) {
                        const id = lk.startsWith(`${p.id}:`) ? lk.slice(p.id.length + 1) : null;
                        if (id && !keys.some((c) => c.credential === id)) keys.push({ source: p.id, credential: id, input: 0, output: 0, usd: 0 });
                      }
                      const limitOf = (id: string) => limitView?.limits[`${p.id}:${id}`] ?? null;
                      const expandable = keys.length >= 2 || keys.some((c) => limitOf(c.credential) !== null);
                      const isOpen = expandable && !!open[p.id];
                      const stored = Object.values(view?.apiKeys ?? {}).flat();
                      return (
                        <li key={p.id} className="typo-caption1_medium text-neutral-700">
                          <div className="flex justify-between gap-2">
                            <span className="inline-flex items-center gap-0.5">
                              {expandable && (
                                <button
                                  type="button"
                                  aria-label={t("usage.showKeys")}
                                  aria-expanded={isOpen}
                                  className="-ml-1 grid size-5 place-items-center rounded text-neutral-500 hover:bg-neutral-100"
                                  onClick={() => setOpen((o) => ({ ...o, [p.id]: !o[p.id] }))}
                                >
                                  <ChevronRight className={`size-3.5 transition-transform ${isOpen ? "rotate-90" : ""}`} aria-hidden="true" />
                                </button>
                              )}
                              {apiName(p.name)}
                            </span>
                            <span className="tabular-nums text-neutral-500">
                              {formatTokens(n)} {t("usage.tokens")} · {formatUsd(s ? s.usd : 0)}
                            </span>
                          </div>
                          <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-neutral-100">
                            <div className="h-full rounded-full bg-main-blue" style={{ width: `${total ? (n / total) * 100 : 0}%` }} />
                          </div>
                          {isOpen && (
                            <ul className="ml-3 mt-2 flex flex-col gap-2 border-l-2 border-neutral-100 pl-2.5">
                              {keys.map((c) => {
                                const meta = stored.find((k) => k.id === c.credential);
                                const known = meta || c.credential === "system" || c.credential === "default";
                                const cn = tokens(c);
                                return (
                                  <li key={c.credential}>
                                    <div className="flex justify-between gap-2">
                                      <span>
                                        {known ? credName(view, c.credential) : t("usage.deletedKey")}
                                        {meta && <span className="ml-1 font-mono text-neutral-400">…{meta.last4}</span>}
                                      </span>
                                      <span className="tabular-nums text-neutral-500">
                                        {formatTokens(cn)} {t("usage.tokens")} · {formatUsd(c.usd ?? 0)}
                                      </span>
                                    </div>
                                    <div className="mt-1 h-1 overflow-hidden rounded-full bg-neutral-100">
                                      <div className="h-full rounded-full bg-main-blue/50" style={{ width: `${total ? (cn / total) * 100 : 0}%` }} />
                                    </div>
                                    {limitOf(c.credential) !== null && (
                                      <div className="mt-1.5">
                                        <LimitMeter spend={limitView?.spend[`${p.id}:${c.credential}`] ?? 0} limit={limitOf(c.credential)} />
                                      </div>
                                    )}
                                  </li>
                                );
                              })}
                            </ul>
                          )}
                        </li>
                      );
                    })}
                  </ul>
                </div>
                <div>
                  <p className="mb-2 typo-caption1_semibold text-neutral-500">{t("usage.byWorkflow")}</p>
                  {sum.byWorkflow.length === 0 ? (
                    <p className="typo-caption1_regular text-neutral-400">{t("usage.noCalls")}</p>
                  ) : (
                    <ol className="flex flex-col gap-1.5">
                      {[...sum.byWorkflow]
                        .sort((a, b) => tokens(b) - tokens(a))
                        .slice(0, 3)
                        .map((w) => (
                          <li key={w.workflowId} className="flex justify-between gap-2 typo-caption1_medium text-neutral-700">
                            <span className="truncate">{name(w.workflowId)}</span>
                            <span className="shrink-0 tabular-nums text-neutral-500">
                              {formatTokens(tokens(w))} {t("usage.tokens")} · {formatUsd(w.usd)}
                            </span>
                          </li>
                        ))}
                    </ol>
                  )}
                </div>
              </div>
            </div>
          )}
          {sum && (
            <p className="px-5 pb-3 text-right typo-caption2_regular text-neutral-400">
              {t("usage.estimated")} · {t("usage.asOf").replace("{date}", sum.asOf)}
            </p>
          )}
        </section>
        <section aria-label={t("usage.subSection")} className="px-5 pt-3 pb-4">
          <div className="mb-3 flex h-8 items-center justify-between gap-3">
            <h4 className="shrink-0 whitespace-nowrap typo-caption1_semibold tracking-[.04em] text-main-gray">{t("usage.subSection")}</h4>
            <button
              type="button"
              aria-label={t("usage.refresh")}
              title={t("usage.refresh")}
              className="mr-auto -ml-2 grid size-7 shrink-0 place-items-center rounded-lg text-neutral-500 hover:bg-neutral-100"
              onClick={refreshNow}
            >
              <RefreshCw className="size-3.5" aria-hidden="true" />
            </button>
            {subs.length > 1 && current && (
              <Dropdown
                label={t("usage.subSection")}
                sel={current.sub.id}
                onSel={pick}
                items={subs.map(({ sub: s, w }) => ({
                  id: s.id,
                  name: s.name,
                  icon: <ProviderLogo id={s.logo} size={16} />,
                  note: <span className={`tabular-nums ${hot(usedOf(w)) || "text-neutral-500"}`}>{usedOf(w)}%</span>,
                }))}
              />
            )}
          </div>
          {current ? (
            <div role="group" aria-label={current.sub.name} className="rounded-lg border border-neutral-100 bg-main-light-blue/30 px-4 py-3.5">
              <div className="mb-3 flex items-center gap-2">
                <ProviderLogo id={current.sub.logo} size={18} />
                <span className="typo-body3_semibold text-main-deep-blue">{current.sub.name}</span>
                {current.plan.accountEmail && <span className="min-w-0 truncate typo-caption1_regular text-neutral-500">{current.plan.accountEmail}</span>}
                <span className="ml-auto shrink-0 typo-caption2_regular text-neutral-400">{t("usage.updated").replace("{time}", formatRelative(current.plan.checkedAt))}</span>
              </div>
              <div className="flex flex-col gap-3">
                {current.plan.windows.map((w) => (
                  <WindowMeter key={`${w.kind}:${w.label ?? ""}`} w={w} now={now} inline />
                ))}
              </div>
            </div>
          ) : (
            <p className="typo-caption1_regular text-neutral-500">
              {t("usage.noSubs")}{" "}
              <button type="button" className="typo-caption1_semibold text-main-blue hover:underline" onClick={onAccounts}>
                {t("usage.connectSubs")}
              </button>
            </p>
          )}
        </section>
      </div>
    </div>
  );
}
