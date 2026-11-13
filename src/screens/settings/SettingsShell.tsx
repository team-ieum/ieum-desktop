import { ArrowLeft, BarChart3, Bell, Bot, CircleUserRound, Keyboard, Search, ShieldCheck, SlidersHorizontal, Wrench, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api, type UsageSummary } from "../../api";
import { UsageCard } from "../../dashboard/UsageCard";
import { t, type Key } from "../../i18n";
import { ROW, ROW_OFF, ROW_ON, SIDE, TOP_ROW } from "../../shell/Sidebar";
import { useCredentials } from "../../stores/credentials";
import { useWeb } from "../../stores/web";
import { AccountPane } from "./AccountPane";
import { AiProvidersPane, aiTexts, usePlans } from "./AiPane";
import { GeneralPane, generalTexts } from "./GeneralPane";
import { AdvancedPane, MacosPane, macosTexts, notificationTexts, NotificationsPane, ShortcutsPane, shortcutTexts, useAdvancedTexts } from "./PermissionsPane";
import { matches, QueryContext } from "./Row";

export type SettingsSection = "account" | "ai" | "usage" | "general" | "notifications" | "shortcuts" | "macos" | "advanced";
type Id = SettingsSection;

const SECTIONS: { id: Id; group: Key; label: Key; icon: LucideIcon; badge?: Key }[] = [
  { id: "account", group: "settings.groupAccount", label: "settings.account", icon: CircleUserRound },
  { id: "ai", group: "settings.groupAi", label: "settings.providers", icon: Bot, badge: "settings.optional" },
  { id: "usage", group: "settings.groupAi", label: "settings.usage", icon: BarChart3 },
  { id: "general", group: "settings.general", label: "settings.general", icon: SlidersHorizontal },
  { id: "notifications", group: "settings.general", label: "settings.notifications", icon: Bell },
  { id: "shortcuts", group: "settings.general", label: "settings.shortcuts", icon: Keyboard },
  { id: "macos", group: "settings.groupPrivacy", label: "settings.permissions", icon: ShieldCheck },
  { id: "advanced", group: "settings.groupAdvanced", label: "settings.advanced", icon: Wrench },
];
/** 자체 테두리 박스가 있는 페이지 — 나머지는 한 박스로 감싼다. */
const OWN_BOXES: Id[] = ["ai", "general", "usage"];
const USAGE_TEXTS = () => [t("usage.title"), t("usage.range.today"), t("usage.range.week"), t("usage.range.month"), t("usage.estimated"), t("usage.bySource"), t("usage.byWorkflow")];
const KEYCAP = "grid h-5 min-w-5 place-items-center rounded border border-[#cde9f4] bg-main-light-blue px-1 font-mono text-[11px] font-semibold text-main-deep-blue/70";

