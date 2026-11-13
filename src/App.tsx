import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import { api, type WebWorkflowSummary } from "./api";
import { AuthScreen } from "./auth/AuthScreen";
import { t } from "./i18n";
import { Onboarding } from "./onboarding/Onboarding";
import { BUTTON, Editor, PRIMARY } from "./screens/Editor";
import { Dashboard } from "./screens/Dashboard";
import { History } from "./screens/History";
import { RunView } from "./screens/RunView";
import { SettingsShell } from "./screens/settings/SettingsShell";
import { Web } from "./screens/Web";
import { WorkflowList } from "./screens/WorkflowList";
import { Header } from "./shell/Header";
import { SearchDialog } from "./shell/SearchDialog";
import { StatusBar } from "./shell/StatusBar";
import { NAV, type Page, type View } from "./shell/nav";
import { recentItems, Sidebar, useSidebarCollapsed } from "./shell/Sidebar";
import { usePendingFinish } from "./stores/pendingFinish";
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
  const relogin = useWeb((s) => s.relogin);
  const setRelogin = useWeb((s) => s.setRelogin);
  const guard = useWeb((s) => s.guard);
  const [ready, setReady] = useState(false);
  const [version, setVersion] = useState("");
  const local = useWorkflows((s) => s.list);
  const loadLocal = useWorkflows((s) => s.load);
  const [web, setWeb] = useState<WebWorkflowSummary[]>([]);
  const pendingFinish = usePendingFinish((s) => s.pending);
  const refreshPending = usePendingFinish((s) => s.refresh);

  useEffect(() => {
    // 설정·계정 응답 전엔 아무것도 그리지 않는다 — 관문·온보딩이 잠깐 깜빡이지 않게.
    void Promise.allSettled([loadSettings(), loadAccount()]).then(() => setReady(true));
    subscribeExecEvents();
    getVersion().then(setVersion, () => {});
  }, [loadSettings, loadAccount]);

  // 관문을 지나면(로그인한 계정의) 최근 작업을 읽는다. 웹 몫은 못 불러오면 내 PC만.
  // 만료돼도 앱은 그대로 쓴다 — 관문은 [다시 로그인]을 눌렀을 때만(설정 이음 계정·대시보드).
  const gated = account === null || (expired && relogin);
  const onboarding = useSettings((s) => s.settings?.onboardingDone === false);
  useEffect(() => {
    if (gated) return setWeb([]);
    // 앱을 다시 켜도 "확인 필요" 표시가 남게, 처음 읽은 목록마다 확인 대기를 한 번 묻는다.
    void loadLocal().then(() => useWorkflows.getState().list.forEach((w) => void refreshPending(w.id)));
    guard(api.webListWorkflows()).then(setWeb, () => setWeb([]));
  }, [gated, guard, loadLocal, refreshPending]);

  // 빌더가 보내는 일: 완료 확인 대기·워크플로우 변경·테스트 되돌리기 실패.
  // 시스템 알림은 화면이 묻고 Rust가 정한다 — 어느 화면인지는 여기만 알고, 창이 앞인지는 Rust만 믿을 수 있다.
  const viewNow = useRef(view);
  viewNow.current = view;
  useEffect(() => {
    if (gated) return;
    const onScreen = (id: string) => "editor" in viewNow.current && viewNow.current.editor === id;
    const subs = [
      api.onFinishPending((p) => {
        void refreshPending(p.id);
        api.finishNotice(p.id, onScreen(p.id)).catch(() => {});
      }),
      api.onWorkflowChanged((p) => {
        void loadLocal();
        void refreshPending(p.id);
      }),
      api.onTestUndoFailed((p) => {
        api.undoFailedNotice(p.id, onScreen(p.id)).catch(() => {});
      }),
    ];
    return () => subs.forEach((s) => void s.then((f) => f(), () => {}));
  }, [gated, loadLocal, refreshPending]);

  const toList = () => setView({ page: "local" });
  const openEditor = (id: string | null) => setView({ page: "local", editor: id });
  const openRuns = (id: string) => setView({ page: "local", run: id });

  // 편집기에 저장 안 한 변경이 있으면 떠나기 전에 앱 안에서 묻는다.
  const [dirty, setDirty] = useState(false);
  const [pending, setPending] = useState<(() => void) | null>(null);
  // 설정 확인 창에서 끄면 묻지 않고 버리고 나간다.
  const askDiscard = useSettings((s) => s.settings?.confirmDiscardUnsaved !== false);
  const leave = (go: () => void) => (dirty && askDiscard ? setPending(() => go) : go());
  const newCanvas = () => leave(() => openEditor(null));

  // 시스템 알림 클릭 → 그 실행 상세(대시보드와 같은 기록 화면)·실행 기록·그 워크플로우의 편집 화면.
  useEffect(() => {
    const un = api.onOpenTarget((t) => {
      const go = () =>
        setView(t.kind === "workflow" ? { page: "local", editor: t.id } : t.kind === "run" ? { page: "history", run: t.runId } : { page: "history" });
      if (dirty && askDiscard) setPending(() => go);
      else go();
    });
    return () => void un.then((f) => f(), () => {});
  }, [dirty, askDiscard]);

  const [collapsed, toggleSidebar] = useSidebarCollapsed();
  const [searching, setSearching] = useState(false);

  // ⌘N = 새 캔버스, ⌘K = 찾기
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (gated || onboarding || !(e.metaKey || e.ctrlKey)) return;
      const k = e.key.toLowerCase();
      if (k === "n" || k === "k") e.preventDefault();
      if (k === "n") newCanvas();
      if (k === "k") setSearching(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const navigate = (page: Page) =>
    leave(() => {
      if (page === "settings") setBack(view);
      setView({ page });
    });

  // 상태 줄 팝오버 → 설정 AI 제공자. 설정 안에서 누르면 돌아갈 곳은 그대로.
  const openAiSettings = () =>
    leave(() => {
      if (view.page !== "settings") setBack(view);
      setView({ page: "settings", section: "ai" });
    });
  // 상태 줄 팝오버 → 설정 › 통계 및 사용량.
  const openUsage = () =>
    leave(() => {
      if (view.page !== "settings") setBack(view);
      setView({ page: "settings", section: "usage" });
    });

  if (!ready) return null;

  // 관문: 로그인 전이거나 만료 뒤 다시 로그인할 때(이메일을 채우고 취소로 돌아갈 수 있다).
  if (gated)
    return (
      <div key={lang} className="h-full">
        <AuthScreen key={account?.email ?? ""} initialEmail={account?.email} onCancel={account ? () => setRelogin(false) : undefined} />
      </div>
    );

  // 관문 다음 첫 실행 온보딩 — 기기 단위(계정과 무관). [시작하기] 저장 뒤 설정을 다시 읽으면 닫힌다.
  if (onboarding)
    return (
      <div key={lang} className="h-full">
        <Onboarding onDone={() => void loadSettings()} />
      </div>
    );

  const searchDialog = (
    <SearchDialog
      open={searching}
      onClose={() => setSearching(false)}
      onPick={(p) => leave(() => (p.kind === "local" ? openEditor(p.id) : setView({ page: "web", workflow: p.id })))}
    />
  );

  // 설정은 셸 사이드바 대신 설정 전용 사이드바(창 신호등 자리 포함). 머리줄은 본문 위.
  if (view.page === "settings")
    return (
      <div key={lang} className="flex h-full flex-col">
        <SettingsShell
          key={"section" in view ? view.section : ""}
          onBack={() => setView(back)}
          version={version}
          initial={"section" in view ? view.section : undefined}
          header={
            <Header
              crumb={t("nav.settings")}
              email={account?.email ?? null}
              onSearch={() => setSearching(true)}
              onOpenRun={(run) => setView({ page: "history", run })}
            />
          }
        />
        <StatusBar onUsage={openUsage} onSettings={openAiSettings} />
        {searchDialog}
      </div>
    );

  return (
    // 언어가 바뀌면 화면 전체를 다시 그린다.
    <div key={lang} className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        {!collapsed && (
          <Sidebar
            current={view.page}
            onNavigate={navigate}
            onNewCanvas={newCanvas}
            onSearch={() => setSearching(true)}
            onCollapse={toggleSidebar}
            recent={recentItems(local, web, pendingFinish)}
            onOpenRecent={(r) => leave(() => (r.kind === "local" ? openEditor(r.id) : setView({ page: "web", workflow: r.id })))}
            version={version}
          />
        )}
        <div className="flex min-w-0 flex-1 flex-col">
          <Header
            crumb={t(NAV.find((n) => n.page === view.page)!.label)}
            email={account?.email ?? null}
            onSearch={() => setSearching(true)}
            onOpenRun={(run) => leave(() => setView({ page: "history", run }))}
            inset={collapsed}
            onExpand={collapsed ? toggleSidebar : undefined}
          />
          <main className="flex min-h-0 flex-1 flex-col bg-neutral-50 text-neutral-800">
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
                <Dashboard onOpenRun={(run) => setView({ page: "history", run })} onUsage={openUsage} onAccounts={openAiSettings} />
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
      <StatusBar onUsage={openUsage} onSettings={openAiSettings} />
      {searchDialog}
    </div>
  );
}
