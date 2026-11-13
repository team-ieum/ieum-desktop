import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type UsageSummary } from "../api";
import { ProviderLogo } from "../assets/providers";
import { t } from "../i18n";
import { useCredentials } from "../stores/credentials";
import { useDefaultPlans } from "../stores/plans";
import { formatReset, formatUsd } from "../usage/format";
import { hot, resetText, usedOf, usedText } from "../usage/parts";
import { connectedSubs, UsagePopover } from "./UsagePopover";

const REFRESH_MS = 60_000;

/** 창 맨 아래 줄 — "사용" + 구독 회사마다 [로고 N% 남은 시간](가장 빡빡한 창). 구독이 없으면 오늘 API 예상 금액. 누르면 사용량 팝오버. */
export function StatusBar({ onUsage, onSettings }: { onUsage: () => void; onSettings: () => void }) {
  const view = useCredentials((s) => s.view);
  const { plans, refreshNow } = useDefaultPlans(view);
  // undefined = 아직, null = 못 읽음
  const [today, setToday] = useState<UsageSummary | null | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const bar = useRef<HTMLElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const loadToday = () => api.usageSummary("today").then(setToday, () => setToday(null));
  useEffect(() => {
    useCredentials.getState().load().catch(() => {});
    void loadToday();
  }, []);

  // 구독은 useDefaultPlans가 처음 읽고, 이후엔 백그라운드 감시(plans-updated)가 채운다. 여기선 오늘 API 금액만 — 창이 안 보이면 멈추고, 다시 보이면 바로 읽는다.
  useEffect(() => {
    const load = () => {
      if (document.visibilityState === "visible") void loadToday();
    };
    const id = setInterval(load, REFRESH_MS);
    document.addEventListener("visibilitychange", load);
    return () => {
      clearInterval(id);
      document.removeEventListener("visibilitychange", load);
    };
  }, []);

  // 바깥 클릭·Esc로 닫는다. Esc면 연 버튼으로 포커스를 돌린다.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!bar.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      setOpen(false);
      opener.current?.focus();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const go = (f: () => void) => () => {
    setOpen(false);
    f();
  };

  const now = new Date();
  const subs = connectedSubs(view, plans);

  return (
    <footer ref={bar} className="relative flex h-7 shrink-0 items-center gap-1 border-t border-[#cde9f4] bg-main-light-blue/60 px-3 text-[12px] text-main-deep-blue">
      <button
        ref={opener}
        type="button"
        className="flex h-6 items-center gap-3 rounded-md px-1.5 tabular-nums hover:bg-white/70"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="font-semibold">{t("usage.barLabel")}</span>
        {subs.length
          ? subs.map(({ sub: s, w }) => {
              const pct = usedOf(w);
              return (
                <span key={s.id} title={`${s.name} ${usedText(pct)} · ${resetText(w, now)}`} className="inline-flex items-center gap-1">
                  <ProviderLogo id={s.logo} size={14} />
                  <span className={`font-semibold ${hot(pct)}`}>{pct}%</span>
                  <span className="text-main-deep-blue/70">{formatReset(w.resetsAt, now)}</span>
                </span>
              );
            })
          : today !== undefined && <span>{t("usage.todayApi").replace("{usd}", formatUsd(today?.totalUsd ?? null))}</span>}
      </button>
      <button
        type="button"
        aria-label={t("usage.refresh")}
        title={t("usage.refresh")}
        className="grid size-6 place-items-center rounded-md text-main-deep-blue/70 hover:bg-white/70"
        onClick={() => {
          refreshNow();
          void loadToday();
        }}
      >
        <RefreshCw className="size-3" aria-hidden="true" />
      </button>
      {open && <UsagePopover onUsage={go(onUsage)} onSettings={go(onSettings)} />}
    </footer>
  );
}