/** 설정 페이지 — 셸 사이드바 대신 설정 전용 사이드바(돌아가기·검색·묶음 메뉴)와 본문. `header`는 본문 위 앱 머리줄. */
export function SettingsShell({ onBack, version, initial = "account", header }: { onBack: () => void; version: string; initial?: Id; header?: ReactNode }) {
  const account = useWeb((s) => s.account);
  const expired = useWeb((s) => s.expired);
  const view = useCredentials((s) => s.view);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<Id>(initial);
  const { plans, refresh: refreshPlan } = usePlans(view);
  // undefined = 읽는 중, null = 못 읽음(Rust 명령이 아직 없을 때도)
  const [usage, setUsage] = useState<UsageSummary | null | undefined>(undefined);
  const search = useRef<HTMLInputElement>(null);
  const advancedTexts = useAdvancedTexts();

  const refresh = useCallback(() => {
    useCredentials.getState().load().catch((e) => setLoadError(String(e)));
    api.usageSummary("month").then(setUsage, () => setUsage(null));
  }, []);
  useEffect(refresh, [refresh]);

  // ⌘F = 설정 검색
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        search.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 줄 이름·설명·버튼 문구 — Row가 숨기는 기준과 같다. 메뉴 이름이 맞으면 그 페이지의 줄은 다 보이고,
  // 줄이 하나도 안 남는 메뉴는 목록에서 뺀다.
  const texts: Record<Id, string[]> = {
    account: account ? [account.email, t("settings.loggedInAs"), t("settings.logout"), ...(expired ? [t("settings.sessionExpired"), t("settings.reloginHint"), t("settings.relogin")] : [])] : [],
    ai: aiTexts(plans, view),
    usage: USAGE_TEXTS(),
    general: generalTexts(version),
    notifications: notificationTexts(),
    shortcuts: shortcutTexts(),
    macos: macosTexts(),
    advanced: advancedTexts,
  };
  const titleHit = (s: (typeof SECTIONS)[number]) => matches(q, t(s.label));
  const visible = SECTIONS.filter((s) => titleHit(s) || texts[s.id].some((x) => matches(q, x)));
  // 메뉴마다 한 페이지. 검색으로 고른 메뉴가 숨으면 남은 첫 메뉴의 페이지(검색어를 지우면 고른 페이지로).
  const shown = visible.find((s) => s.id === sel) ?? visible[0];

  const panes: Record<Id, ReactNode> = {
    account: <AccountPane />,
    ai: <AiProvidersPane view={view} plans={plans} refreshPlan={refreshPlan} refreshCreds={refresh} usage={usage} />,
    usage: <UsageCard hidden={false} />,
    general: <GeneralPane version={version} />,
    notifications: <NotificationsPane />,
    shortcuts: <ShortcutsPane />,
    macos: <MacosPane />,
    advanced: <AdvancedPane />,
  };

  return (
    <div className="flex min-h-0 flex-1">
      <aside aria-label={t("settings.title")} className={`flex w-64 shrink-0 flex-col ${SIDE}`}>
        {/* 창 신호등 자리 — 앱 사이드바 첫 줄과 같은 높이, 끌어서 창을 옮긴다. */}
        <div data-tauri-drag-region className={`${TOP_ROW} shrink-0`} />
        <div className="px-2.5">
          <button type="button" onClick={onBack} className={`${ROW} ${ROW_OFF}`}>
            <ArrowLeft className="size-4" aria-hidden="true" />
            {t("settings.back")}
          </button>
        </div>
        <div className="mx-2.5 my-2 border-t border-[#cde9f4]" />
        <div className="px-2.5">
          <label className="flex h-8 items-center gap-2 rounded-lg bg-white px-2.5 text-[13px] ring-1 ring-[#cde9f4] focus-within:ring-main-blue">
            <Search className="size-[15px] shrink-0 text-main-deep-blue/60" aria-hidden="true" />
            <input
              ref={search}
              aria-label={t("settings.search")}
              placeholder={t("settings.search")}
              value={q}
              onChange={(e) => setQ(e.target.value)}
              className="min-w-0 flex-1 bg-transparent text-neutral-800 outline-none placeholder:text-main-deep-blue/50"
            />
            <span className="flex gap-0.5" aria-hidden="true">
              <span className={KEYCAP}>⌘</span>
              <span className={KEYCAP}>F</span>
            </span>
          </label>
        </div>
        <div className="mx-2.5 mt-2.5 border-t border-[#cde9f4]" />
        <nav className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto px-2.5 pb-3">
          {visible.map((s, i) => (
            <div key={s.id} className="contents">
              {visible[i - 1]?.group !== s.group && (
                <div data-group className="px-2.5 pt-5 pb-1.5 text-[11px] font-semibold tracking-[0.2em] text-main-gray">
                  {t(s.group)}
                </div>
              )}
              <button
                type="button"
                aria-label={t(s.label)}
                onClick={() => setSel(s.id)}
                aria-current={shown?.id === s.id ? "true" : undefined}
                className={`${ROW} ${shown?.id === s.id ? ROW_ON : ROW_OFF}`}
              >
                <s.icon className="size-4 shrink-0" aria-hidden="true" />
                <span className="flex-1 truncate text-left">{t(s.label)}</span>
                {s.badge && <span className={`shrink-0 text-[11px] font-normal ${shown?.id === s.id ? "text-white/70" : "text-main-gray/80"}`}>{t(s.badge)}</span>}
              </button>
            </div>
          ))}
        </nav>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        {header}
        <main className="flex min-h-0 flex-1 flex-col bg-white text-neutral-800">
          <div className="flex h-12 shrink-0 items-center gap-1.5 border-b border-neutral-200 px-8 text-sm text-neutral-500">
            {t("settings.title")}
            {shown && (
              <>
                <span className="text-neutral-300">/</span>
                <span data-testid="settings-crumb" className="font-semibold text-main-deep-blue">
                  {t(shown.label)}
                </span>
              </>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            <div className="mx-auto max-w-[640px] pt-6 pb-10">
              {loadError && <p className="mb-4 typo-caption1_semibold text-danger-700">{loadError}</p>}
              {shown ? (
                <section key={shown.id} id={`settings-${shown.id}`}>
                  <h2 className="mb-4 typo-title3_bold text-main-deep-blue">{t(shown.label)}</h2>
                  <QueryContext.Provider value={titleHit(shown) ? "" : q}>
                    {OWN_BOXES.includes(shown.id) ? panes[shown.id] : <div className="rounded-brand-md border border-[#cde9f4] bg-white px-5">{panes[shown.id]}</div>}
                  </QueryContext.Provider>
                </section>
              ) : (
                <p className="typo-body3_regular text-neutral-500">{t("settings.noMatch")}</p>
              )}
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}
