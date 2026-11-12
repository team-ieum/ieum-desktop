import { useEffect, useState } from "react";
import { api, type PlanUsage, type UsageSummary } from "../api";
import { t } from "../i18n";
import { formatReset, formatUsd } from "../usage/format";

const REFRESH_MS = 60_000;
const UNAVAILABLE: PlanUsage = { state: "unavailable", windows: [], checkedAt: "" };

/** 남은 비율 + 초기화까지 — "66% · 2일 10시간". */
export const windowText = (w: PlanUsage["windows"][number], now: Date) => `${Math.max(0, Math.round(100 - w.usedPct))}% · ${formatReset(w.resetsAt, now)}`;

/** 창 맨 아래 줄 — Claude 구독 남은 비율(5시간 창)과 오늘 API 예상 금액. 누르면 설정의 그 메뉴. */
export function StatusBar({ onOpen }: { onOpen: (section: "providers" | "keys") => void }) {
  const [plan, setPlan] = useState<PlanUsage | null>(null);
  // undefined = 아직, null = 못 읽음
  const [today, setToday] = useState<UsageSummary | null | undefined>(undefined);

  useEffect(() => {
    const load = () => {
      api.claudePlanUsage().then(setPlan, () => setPlan(UNAVAILABLE));
      api.usageSummary("today").then(setToday, () => setToday(null));
    };
    load();
    // 창이 안 보이면 멈춘다 — 다시 보이면 바로 새로 읽는다.
    const id = setInterval(() => document.visibilityState === "visible" && load(), REFRESH_MS);
    const onVis = () => document.visibilityState === "visible" && load();
    document.addEventListener("visibilitychange", onVis);
    return () => {
      clearInterval(id);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, []);

  const now = new Date();
  const five = plan?.windows.find((w) => w.kind === "five_hour");
  const week = plan?.windows.find((w) => w.kind === "seven_day");
  const claude =
    !plan || plan.state === "not_logged_in" ? null : plan.state === "ok" && five ? `Claude ${windowText(five, now)}` : t("usage.claudeUnavailable");

  return (
    <footer className="flex h-7 shrink-0 items-center justify-between gap-4 border-t border-[#cde9f4] bg-main-light-blue/60 px-4 text-[12px] text-main-deep-blue">
      {claude ? (
        <button
          type="button"
          className="tabular-nums hover:underline"
          title={week ? `${t("usage.window.seven_day")} ${windowText(week, now)}` : undefined}
          onClick={() => onOpen("providers")}
        >
          {claude}
        </button>
      ) : (
        <span />
      )}
      {today !== undefined && (
        <button type="button" className="tabular-nums hover:underline" onClick={() => onOpen("keys")}>
          {t("usage.todayApi").replace("{usd}", formatUsd(today?.totalUsd ?? null))}
        </button>
      )}
    </footer>
  );
}
