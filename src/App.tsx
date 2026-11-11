import { Globe, History as HistoryIcon, Settings as SettingsIcon, Workflow as WorkflowIcon, X, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { t, type Key } from "./i18n";
import { BUTTON, Editor, PRIMARY } from "./screens/Editor";
import { History } from "./screens/History";
import { RunView } from "./screens/RunView";
import { Settings } from "./screens/Settings";
import { Web } from "./screens/Web";
import { WorkflowList } from "./screens/WorkflowList";
import { subscribeExecEvents } from "./stores/runs";
import { useSettings } from "./stores/settings";
import { useWeb } from "./stores/web";

type Tab = "workflows" | "history" | "web" | "settings";
/** 웹 이음은 로그인했을 때만 보인다. */
const TABS: { id: Tab; label: Key; icon: LucideIcon }[] = [
  { id: "workflows", label: "nav.workflows", icon: WorkflowIcon },
  { id: "history", label: "nav.history", icon: HistoryIcon },
  { id: "web", label: "nav.web", icon: Globe },
  { id: "settings", label: "nav.settings", icon: SettingsIcon },
];

// 첫 실행 안내 배너 — 닫으면 다시 안 보인다(Rust 설정의 firstRunDone은 화면이 뜨기 전에 켜져 못 쓴다).
const BANNER_KEY = "ieum.autostartBannerClosed";
const bannerClosed = () => {
  try {
    return localStorage.getItem(BANNER_KEY) === "1";
  } catch {
    return false;
  }
};
const closeBanner = () => {
  try {
    localStorage.setItem(BANNER_KEY, "1");
  } catch {
    // 저장소를 못 쓰면 이번 실행에서만 닫힌다.
  }
};

type WorkflowView = { kind: "list" } | { kind: "editor"; id: string | null } | { kind: "run"; id: string };

export default function App() {
  const lang = useSettings((s) => s.lang);
  const loadSettings = useSettings((s) => s.load);
  const [tab, setTab] = useState<Tab>("workflows");
  const [view, setView] = useState<WorkflowView>({ kind: "list" });
  const account = useWeb((s) => s.account);
  const loadAccount = useWeb((s) => s.load);
  const autostart = useSettings((s) => s.settings?.autostart ?? false);
  const [banner, setBanner] = useState(() => !bannerClosed());

  useEffect(() => {
    void loadSettings();
    void loadAccount();
    subscribeExecEvents();
  }, [loadSettings, loadAccount]);

  // 로그아웃·로그인 만료(`AUTH_EXPIRED`)면 설정의 계정 칸(로그인 화면)으로.
  useEffect(() => {
    if (!account && tab === "web") setTab("settings");
  }, [account, tab]);

  const toList = () => setView({ kind: "list" });
  const openEditor = (id: string | null) => setView({ kind: "editor", id });
  const openRuns = (id: string) => setView({ kind: "run", id });

  // 편집기에 저장 안 한 변경이 있으면 떠나기 전에 앱 안에서 묻는다.
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState<(() => void) | null>(null);
  const leave = (go: () => void) => (dirty ? setPending(() => go) : go());

  return (
    // 언어가 바뀌면 화면 전체를 다시 그린다.
    <div key={lang} className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-neutral-200 bg-white p-3">
        <span className="px-3 pb-4 pt-2 typo-title2_bold text-main-blue">{t("app.name")}</span>
        {TABS.filter(({ id }) => id !== "web" || account).map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() =>
              leave(() => {
                setTab(id);
                if (id === "workflows") toList();
              })
            }
            className={`flex items-center gap-2 rounded-brand-sm px-3 py-2 text-left typo-body2_semibold ${
              tab === id ? "bg-main-light-blue text-main-deep-blue" : "text-neutral-600 hover:bg-neutral-100"
            }`}
            aria-current={tab === id ? "page" : undefined}
          >
            <Icon size={18} aria-hidden="true" />
            {t(label)}
          </button>
        ))}
      </nav>
      <main className="flex min-w-0 flex-1 flex-col">
        {banner && autostart && (
          <div role="status" className="flex items-center gap-3 border-b border-main-light-blue bg-main-light-blue px-4 py-2 typo-body3_semibold text-main-deep-blue">
            <span className="min-w-0 flex-1">{t("banner.autostart")}</span>
            <button
              type="button"
              className="rounded-brand-sm p-1 hover:bg-white/60"
              aria-label={t("banner.close")}
              onClick={() => {
                closeBanner();
                setBanner(false);
              }}
            >
              <X size={16} aria-hidden="true" />
            </button>
          </div>
        )}
        {pending && (
          <div role="alert" className="flex items-center gap-3 border-b border-danger-300 bg-danger-100 px-4 py-2 typo-body3_semibold text-danger-700">
            <span className="min-w-0 flex-1">{t("common.unsavedLeave")}</span>
            <button type="button" className={BUTTON} onClick={() => setPending(null)}>
              {t("common.cancel")}
            </button>
            <button
              type="button"
              className={PRIMARY}
              onClick={() => {
                setPending(null);
                pending();
              }}
            >
              {t("common.discard")}
            </button>
          </div>
        )}
        <div className="min-h-0 flex-1 overflow-y-auto">
          {tab === "settings" ? (
            <Settings />
          ) : tab === "web" ? (
            <Web />
          ) : tab === "history" ? (
            <History />
          ) : view.kind === "editor" ? (
            <Editor
              key={view.id ?? "new"}
              id={view.id}
              onBack={() => leave(toList)}
              onOpenRuns={(id) => leave(() => openRuns(id))}
              onDirty={setDirty}
            />
          ) : view.kind === "run" ? (
            <RunView workflowId={view.id} onBack={toList} onEdit={openEditor} />
          ) : (
            <WorkflowList onOpen={openEditor} onOpenRuns={openRuns} />
          )}
        </div>
      </main>
    </div>
  );
}
