import { ChevronRight } from "lucide-react";
import symbol from "../assets/symbol.png";
import { t } from "../i18n";

/** 위쪽 줄 — 로고·현재 위치·계정. 검색(⌘K)·알림 종은 Task 13에서 동작과 함께 붙인다. */
export function Header({ crumb, email }: { crumb: string; email: string | null }) {
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
    </header>
  );
}
