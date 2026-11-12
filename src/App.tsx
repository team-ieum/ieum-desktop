import { getVersion } from "@tauri-apps/api/app";
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type WebWorkflowSummary } from "./api";
import { t } from "./i18n";
import { BUTTON, Editor, PRIMARY } from "./screens/Editor";
import { History } from "./screens/History";
import { RunView } from "./screens/RunView";
import { Settings } from "./screens/Settings";
import { Web } from "./screens/Web";
import { WorkflowList } from "./screens/WorkflowList";
import { Header } from "./shell/Header";
import { NAV, type Page, type View } from "./shell/nav";
import { recentItems, Sidebar } from "./shell/Sidebar";
import { subscribeExecEvents } from "./stores/runs";
import { useSettings } from "./stores/settings";
import { useWeb } from "./stores/web";
import { useWorkflows } from "./stores/workflows";

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

export default function App() {
  const lang = useSettings((s) => s.lang);
  const loadSettings = useSettings((s) => s.load);
  const [view, setView] = useState<View>({ page: "local" });
  const account = useWeb((s) => s.account);
  const loadAccount = useWeb((s) => s.load);
  const autostart = useSettings((s) => s.settings?.autostart ?? false);
  const [banner, setBanner] = useState(() => !bannerClosed());
  const [collapsed, setCollapsed] = useState(false);
  const [version, setVersion] = useState("");
  const local = useWorkflows((s) => s.list);
  const loadLocal = useWorkflows((s) => s.load);
  const [web, setWeb] = useState<WebWorkflowSummary[]>([]);

  useEffect(() => {
    void loadSettings();
    void loadAccount();
    void loadLocal();
    subscribeExecEvents();
    getVersion().then(setVersion, () => {});
  }, [loadSettings, loadAccount, loadLocal]);

  // 최근 작업의 웹 몫 — 못 불러오면 내 PC만.
  useEffect(() => {
    if (!account) return setWeb([]);
    api.webListWorkflows().then(setWeb, () => setWeb([]));
  }, [account]);

  // 로그아웃·로그인 만료(`AUTH_EXPIRED`)면 설정의 계정 칸(로그인 화면)으로 — 관문은 Task 7.
  useEffect(() => {
    if (!account && view.page === "web") setView({ page: "settings" });
  }, [account, view.page]);

  const toList = () => setView({ page: "local" });
  const openEditor = (id: string | null) => setView({ page: "local", editor: id });
  const openRuns = (id: string) => setView({ page: "local", run: id });

  // 편집기에 저장 안 한 변경이 있으면 떠나기 전에 앱 안에서 묻는다.
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState<(() => void) | null>(null);
  const leave = (go: () => void) => (dirty ? setPending(() => go) : go());
  const newCanvas = () => leave(() => openEditor(null));

  // ⌘N = 새 캔버스
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newCanvas();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const navigate = (page: Page) => leave(() => setView({ page }));

  return (
    // 언어가 바뀌면 화면 전체를 다시 그린다.
    <div key={lang} className="flex h-full flex-col">
      <Header crumb={t(NAV.find((n) => n.page === view.page)!.label)} email={account?.email ?? null} />
      <div className="flex min-h-0 flex-1">
        <Sidebar
          current={view.page}
          onNavigate={navigate}
          onNewCanvas={newCanvas}
          recent={recentItems(local, web)}
          onOpenRecent={(r) => (r.kind === "local" ? leave(() => openEditor(r.id)) : navigate("web"))}
          collapsed={collapsed}
          onToggleCollapse={() => setCollapsed((c) => !c)}
          version={version}
        />
        <main className="flex min-w-0 flex-1 flex-col bg-neutral-50 text-neutral-800">
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
            {view.page === "dashboard" ? (
              // 내용은 Task 9.
              <h1 className="p-6 typo-heading3_bold text-neutral-800">{t("dashboard.title")}</h1>
            ) : view.page === "settings" ? (
              <Settings />
            ) : view.page === "web" ? (
              <Web />
            ) : view.page === "history" ? (
              <History />
            ) : "editor" in view ? (
              <Editor
                key={view.editor ?? "new"}
                id={view.editor}
                onBack={() => leave(toList)}
                onOpenRuns={(id) => leave(() => openRuns(id))}
                onDirty={setDirty}
              />
            ) : "run" in view ? (
              <RunView workflowId={view.run} onBack={toList} onEdit={openEditor} />
            ) : (
              <WorkflowList onOpen={openEditor} onOpenRuns={openRuns} />
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
