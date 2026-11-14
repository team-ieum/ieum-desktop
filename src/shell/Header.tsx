import { PanelLeft, Search } from "lucide-react";
import { t } from "../i18n";
import { NotificationsMenu } from "./NotificationsMenu";
import { LIGHTS_PAD, TOP_ROW } from "./Sidebar";

/** 본문 위쪽 줄 — 현재 위치·찾기(⌘K)·알림 종·계정. 사이드바가 없으면 왼쪽에 신호등 자리(+펼치기). */
export function Header({
  crumb,
  email,
  onSearch,
  onOpenRun,
  inset,
  onExpand,
}: {
  crumb: string;
  email: string | null;
  onSearch: () => void;
  onOpenRun: (runId: string) => void;
  /** 창 왼쪽 끝에 붙어 신호등과 겹치는 자리. */
  inset?: boolean;
  onExpand?: () => void;
}) {
  return (
    <header
      data-tauri-drag-region
      className={`z-40 flex ${TOP_ROW} w-full shrink-0 items-center gap-3 border-b border-neutral-200 bg-white pr-4 ${inset ? LIGHTS_PAD : "pl-4"}`}
    >
      {onExpand && (
        <button
          type="button"
          onClick={onExpand}
          aria-label={t("nav.expand")}
          title={t("nav.expand")}
          className="-ml-1 grid size-7 shrink-0 place-items-center rounded-md text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-800"
        >
          <PanelLeft className="size-4" aria-hidden="true" />
        </button>
      )}
      <span data-testid="crumb" className="inline-flex h-7 items-center rounded-full bg-main-deep-blue px-3 text-xs font-semibold text-white">
        {crumb}
      </span>
      <div data-tauri-drag-region className="h-full flex-1" />
      <button
        type="button"
        className="inline-flex h-8 min-w-[220px] items-center gap-2.5 rounded-full border border-[#cde9f4] bg-white pr-2.5 pl-3.5 text-[13px] text-main-deep-blue hover:bg-main-light-blue/40"
        onClick={onSearch}
      >
        <Search className="size-[15px]" aria-hidden="true" />
        <span className="flex-1 text-left">{t("header.search")}</span>
        <span className="rounded-md border border-[#cde9f4] bg-white px-1.5 py-0.5 font-mono text-[11px] font-semibold opacity-80">⌘K</span>
      </button>
      <NotificationsMenu onOpenRun={onOpenRun} />
      {email && (
        <div
          role="img"
          aria-label={email}
          title={email}
          className="flex size-8 items-center justify-center rounded-full border-2 border-white bg-main-deep-blue text-[13px] font-bold text-white shadow-[0_0_0_2px_var(--color-sub-blue)]"
        >
          {email[0].toUpperCase()}
        </div>
      )}
    </header>
  );
}
