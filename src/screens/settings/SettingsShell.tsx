import { ArrowLeft, Bot, CircleUserRound, Search, ShieldCheck, SlidersHorizontal, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api, keyStates, type Providers, type UsageSummary } from "../../api";
import { t, type Key } from "../../i18n";
import { useCredentials } from "../../stores/credentials";
import { useWeb } from "../../stores/web";
import { AccountPane, serverHost } from "./AccountPane";
import { AiProvidersPane, aiTexts, usePlans } from "./AiPane";
import { GeneralPane } from "./GeneralPane";
import { aiStatus, PermissionsPane } from "./PermissionsPane";
import { matches, QueryContext } from "./Row";

export type SettingsSection = "account" | "general" | "permissions" | "ai";
type Id = SettingsSection;

const SECTIONS: { id: Id; group: Key; label: Key; icon: LucideIcon }[] = [
  { id: "account", group: "settings.groupAccount", label: "settings.account", icon: CircleUserRound },
  { id: "general", group: "settings.general", label: "settings.general", icon: SlidersHorizontal },
  { id: "permissions", group: "settings.general", label: "settings.permissions", icon: ShieldCheck },
  { id: "ai", group: "settings.ai", label: "settings.cli", icon: Bot },
];

const NAV_ON = "bg-main-deep-blue font-semibold text-white shadow-[0_4px_12px_-4px_rgba(41,83,124,.4)]";
const NAV_OFF = "font-normal text-main-deep-blue hover:bg-white/50 [&_svg]:opacity-70";

