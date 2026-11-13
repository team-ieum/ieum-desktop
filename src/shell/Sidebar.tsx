import { CircleHelp, MoreHorizontal, PanelLeft, Search, Settings, SquarePen } from "lucide-react";
import { useState } from "react";
import { api, type WebWorkflowSummary, type WorkflowEntry } from "../api";
import symbol from "../assets/symbol.png";
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

const ROW = "flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] transition-colors";
const ROW_ON = "bg-main-light-blue font-semibold text-main-deep-blue";
const ROW_OFF = "text-neutral-700 hover:bg-neutral-200/70";
const ICON_BTN = "grid size-7 shrink-0 place-items-center rounded-md text-neutral-500 transition-colors hover:bg-neutral-200/70 hover:text-neutral-800";
const KBD = "font-mono text-[11px] text-neutral-400";

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

/** 사이드바를 통째로 숨겼는지 — [접힘, 뒤집기]. */
export function useSidebarCollapsed(): [boolean, () => void] {
  const [collapsed, setCollapsed] = useState(readCollapsed);
  return [
    collapsed,
    () => {
      writeCollapsed(!collapsed);
      setCollapsed(!collapsed);
    },
  ];
}

/** 첫 줄 높이 — 창 신호등(tauri.conf.json `trafficLightPosition`)이 이 줄 가운데에 온다. */
export const TOP_ROW = "h-12";
/** 신호등 자리(왼쪽 여백). */
export const LIGHTS_PAD = "pl-[84px]";

export function Sidebar({
  current,
  onNavigate,
  onNewCanvas,
  onSearch,
  onCollapse,
  recent,
  onOpenRecent,
  version,
}: {
  current: Page;
  onNavigate: (p: Page) => void;
  onNewCanvas: () => void;
  onSearch: () => void;
  onCollapse: () => void;
  recent: RecentItem[];
  onOpenRecent: (r: RecentItem) => void;
  version: string;
}) {
  return (
    <aside aria-label={t("nav.sidebar")} className="flex w-64 shrink-0 flex-col border-r border-neutral-200 bg-neutral-100">
      <div data-tauri-drag-region className={`flex ${TOP_ROW} shrink-0 items-center gap-2 pr-2.5 ${LIGHTS_PAD}`}>
        <img src={symbol} className="pointer-events-none h-[18px] w-[18px]" alt="" />
        <span className="pointer-events-none text-[15px] leading-none font-black tracking-wide text-main-deep-blue">{t("app.name")}</span>
        <div data-tauri-drag-region className="h-full flex-1" />
        <button type="button" onClick={onCollapse} className={ICON_BTN} aria-label={t("nav.collapse")} title={t("nav.collapse")}>
          <PanelLeft className="size-4" aria-hidden="true" />
        </button>
      </div>

      <div className="px-2.5 pt-1">
        <button
          type="button"
          onClick={onSearch}
          className="flex h-8 w-full items-center gap-2 rounded-lg bg-neutral-200/70 px-2.5 text-[13px] text-neutral-500 transition-colors hover:bg-neutral-200"
        >
          <Search className="size-[15px]" aria-hidden="true" />
          <span className="flex-1 text-left">{t("nav.search")}</span>
          <span className={KBD}>⌘K</span>
        </button>
      </div>

      <nav className="mt-2 flex flex-col gap-px px-2.5">
        <button type="button" onClick={onNewCanvas} className={`${ROW} ${ROW_OFF}`}>
          <SquarePen className="size-4" aria-hidden="true" />
          <span className="flex-1 text-left">{t("nav.newCanvas")}</span>
          <span className={KBD}>⌘N</span>
        </button>
        {NAV.filter((n) => n.page !== "settings").map(({ page, label, icon: Icon }) => (
          <button
            key={page}
            type="button"
            onClick={() => onNavigate(page)}
            aria-current={current === page ? "page" : undefined}
            className={`${ROW} ${current === page ? ROW_ON : ROW_OFF}`}
          >
            <Icon className="size-4" aria-hidden="true" />
            {t(label)}
          </button>
        ))}
      </nav>

      <div className="mt-5 flex min-h-0 flex-1 flex-col px-2.5">
        <div className="flex h-7 shrink-0 items-center justify-between pr-1 pl-2.5 text-[11px] font-medium text-neutral-500">
          {t("nav.recent")}
          <MoreHorizontal className="size-[14px]" aria-hidden="true" />
        </div>
        <div className="flex min-h-0 flex-col gap-px overflow-y-auto">
          {recent.length === 0 && <p className="px-2.5 py-2 text-[12px] text-neutral-500">{t("nav.recentEmpty")}</p>}
          {recent.map((r) => (
            <button
              key={`${r.kind}:${r.id}`}
              type="button"
              onClick={() => onOpenRecent(r)}
              className="flex items-start gap-2 rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-neutral-200/70"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-semibold text-neutral-800">{r.name}</span>
                <span className="mt-0.5 flex items-center gap-1 text-[11px] text-neutral-500">
                  <span>{r.kind === "local" ? t("nav.local") : t("nav.web")}</span>
                  <span aria-hidden="true">·</span>
                  <span className={`size-1.5 shrink-0 rounded-full ${DOT[r.dot]}`} />
                  <span>{t(`nav.dot.${r.dot}`)}</span>
                </span>
              </span>
              <span className="shrink-0 pt-0.5 text-[11px] text-neutral-400">{ago(r.at)}</span>
            </button>
          ))}
        </div>
      </div>

      {/* 창이 낮으면 숨긴다. */}
      <div className="relative mx-2.5 mb-2 shrink-0 overflow-hidden rounded-lg bg-main-deep-blue px-3 py-2.5 text-white [@media(max-height:820px)]:hidden">
        <div className="absolute -top-4 -right-4 h-16 w-16 rounded-full bg-sub-blue/35" />
        <div className="relative text-[13px] leading-snug whitespace-pre-line">{t("banner.tagline")}</div>
      </div>

      <div className="flex h-10 shrink-0 items-center gap-0.5 border-t border-neutral-200 px-2.5">
        <button type="button" onClick={() => onNavigate("settings")} className={ICON_BTN} aria-label={t("nav.settings")} title={t("nav.settings")}>
          <Settings className="size-4" aria-hidden="true" />
        </button>
        <button
          type="button"
          onClick={() => api.openHelp().catch((e: unknown) => console.error("[ieum] 도움말을 열지 못했어요", e))}
          className={ICON_BTN}
          aria-label={t("nav.help")}
          title={t("nav.help")}
        >
          <CircleHelp className="size-4" aria-hidden="true" />
        </button>
        <span className="ml-auto text-[11px] text-neutral-400">{version && `v${version}`}</span>
      </div>
    </aside>
  );
}
