import { History as HistoryIcon, Workflow as WorkflowIcon, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { t, type Key } from "./i18n";
import { Editor } from "./screens/Editor";
import { History } from "./screens/History";
import { RunView } from "./screens/RunView";
import { WorkflowList } from "./screens/WorkflowList";
import { subscribeExecEvents } from "./stores/runs";
import { useSettings } from "./stores/settings";

// 16b: 설정·웹 이음(로그인 시에만) 탭을 여기에 붙인다.
type Tab = "workflows" | "history";
const TABS: { id: Tab; label: Key; icon: LucideIcon }[] = [
  { id: "workflows", label: "nav.workflows", icon: WorkflowIcon },
  { id: "history", label: "nav.history", icon: HistoryIcon },
];

type WorkflowView = { kind: "list" } | { kind: "editor"; id: string | null } | { kind: "run"; id: string };

export default function App() {
  const lang = useSettings((s) => s.lang);
  const loadSettings = useSettings((s) => s.load);
  const [tab, setTab] = useState<Tab>("workflows");
  const [view, setView] = useState<WorkflowView>({ kind: "list" });

  useEffect(() => {
    void loadSettings();
    subscribeExecEvents();
  }, [loadSettings]);

  const toList = () => setView({ kind: "list" });
  const openEditor = (id: string | null) => setView({ kind: "editor", id });
  const openRuns = (id: string) => setView({ kind: "run", id });

  return (
    // 언어가 바뀌면 화면 전체를 다시 그린다.
    <div key={lang} className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-neutral-200 bg-white p-3">
        <span className="px-3 pb-4 pt-2 typo-title2_bold text-main-blue">{t("app.name")}</span>
        {TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => {
              setTab(id);
              if (id === "workflows") toList();
            }}
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
      <main className="min-w-0 flex-1 overflow-y-auto">
        {tab === "history" ? (
          <History />
        ) : view.kind === "editor" ? (
          <Editor key={view.id ?? "new"} id={view.id} onBack={toList} onOpenRuns={openRuns} />
        ) : view.kind === "run" ? (
          <RunView workflowId={view.id} onBack={toList} onEdit={openEditor} />
        ) : (
          <WorkflowList onOpen={openEditor} onOpenRuns={openRuns} />
        )}
      </main>
    </div>
  );
}
