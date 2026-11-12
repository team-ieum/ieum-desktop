import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { api, type PlanUsage, type UsageSummary } from "../api";
import { t } from "../i18n";
import { apiName, leftText, PROVIDERS, SUBS, subName, subStateText, usePlans } from "../screens/settings/AiPane";
import { formatReset, formatUsd } from "../usage/format";

const REFRESH_MS = 60_000;
const UNAVAILABLE: PlanUsage = { state: "unavailable", windows: [], checkedAt: "", accountEmail: null };

/** 남은 비율 + 초기화까지 — "66% · 2일 10시간". */
export const windowText = (w: PlanUsage["windows"][number], now: Date) => `${Math.max(0, Math.round(100 - w.usedPct))}% · ${formatReset(w.resetsAt, now)}`;

/** 상태 줄 사용량 팝오버 — 공급자마다 한 줄, 새로 고침, 사용량 화면·AI 설정으로. */
function UsagePopover({ side, onUsage, onSettings }: { side: "left" | "right"; onUsage: () => void; onSettings: () => void }) {
  const { plans, refresh } = usePlans();
  // undefined = 읽는 중, null = 못 읽음
  const [month, setMonth] = useState<UsageSummary | null | undefined>(undefined);
  const first = useRef<HTMLButtonElement>(null);
  const loadMonth = () => api.usageSummary("month").then(setMonth, () => setMonth(null));
  useEffect(() => {
    void loadMonth();
    first.current?.focus();
  }, []);

  const now = new Date();
  const ROW = "flex justify-between gap-3 py-1 typo-caption1_medium";
  const LINK = "h-8 flex-1 rounded-lg px-2 typo-caption1_semibold text-main-deep-blue hover:bg-main-light-blue";
  return (
    <div
      role="dialog"
      aria-label={t("usage.title")}
      className={`absolute bottom-8 z-50 w-80 rounded-brand-md border border-[#cde9f4] bg-white p-3 text-neutral-800 shadow-[0_12px_32px_-12px_rgba(41,83,124,.35)] ${side === "left" ? "left-2" : "right-2"}`}
    >
      <div className="mb-1 flex items-center justify-between">
        <span className="typo-body3_semibold text-main-deep-blue">{t("usage.title")}</span>
        <button
          ref={first}
          type="button"
          className="inline-flex h-7 items-center gap-1 rounded-lg px-2 typo-caption1_semibold text-neutral-600 hover:bg-neutral-100"
          onClick={() => {
            refresh();
            void loadMonth();
          }}
        >
          <RefreshCw className="size-3.5" aria-hidden="true" />
          {t("usage.refresh")}
        </button>
      </div>
      <ul className="border-y border-neutral-100 py-1.5">
        {SUBS.map((s) => {
          const p = plans[s.id];
          const w = p?.state === "ok" ? p.windows[0] : undefined;
          return (
            <li key={s.id} className={ROW}>
              <span>{subName(s.name)}</span>
              <span className="tabular-nums text-neutral-500">{w ? leftText(w, now) : subStateText(p)}</span>
            </li>
          );
        })}
        {PROVIDERS.map((p) => {
          const usd = month ? (month.bySource.find((x) => x.source === p.id)?.usd ?? 0) : null;
          return (
            <li key={p.id} className={ROW}>
              <span>{apiName(p.name)}</span>
              <span className="tabular-nums text-neutral-500">
                {month === undefined ? t("common.loading") : t("usage.monthUsd").replace("{usd}", formatUsd(usd))}
              </span>
            </li>
          );
        })}
      </ul>
      <div className="mt-2 flex gap-1">
        <button type="button" className={LINK} onClick={onUsage}>
          {t("usage.details")}
        </button>
        <button type="button" className={LINK} onClick={onSettings}>
          {t("usage.openAiSettings")}
        </button>
      </div>
    </div>
  );
}

/** 창 맨 아래 줄 — Claude 구독 남은 비율(5시간 창)과 오늘 API 예상 금액. 누르면 사용량 팝오버. */
export function StatusBar({ onUsage, onSettings }: { onUsage: () => void; onSettings: () => void }) {
  const [plan, setPlan] = useState<PlanUsage | null>(null);
  // undefined = 아직, null = 못 읽음
  const [today, setToday] = useState<UsageSummary | null | undefined>(undefined);
  const [open, setOpen] = useState<"left" | "right" | null>(null);
  const bar = useRef<HTMLElement>(null);
  const opener = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    const load = () => {
      api.subscriptionUsage("claude", "system").then(setPlan, () => setPlan(UNAVAILABLE));
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

  // 바깥 클릭·Esc로 닫는다. Esc면 연 버튼으로 포커스를 돌린다.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: globalThis.MouseEvent) => {
      if (!bar.current?.contains(e.target as Node)) setOpen(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      setOpen(null);
      opener.current?.focus();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const toggle = (side: "left" | "right") => (e: MouseEvent<HTMLButtonElement>) => {
    opener.current = e.currentTarget;
    setOpen((o) => (o === side ? null : side));
  };
  const go = (f: () => void) => () => {
    setOpen(null);
    f();
  };

  const now = new Date();
  const five = plan?.windows.find((w) => w.kind === "five_hour");
  const week = plan?.windows.find((w) => w.kind === "seven_day");
  const claude =
    !plan || plan.state === "not_logged_in" ? null : plan.state === "ok" && five ? `Claude ${windowText(five, now)}` : t("usage.claudeUnavailable");

  return (
    <footer ref={bar} className="relative flex h-7 shrink-0 items-center justify-between gap-4 border-t border-[#cde9f4] bg-main-light-blue/60 px-4 text-[12px] text-main-deep-blue">
      {claude ? (
        <button
          type="button"
          className="tabular-nums hover:underline"
          title={week ? `${t("usage.window.seven_day")} ${windowText(week, now)}` : undefined}
          aria-haspopup="dialog"
          aria-expanded={open === "left"}
          onClick={toggle("left")}
        >
          {claude}
        </button>
      ) : (
        <span />
      )}
      {today !== undefined && (
        <button type="button" className="tabular-nums hover:underline" aria-haspopup="dialog" aria-expanded={open === "right"} onClick={toggle("right")}>
          {t("usage.todayApi").replace("{usd}", formatUsd(today?.totalUsd ?? null))}
        </button>
      )}
      {open && <UsagePopover side={open} onUsage={go(onUsage)} onSettings={go(onSettings)} />}
    </footer>
  );
}