/** 설정 페이지 — 셸 사이드바 대신 설정 전용 사이드바(돌아가기·검색·묶음 메뉴)와 본문. */
export function SettingsShell({ onBack, version, initial = "account" }: { onBack: () => void; version: string; initial?: Id }) {
  const account = useWeb((s) => s.account);
  const [providers, setProviders] = useState<Providers | null>(null);
  const view = useCredentials((s) => s.view);
  const keychain = view ? keyStates(view) : null;
  const [loadError, setLoadError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<Id>(initial);
  const { plans, refresh: refreshPlan } = usePlans(view);
  // undefined = 읽는 중, null = 못 읽음(Rust 명령이 아직 없을 때도)
  const [usage, setUsage] = useState<UsageSummary | null | undefined>(undefined);
  const search = useRef<HTMLInputElement>(null);

  const refreshCreds = useCallback(() => {
    useCredentials.getState().load().catch((e) => setLoadError(String(e)));
  }, []);
  const refresh = useCallback(() => {
    api.detectProviders().then(setProviders, (e) => setLoadError(String(e)));
    refreshCreds();
    api.usageSummary("month").then(setUsage, () => setUsage(null));
  }, [refreshCreds]);
  useEffect(refresh, [refresh]);
  // 상태 줄에서 들어오면 그 묶음으로.
  useEffect(() => {
    if (initial !== "account") document.getElementById(`settings-${initial}`)?.scrollIntoView?.({ block: "start" });
  }, [initial]);

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

  // 줄 이름·설명·버튼 문구 — Row가 숨기는 기준과 같다. 묶음 이름이 맞으면 그 묶음의 줄은 다 보이고,
  // 줄이 하나도 안 남는 묶음은 메뉴·본문에서 뺀다.
  const texts: Record<Id, string[]> = {
    account: account ? [account.email, `${t("settings.loggedInAs")} · ${serverHost(account.serverUrl)}`, t("settings.logout")] : [],
    general: [t("settings.language"), t("settings.lang.system"), t("settings.lang.ko"), t("settings.lang.en")],
    permissions: [
      t("settings.filesHint"),
      t("settings.notifyTest"),
      t("settings.notifyTestDesc"),
      t("settings.notifyTestSend"),
      t("onboarding.openSettings"),
      t("settings.notifyManualStart"),
      t("settings.notifyManualEnd"),
      t("settings.notifyWatchEnd"),
      t("settings.autostart"),
      t("settings.autostartDesc"),
      t("settings.loginItems"),
      t("settings.aiStatus"),
      aiStatus(providers, keychain),
      t("settings.toAi"),
      t("settings.replayOnboarding"),
      t("settings.replayOnboardingDesc"),
    ],
    ai: aiTexts(plans, view),
  };
  const titleHit = (s: (typeof SECTIONS)[number]) => matches(q, t(s.label));
  const visible = SECTIONS.filter((s) => titleHit(s) || texts[s.id].some((x) => matches(q, x)));
  // 검색으로 선택한 묶음이 숨으면 위 경로는 남은 첫 묶음을 가리킨다.
  const shown = visible.find((s) => s.id === sel) ?? visible[0];

  const pick = (id: Id) => {
    setSel(id);
    document.getElementById(`settings-${id}`)?.scrollIntoView?.({ behavior: "smooth", block: "start" });
  };

  const panes: Record<Id, ReactNode> = {
    account: <AccountPane />,
    general: <GeneralPane />,
    permissions: <PermissionsPane providers={providers} keychain={keychain} onAi={() => pick("ai")} />,
    ai: <AiProvidersPane view={view} plans={plans} refreshPlan={refreshPlan} refreshCreds={refresh} usage={usage} />,
  };

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-72 shrink-0 flex-col border-r border-[#cde9f4] bg-main-light-blue px-3 py-3.5">
        <button type="button" onClick={onBack} className="flex h-10 items-center gap-2 rounded-xl px-3.5 text-sm font-semibold text-main-deep-blue hover:bg-white/50">
          <ArrowLeft className="size-4" aria-hidden="true" />
          {t("settings.back")}
        </button>
        <label className="mt-2 flex h-10 items-center gap-2.5 rounded-xl border border-[#cde9f4] bg-white px-3 text-sm focus-within:border-main-blue">
          <Search className="size-[15px] text-main-deep-blue opacity-70" aria-hidden="true" />
          <input
            ref={search}
            aria-label={t("settings.search")}
            placeholder={t("settings.search")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="min-w-0 flex-1 bg-transparent text-neutral-800 outline-none placeholder:text-neutral-400"
          />
          <span className="rounded-md border border-[#cde9f4] bg-white px-1.5 py-0.5 font-mono text-[11px] font-semibold text-main-deep-blue opacity-80">⌘F</span>
        </label>
        <nav className="mt-2 flex flex-col gap-0.5">
          {visible.map((s, i) => (
            <div key={s.id} className="contents">
              {visible[i - 1]?.group !== s.group && (
                <div className="px-3.5 pt-4 pb-1.5 text-[11px] font-semibold tracking-[.08em] text-main-gray uppercase">{t(s.group)}</div>
              )}
              <button
                type="button"
                onClick={() => pick(s.id)}
                aria-current={shown?.id === s.id ? "true" : undefined}
                className={`flex h-10 w-full items-center gap-2.5 rounded-xl px-3.5 text-sm transition-colors ${shown?.id === s.id ? NAV_ON : NAV_OFF}`}
              >
                <s.icon className="size-[18px]" aria-hidden="true" />
                {t(s.label)}
              </button>
            </div>
          ))}
        </nav>
        <div className="flex-1" />
        <div className="px-3.5 pb-1 text-[11px] font-medium text-main-gray">IEUM desktop{version && ` · v${version}`}</div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col bg-white text-neutral-800">
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
            {visible.length === 0 && <p className="typo-body3_regular text-neutral-500">{t("settings.noMatch")}</p>}
            {visible.map((s) => (
              <section key={s.id} id={`settings-${s.id}`} className="mb-6 scroll-mt-6">
                <h3 className="mb-2.5 typo-body1_bold text-main-deep-blue">{t(s.label)}</h3>
                <QueryContext.Provider value={titleHit(s) ? "" : q}>
                  {/* AI 제공자는 카드마다 테두리가 있다. */}
                  {s.id === "ai" ? panes.ai : <div className="rounded-brand-md border border-[#cde9f4] bg-white px-5">{panes[s.id]}</div>}
                </QueryContext.Provider>
              </section>
            ))}
          </div>
        </div>
      </main>
    </div>
  );
}
