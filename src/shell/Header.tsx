import { ChevronRight, Search } from "lucide-react";
import { useEffect, useState } from "react";
import symbol from "../assets/symbol.png";
import { t } from "../i18n";
import { NotificationsMenu } from "./NotificationsMenu";
import { SearchDialog, type SearchPick } from "./SearchDialog";

/** 위쪽 줄 — 로고·현재 위치·찾기(⌘K)·알림 종·계정. */
export function Header({
  crumb,
  email,
  onPick,
  onOpenRun,
}: {
  crumb: string;
  email: string | null;
  onPick: (item: SearchPick) => void;
  onOpenRun: (runId: string) => void;
}) {
  const [searching, setSearching] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSearching(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <header className="z-40 flex h-14 w-full shrink-0 items-center gap-3.5 border-b border-[#cde9f4] bg-linear-to-b from-[#EAF7FE] to-main-light-blue px-5">
      <div className="inline-flex h-10 items-center gap-2.5 rounded-xl px-2">
        <img src={symbol} className="h-6 w-6" alt="" />
        <span className="text-xl leading-none font-black tracking-wide text-main-deep-blue">{t("app.name")}</span>
      </div>
      <ChevronRight className="size-[15px] text-main-gray" aria-hidden="true" />
      <span data-testid="crumb" className="inline-flex h-7 items-center rounded-full bg-main-deep-blue px-3 text-xs font-semibold text-white">
        {crumb}
      </span>
      <div className="flex-1" />
      <button
        type="button"
        className="inline-flex h-9 min-w-[220px] items-center gap-2.5 rounded-full border border-[#cde9f4] bg-white/70 pr-2.5 pl-3.5 text-[13px] text-main-deep-blue hover:bg-white/90"
        onClick={() => setSearching(true)}
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
          className="flex h-9 w-9 items-center justify-center rounded-full border-2 border-white bg-main-deep-blue text-[13px] font-bold text-white shadow-[0_0_0_2px_var(--color-sub-blue)]"
        >
          {email[0].toUpperCase()}
        </div>
      )}
      <SearchDialog open={searching} onClose={() => setSearching(false)} onPick={onPick} />
    </header>
  );
}
