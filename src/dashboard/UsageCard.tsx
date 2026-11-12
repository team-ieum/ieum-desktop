import { useEffect, useState } from "react";
import { api, type UsageRange, type UsageSummary } from "../api";
import { t } from "../i18n";
import { PROVIDERS } from "../screens/settings/AiPane";
import { useWorkflows } from "../stores/workflows";
import { formatTokens, formatUsd } from "../usage/format";

const RANGES: UsageRange[] = ["today", "week", "month"];
const tokens = (x: { input: number; output: number }) => x.input + x.output;

/** 대시보드 "AI 사용량" — 내 PC AI 노드가 쓴 토큰·예상 금액. 웹 이음 탭에서는 그리지 않는다. */
export function UsageCard({ hidden }: { hidden: boolean }) {
  const [range, setRange] = useState<UsageRange>("today");
  // undefined = 읽는 중, null = 못 읽음
  const [sum, setSum] = useState<UsageSummary | null | undefined>(undefined);
  const workflows = useWorkflows((s) => s.list);

  useEffect(() => {
    if (hidden) return;
    let live = true;
    setSum(undefined);
    api.usageSummary(range).then(
      (s) => live && setSum(s),
      () => live && setSum(null),
    );
    return () => {
      live = false;
    };
  }, [hidden, range]);

  if (hidden) return null;
  const total = sum ? tokens({ input: sum.totalInput, output: sum.totalOutput }) : 0;
  const name = (id: string) => workflows.find((w) => w.id === id)?.name ?? t("history.deletedWorkflow");

  return (
    <div className="overflow-hidden rounded-brand-md border border-neutral-200 bg-neutral-white shadow-sm">
      <div className="flex items-center justify-between gap-4 border-b border-neutral-200 px-5 py-3.5">
        <h3 className="typo-body2_semibold text-neutral-900">{t("usage.title")}</h3>
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
        <div className="grid grid-cols-[minmax(0,0.8fr)_minmax(0,1fr)_minmax(0,1fr)] gap-6 px-5 py-4">
          <div>
            <p className="typo-caption1_semibold text-neutral-500">{t("usage.estimated")}</p>
            <p className="mt-1 text-[28px] font-bold tabular-nums text-main-deep-blue">{formatUsd(sum.totalUsd)}</p>
            <p className="typo-caption1_regular text-neutral-500">
              {formatTokens(total)} {t("usage.tokens")}
            </p>
          </div>
          <div>
            <p className="mb-2 typo-caption1_semibold text-neutral-500">{t("usage.bySource")}</p>
            <ul className="flex flex-col gap-2">
              {PROVIDERS.map((p) => {
                const s = sum.bySource.find((x) => x.source === p.id);
                const n = s ? tokens(s) : 0;
                return (
                  <li key={p.id} className="typo-caption1_medium text-neutral-700">
                    <div className="flex justify-between gap-2">
                      <span>{p.name}</span>
                      <span className="tabular-nums text-neutral-500">
                        {formatTokens(n)} · {formatUsd(s ? s.usd : 0)}
                      </span>
                    </div>
                    <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-neutral-100">
                      <div className="h-full rounded-full bg-main-blue" style={{ width: `${total ? (n / total) * 100 : 0}%` }} />
                    </div>
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
                        {formatTokens(tokens(w))} · {formatUsd(w.usd)}
                      </span>
                    </li>
                  ))}
              </ol>
            )}
          </div>
        </div>
      )}
      {sum && (
        <p className="px-5 pb-3 text-right typo-caption2_regular text-neutral-400">
          {t("usage.estimated")} · {t("usage.asOf").replace("{date}", sum.asOf)}
        </p>
      )}
    </div>
  );
}
