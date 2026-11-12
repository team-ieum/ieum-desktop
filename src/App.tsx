import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";
import { api, type WebWorkflowSummary } from "./api";
import { AuthScreen } from "./auth/AuthScreen";
import { t } from "./i18n";
import { BUTTON, Editor, PRIMARY } from "./screens/Editor";
import { Dashboard } from "./screens/Dashboard";
import { History } from "./screens/History";
import { RunView } from "./screens/RunView";
import { SettingsShell } from "./screens/settings/SettingsShell";
import { Web } from "./screens/Web";
import { WorkflowList } from "./screens/WorkflowList";
import { Header } from "./shell/Header";
import { NAV, type Page, type View } from "./shell/nav";
import { recentItems, Sidebar } from "./shell/Sidebar";
import { subscribeExecEvents } from "./stores/runs";
import { useSettings } from "./stores/settings";
import { useWeb } from "./stores/web";
import { useWorkflows } from "./stores/workflows";

export default function App() {
  const lang = useSettings((s) => s.lang);
  const loadSettings = useSettings((s) => s.load);
  const [view, setView] = useState<View>({ page: "dashboard" });
  const [back, setBack] = useState<View>({ page: "dashboard" });
  const account = useWeb((s) => s.account);
  // 로그아웃하면 다음 로그인은 대시보드에서 시작한다.
  const [lastAccount, setLastAccount] = useState(account);
  if (account !== lastAccount) {
    setLastAccount(account);
    if (!account) setView({ page: "dashboard" });
  }
  const loadAccount = useWeb((s) => s.load);
  const expired = useWeb((s) => s.expired);
  const guard = useWeb((s) => s.guard);
  const [ready, setReady] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [version, setVersion] = useState("");
  const local = useWorkflows((s) => s.list);
  const loadLocal = useWorkflows((s) => s.load);
  const [web, setWeb] = useState<WebWorkflowSummary[]>([]);

  useEffect(() => {
    void loadSettings();
    // 계정 응답 전엔 아무것도 그리지 않는다 — 관문이 잠깐 깜빡이지 않게.
    void loadAccount().finally(() => setReady(true));
    subscribeExecEvents();
    getVersion().then(setVersion, () => {});
  }, [loadSettings, loadAccount]);

  // 관문을 지나면(로그인한 계정의) 최근 작업을 읽는다. 웹 몫은 못 불러오면 내 PC만 —
  // 만료면 guard가 관문으로, 오프라인이면 그대로.
  const gated = account === null || expired;
  useEffect(() => {
    if (gated) return setWeb([]);
    void loadLocal();
    guard(api.webListWorkflows()).then(setWeb, () => setWeb([]));
  }, [gated, guard, loadLocal]);

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
      if (!gated && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newCanvas();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const navigate = (page: Page) =>
    leave(() => {
      if (page === "settings") setBack(view);
      setView({ page });
    });

  if (!ready) return null;

  // 관문: 로그인 전·만료면 로그인 화면만(만료면 이메일을 채운다). 오프라인은 관문으로 보내지 않는다.
  if (gated)
    return (
      <div key={lang} className="h-full">
        <AuthScreen key={account?.email ?? ""} initialEmail={account?.email} />
      </div>
    );

  // 설정은 셸 사이드바 대신 설정 전용 사이드바.
  if (view.page === "settings")
    return (
      <div key={lang} className="flex h-full flex-col">
        <Header
          crumb={t("nav.settings")}
          email={account?.email ?? null}
          onPick={(p) => (p.kind === "local" ? openEditor(p.id) : setView({ page: "web", workflow: p.id }))}
          onOpenRun={(run) => setView({ page: "history", run })}
        />
        <SettingsShell onBack={() => setView(back)} version={version} />
      </div>
    );

  return (
    // 언어가 바뀌면 화면 전체를 다시 그린다.
    <div key={lang} className="flex h-full flex-col">
      <Header
        crumb={t(NAV.find((n) => n.page === view.page)!.label)}
        email={account?.email ?? null}
        onPick={(p) => leave(() => (p.kind === "local" ? openEditor(p.id) : setView({ page: "web", workflow: p.id })))}
        onOpenRun={(run) => leave(() => setView({ page: "history", run }))}
      />
      <div className="flex min-h-0 flex-1">
        <Sidebar
          current={view.page}
          onNavigate={navigate}
          onNewCanvas={newCanvas}
          recent={recentItems(local, web)}
          onOpenRecent={(r) => leave(() => (r.kind === "local" ? openEditor(r.id) : setView({ page: "web", workflow: r.id })))}
          collapsed={collapsed}
          onToggleCollapse={() => setCollapsed((c) => !c)}
          version={version}
        />
        <main className="flex min-w-0 flex-1 flex-col bg-neutral-50 text-neutral-800">
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
              <Dashboard onOpenRun={(run) => setView({ page: "history", run })} />

            ) : view.page === "web" ? (
              <Web workflow={"workflow" in view ? view.workflow : undefined} onOpen={(id) => setView(id ? { page: "web", workflow: id } : { page: "web" })} />
            ) : view.page === "history" ? (
              <History key={"run" in view ? view.run : ""} initialRun={"run" in view ? view.run : null} />
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
              <WorkflowList onOpen={openEditor} />
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
