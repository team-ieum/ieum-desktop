import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { api, type PlanUsage, type UsageSummary } from "../api";
import { ProviderLogo } from "../assets/providers";
import { t } from "../i18n";
import { accountsOf, BOXES, credLabel, leftText, planKey, subStateText, SYSTEM, usePlans } from "../screens/settings/AiPane";
import { useCredentials } from "../stores/credentials";
import { formatReset, formatUsd } from "../usage/format";

const REFRESH_MS = 60_000;
const UNAVAILABLE: PlanUsage = { state: "unavailable", windows: [], checkedAt: "", accountEmail: null };
const NOT_LOGGED_IN: PlanUsage = { state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null };

/** "구독"·"API 키" + 기본 키·계정 이름표(이 Mac의 로그인·옛 키면 생략). */
function Name({ name, m }: { name: string; m?: { id: string; label: string } }) {
  return (
    <span className="min-w-0 truncate pl-[22px] text-neutral-600">
      {name}
      {m && m.id !== SYSTEM && m.id !== "default" && <span className="text-neutral-400"> · {credLabel(m)}</span>}
    </span>
  );
}

/** 남은 비율 + 초기화까지 — "66% · 2일 10시간". */
export const windowText = (w: PlanUsage["windows"][number], now: Date) => `${Math.max(0, Math.round(100 - w.usedPct))}% · ${formatReset(w.resetsAt, now)}`;

/** 상태 줄 사용량 팝오버 — 회사 박스마다 구독 기본·API 기본 한 줄씩, 새로 고침, 사용량 화면·AI 설정으로. */
function UsagePopover({ side, onUsage, onSettings }: { side: "left" | "right"; onUsage: () => void; onSettings: () => void }) {
  const view = useCredentials((s) => s.view);
  // 구독은 기본 계정만.
  const { plans, refresh } = usePlans(view, true);
  // undefined = 읽는 중, null = 못 읽음
  const [month, setMonth] = useState<UsageSummary | null | undefined>(undefined);
  const first = useRef<HTMLButtonElement>(null);
  const loadMonth = () => api.usageSummary("month").then(setMonth, () => setMonth(null));
  useEffect(() => {
    void loadMonth();
    useCredentials.getState().load().catch(() => {});
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
        {BOXES.map(({ id, name, logo, sub: s, api: p }) => {
          const subDef = s.id === "antigravity" ? undefined : accountsOf(view, s.id).find((m) => m.isDefault);
          const plan = plans[planKey(s.id, subDef?.id)];
          const w = plan?.state === "ok" ? plan.windows[0] : undefined;
          const keyDef = view?.apiKeys[p.id]?.find((k) => k.isDefault);
          // 목록을 못 읽었으면 공급원 전체.
          const usd = month
            ? ((keyDef ? month.byCredential.find((x) => x.source === p.id && x.credential === keyDef.id) : month.bySource.find((x) => x.source === p.id))?.usd ?? 0)
            : null;
          return (
            <li key={id} aria-label={name} className="py-1">
              <span className="flex items-center gap-1.5 typo-caption1_semibold">
                <ProviderLogo id={logo} size={16} />
                {name}
              </span>
              {/* 계정이 하나도 없으면(이 Mac의 로그인을 숨김) 구독 줄을 뺀다. */}
              {(s.id === "antigravity" || subDef) && (
                <div className={ROW}>
                  <Name name={t("settings.subscription")} m={subDef} />
                  <span className="shrink-0 tabular-nums text-neutral-500">{w ? leftText(w, now) : subStateText(plan)}</span>
                </div>
              )}
              <div className={ROW}>
                <Name name={t("settings.apiKey")} m={keyDef} />
                <span className="shrink-0 tabular-nums text-neutral-500">
                  {month === undefined ? t("common.loading") : t("usage.monthUsd").replace("{usd}", formatUsd(usd))}
                </span>
              </div>
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
  // 기본 Claude 계정 — 목록을 못 읽으면 이 Mac의 로그인, 계정이 하나도 없으면 null(숨김).
  const view = useCredentials((s) => s.view);
  const claudeId = view ? (view.claudeAccounts.find((m) => m.isDefault)?.id ?? null) : SYSTEM;
  useEffect(() => {
    useCredentials.getState().load().catch(() => {});
  }, []);

  useEffect(() => {
    const load = () => {
      (claudeId ? api.subscriptionUsage("claude", claudeId) : Promise.resolve(NOT_LOGGED_IN)).then(setPlan, () => setPlan(UNAVAILABLE));
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
  }, [claudeId]);

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
