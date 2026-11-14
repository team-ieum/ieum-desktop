import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";
import { api, type Suggestion, type WebWorkflowSummary } from "./api";
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
import { accountsOf } from "./screens/settings/AiPane";
import { useCredentials } from "./stores/credentials";
import { subscribeExecEvents } from "./stores/runs";
import { useSettings } from "./stores/settings";
import { useWeb } from "./stores/web";
import { useWorkflows } from "./stores/workflows";
import { SuggestBanner } from "./usage/SuggestBanner";

/** 창이 한 번이라도 보이기 전(로그인 시 숨은 채 자동 시작)엔 아무것도 그리지 않는다
 *  — 화면마다 막지 않고 여기 한 곳에서, 키체인 허용 창이 뜰 수 있는 첫 조회를 미룬다. */
export default function App() {
  const [shown, setShown] = useState(() => document.visibilityState === "visible");
  useEffect(() => {
    if (shown) return;
    const on = () => {
      if (document.visibilityState === "visible") setShown(true);
    };
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, [shown]);
  return shown ? <AppBody /> : null;
}

function AppBody() {
  const lang = useSettings((s) => s.lang);
  const loadSettings = useSettings((s) => s.load);
  const [view, setView] = useState<View>({ page: "dashboard" });
  const [back, setBack] = useState<View>({ page: "dashboard" });
  const account = useWeb((s) => s.account);
  // 로그아웃하면 다음 로그인은 대시보드에서 시작한다.
  const [lastAccount, setLastAccount] = useState(account);
  // 계정 권하기 띠 대기열(아래 설명).
  const [suggests, setSuggests] = useState<Suggestion[]>([]);
  if (account !== lastAccount) {
    setLastAccount(account);
    if (!account) setView({ page: "dashboard" });
    // 다른 이음 계정(또는 로그아웃) — 이전 계정 때 받은 제안은 버린다.
    if (account?.email !== lastAccount?.email || account?.serverUrl !== lastAccount?.serverUrl) setSuggests([]);
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
    void loadLocal();
    guard(api.webListWorkflows()).then(setWeb, () => setWeb([]));
  }, [gated, guard, loadLocal]);

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

  // 기본 계정 한도 도달 → 다른 계정 권하기 띠. Rust는 보낸 제안을 대기 칸에도 넣으므로
  // 마운트·창이 다시 앞에 올 때·알림 클릭 때 꺼내 오고, 이벤트로 띄울 때도 대기 칸을 비운다.
  // "나중에"는 띠만 닫는다 — 같은 제안을 다시 띄우지 않는 건 Rust가 기억한다.
  // 회사별로 하나씩 줄 세워 앞의 것부터 보인다. 같은 회사 제안이 다시 오면 그 자리를 바꾼다.
  const creds = useCredentials((c) => c.view);
  const addSuggests = (list: Suggestion[]) =>
    setSuggests((q) => {
      const next = q.map((x) => list.find((s) => s.company === x.company) ?? x);
      return [...next, ...list.filter((s) => !q.some((x) => x.company === s.company))];
    });
  const takeSuggest = () => void api.takeAccountSuggestion().then(addSuggests, () => {});
  useEffect(() => {
    const un = api
      .onAccountSuggest((s) => {
        addSuggests([s]);
        takeSuggest();
      })
      .catch(() => () => {});
    takeSuggest();
    window.addEventListener("focus", takeSuggest);
    return () => {
      void un.then((f) => f());
      window.removeEventListener("focus", takeSuggest);
    };
  }, []);

  // 시스템 알림 클릭 → 그 실행 상세(대시보드와 같은 기록 화면) 또는 실행 기록.
  useEffect(() => {
    const un = api.onOpenTarget((t) => {
      if (t.kind === "suggest") return takeSuggest();
      const go = () => setView(t.kind === "run" ? { page: "history", run: t.runId } : { page: "history" });
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

  // 본문 열 맨 위(머리줄 아래) — 창 맨 위에 두면 신호등과 겹치고 끌어 옮길 수 없다.
  // 지금 자격 목록에 없는 계정(이음 계정이 바뀌었거나 지운 계정)을 권하는 제안은 띄우지 않는다.
  const suggest =suggests.find((s) => accountsOf(creds, s.company).some((m) => m.id === s.toId)) ?? null;
  const suggestBanner = <SuggestBanner s={suggest} onClose={() => setSuggests((q) => q.filter((x) => x !== suggest))} />;

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
            <>
              <Header
                crumb={t("nav.settings")}
                email={account?.email ?? null}
                onSearch={() => setSearching(true)}
                onOpenRun={(run) => setView({ page: "history", run })}
              />
              {suggestBanner}
            </>
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
            recent={recentItems(local, web)}
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
            {suggestBanner}
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
