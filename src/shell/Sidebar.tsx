import { ChevronLeft, ChevronRight, CircleHelp, Globe, Laptop, MoreHorizontal, Settings, Sparkles } from "lucide-react";
import { useState } from "react";
import { api, type WebWorkflowSummary, type WorkflowEntry } from "../api";
import { locale, t } from "../i18n";
import { NAV, type Page } from "./nav";

export type RecentItem = { kind: "local" | "web"; id: string; name: string; at: string; dot: "active" | "paused" | "error" };

const time = (iso: string) => {
  const n = Date.parse(iso);
  return Number.isNaN(n) ? 0 : n;
};

/** 최근 작업 5개 — 내 PC·웹 이음을 수정 시각 내림차순으로 섞는다. */
export function recentItems(local: WorkflowEntry[], web: WebWorkflowSummary[]): RecentItem[] {
  return [
    ...local.map((w): RecentItem => ({
      kind: "local",
      id: w.id,
      name: w.name,
      at: w.updatedAt,
      dot: w.watch === "watch_error" ? "error" : w.enabled ? "active" : "paused",
    })),
    ...web.map((w): RecentItem => ({ kind: "web", id: w.id, name: w.name, at: w.updatedAt ?? "", dot: w.active ? "active" : "paused" })),
  ]
    .sort((a, b) => time(b.at) - time(a.at))
    .slice(0, 5);
}

const DOT = { active: "bg-[#006A4E]", paused: "bg-[#959595]", error: "bg-[#EC2D30]" };

/** "방금"·"4시간 전"·"어제" */
export function ago(iso: string): string {
  const ms = time(iso);
  if (!ms) return "";
  const rtf = new Intl.RelativeTimeFormat(locale(), { numeric: "auto" });
  const min = Math.round((ms - Date.now()) / 60000);
  if (min > -1) return rtf.format(0, "second");
  if (min > -60) return rtf.format(min, "minute");
  if (min > -60 * 24) return rtf.format(Math.round(min / 60), "hour");
  return rtf.format(Math.round(min / 1440), "day");
}

const NAV_ON = "bg-main-deep-blue font-semibold text-white shadow-[0_4px_12px_-4px_rgba(41,83,124,.4)]";
const NAV_OFF = "font-normal text-main-deep-blue hover:bg-white/50 [&_svg]:opacity-70";
const FOOT = "flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px] border border-[#cde9f4] bg-white text-main-deep-blue hover:bg-neutral-50";
const BADGE = "inline-flex items-center gap-1 rounded-full bg-white px-1.5 text-[10px] font-bold ring-1 ring-[#cde9f4]";

// 접힌 상태는 이 PC에만 기억 — 저장소가 막혀도 펼친 상태로 동작한다.
const COLLAPSED_KEY = "ieum.sidebarCollapsed";
const readCollapsed = () => {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
};
const writeCollapsed = (c: boolean) => {
  try {
    localStorage.setItem(COLLAPSED_KEY, c ? "1" : "0");
  } catch {
    // 기억 못 해도 화면은 그대로.
  }
};

export function Sidebar({
  current,
  onNavigate,
  onNewCanvas,
  recent,
  onOpenRecent,
  version,
}: {
  current: Page;
  onNavigate: (p: Page) => void;
  onNewCanvas: () => void;
  recent: RecentItem[];
  onOpenRecent: (r: RecentItem) => void;
  version: string;
}) {
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const onToggleCollapse = () => {
    writeCollapsed(!collapsed);
    setCollapsed(!collapsed);
  };
  return (
    <aside className={`flex shrink-0 flex-col border-r border-[#cde9f4] bg-main-light-blue ${collapsed ? "w-16" : "w-64"}`}>
      <div className="p-3.5">
        <button
          type="button"
          onClick={onNewCanvas}
          aria-label={collapsed ? t("nav.newCanvas") : undefined}
          title={collapsed ? t("nav.newCanvas") : undefined}
          className={`flex h-[42px] w-full items-center rounded-xl bg-main-deep-blue text-sm font-semibold text-white shadow-[0_4px_12px_-4px_rgba(41,83,124,.5)] hover:bg-main-deep-blue/90 ${
            collapsed ? "justify-center" : "justify-between px-3.5"
          }`}
        >
          <span className="flex items-center gap-2.5">
            <Sparkles className="size-4" aria-hidden="true" />
            {!collapsed && t("nav.newCanvas")}
          </span>
          {!collapsed && <span className="rounded-md bg-white/12 px-1.5 py-0.5 font-mono text-[11px] font-medium opacity-70">⌘N</span>}
        </button>
      </div>

      <nav className="flex flex-col gap-0.5 px-3">
        {NAV.map(({ page, label, icon: Icon }) => (
          <button
            key={page}
            type="button"
            onClick={() => onNavigate(page)}
            aria-current={current === page ? "page" : undefined}
            aria-label={collapsed ? t(label) : undefined}
            title={collapsed ? t(label) : undefined}
            className={`flex h-10 w-full items-center gap-2.5 rounded-xl text-sm transition-colors ${collapsed ? "justify-center" : "px-3.5"} ${
              current === page ? NAV_ON : NAV_OFF
            }`}
          >
            <Icon className="size-[18px]" aria-hidden="true" />
            {!collapsed && t(label)}
          </button>
        ))}
      </nav>

      {!collapsed && (
        <div className="mt-5 flex min-h-0 flex-1 flex-col px-3">
          <div className="flex shrink-0 items-center justify-between px-3 pb-1.5 text-[11px] font-semibold tracking-[.08em] text-main-gray uppercase">
            {t("nav.recent")}
            <MoreHorizontal className="size-[13px]" aria-hidden="true" />
          </div>
          <div className="flex min-h-0 flex-col gap-0.5 overflow-y-auto">
            {recent.length === 0 && <p className="px-3 py-2 text-[12px] text-neutral-500">{t("nav.recentEmpty")}</p>}
            {recent.map((r, i) => (
              <button
                key={`${r.kind}:${r.id}`}
                type="button"
                onClick={() => onOpenRecent(r)}
                className={`flex items-center gap-2.5 rounded-[10px] border px-3 py-2 text-left transition-colors hover:bg-white/50 ${
                  i === 0 ? "border-[#cde9f4] bg-white/70" : "border-transparent"
                }`}
              >
                <span className={`h-2 w-2 shrink-0 rounded-full ${DOT[r.dot]}`} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] font-semibold text-neutral-800">{r.name}</span>
                  <span className="mt-0.5 flex items-center gap-1.5 text-[11px] text-neutral-500">
                    {ago(r.at)}
                    {r.kind === "local" ? (
                      <span className={`${BADGE} text-main-deep-blue`}>
                        <Laptop className="size-2.5" aria-hidden="true" />
                        {t("nav.local")}
                      </span>
                    ) : (
                      <span className={`${BADGE} text-main-blue`}>
                        <Globe className="size-2.5" aria-hidden="true" />
                        {t("nav.badgeWeb")}
                      </span>
                    )}
                  </span>
                </span>
                <span className="sr-only">{t(`nav.dot.${r.dot}`)}</span>
              </button>
            ))}
          </div>
        </div>
      )}

      {collapsed && <div className="flex-1" />}

      {!collapsed && (
        <div className="relative m-3 shrink-0 overflow-hidden rounded-xl bg-main-deep-blue p-3.5 text-white">
          <div className="absolute -top-5 -right-5 h-[90px] w-[90px] rounded-full bg-sub-blue/35" />
          <div className="absolute right-[18px] -bottom-[18px] h-[50px] w-[50px] rounded-full bg-node-yellow/50" />
          <div className="relative text-lg leading-snug whitespace-pre-line">{t("banner.tagline")}</div>
          <div className="relative mt-2.5 text-[11px] font-medium opacity-75">IEUM desktop{version && ` · v${version}`}</div>
        </div>
      )}

      <div className={`flex shrink-0 gap-1.5 border-t border-[#cde9f4] bg-white/40 p-3 ${collapsed ? "flex-col items-center" : ""}`}>
        {!collapsed && (
          <button
            type="button"
            onClick={() => api.openHelp().catch((e: unknown) => console.error("[ieum] 도움말을 열지 못했어요", e))}
            className="inline-flex h-9 flex-1 items-center justify-center gap-1.5 rounded-[10px] border border-[#cde9f4] bg-white text-sm font-semibold text-main-deep-blue hover:bg-neutral-50"
          >
            <CircleHelp className="size-[15px]" aria-hidden="true" />
            {t("nav.help")}
          </button>
        )}
        <button type="button" onClick={() => onNavigate("settings")} className={FOOT} aria-label={t("nav.settings")} title={t("nav.settings")}>
          <Settings className="size-4" aria-hidden="true" />
        </button>
        <button
          type="button"
          onClick={onToggleCollapse}
          className={FOOT}
          aria-label={collapsed ? t("nav.expand") : t("nav.collapse")}
          title={collapsed ? t("nav.expand") : t("nav.collapse")}
        >
          {collapsed ? <ChevronRight className="size-4" aria-hidden="true" /> : <ChevronLeft className="size-4" aria-hidden="true" />}
        </button>
      </div>
    </aside>
  );
}
