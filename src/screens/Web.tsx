import { ArrowLeft, ExternalLink, Eye } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { api, type WebExecution, type WebNodeLog, type WebWorkflow, type WebWorkflowSummary } from "../api";
import { Canvas, nodeTitle, StatusIcon } from "../editor/Canvas";
import { locale, t, tDyn } from "../i18n";
import { applyWebEvent, emptyWebLive, useWeb, webErrorText, webStatuses } from "../stores/web";
import { BUTTON } from "./Editor";

/** 아직 끝나지 않은 실행 — 열면 생중계를 건다. */
const LIVE_STATUSES = ["PENDING", "RUNNING"];

const when = (iso?: string | null) => (iso ? new Date(iso).toLocaleString(locale()) : "");
const execLabel = (s: string) => tDyn(`web.exec.${s}`) ?? s;

const PILL = "inline-flex shrink-0 items-center rounded-full border px-2.5 py-1 typo-caption1_semibold";
const ExecPill = ({ status }: { status: string }) => (
  <span
    className={`${PILL} ${
      status === "FAILED"
        ? "border-danger-300 bg-danger-100 text-danger-700"
        : status === "SUCCESS"
          ? "border-main-light-blue bg-main-light-blue text-main-deep-blue"
          : "border-neutral-300 bg-white text-neutral-600"
    }`}
  >
    {execLabel(status)}
  </span>
);

const ErrorLine = ({ msg }: { msg: string | null }) =>
  msg ? <p className="border-b border-danger-300 bg-danger-100 px-4 py-2 typo-caption1_semibold text-danger-700">{msg}</p> : null;

function Header({
  title,
  onBack,
  onError,
  children,
}: {
  title: string;
  onBack?: () => void;
  onError: (m: string) => void;
  children?: ReactNode;
}) {
  return (
    <header className="flex items-center gap-3 border-b border-neutral-200 bg-white px-4 py-3">
      {onBack && (
        <button type="button" className={BUTTON} onClick={onBack} aria-label={t("common.back")}>
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
      )}
      <h1 className="min-w-0 flex-1 truncate typo-title3_bold text-neutral-800">{title}</h1>
      {children}
      <button type="button" className={BUTTON} onClick={() => void api.openWebEditor().catch((e) => onError(webErrorText(e)))}>
        <ExternalLink size={16} aria-hidden="true" />
        {t("web.editOnWeb")}
      </button>
    </header>
  );
}

type View = { kind: "list" } | { kind: "workflow"; id: string } | { kind: "execution"; workflowId: string; execution: WebExecution };

/** 웹 이음 탭 — 읽기 전용. 로그인 만료는 `guard`가 App을 로그인 화면으로 돌린다. */
export function Web() {
  const [view, setView] = useState<View>({ kind: "list" });
  if (view.kind === "workflow")
    return (
      <WebWorkflowView
        id={view.id}
        onBack={() => setView({ kind: "list" })}
        onOpenExecution={(execution) => setView({ kind: "execution", workflowId: view.id, execution })}
      />
    );
  if (view.kind === "execution")
    return (
      <WebExecutionView
        workflowId={view.workflowId}
        execution={view.execution}
        onBack={() => setView({ kind: "workflow", id: view.workflowId })}
      />
    );
  return <WebList onOpen={(id) => setView({ kind: "workflow", id })} />;
}

function WebList({ onOpen }: { onOpen: (id: string) => void }) {
  const guard = useWeb((s) => s.guard);
  const [list, setList] = useState<WebWorkflowSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    guard(api.webListWorkflows()).then(setList, (e) => setError(webErrorText(e)));
  }, [guard]);

  return (
    <div className="flex h-full flex-col">
      <Header title={t("web.title")} onError={setError} />
      <ErrorLine msg={error} />
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-2 p-6">
        {!list ? (
          !error && <p className="typo-body3_regular text-neutral-500">{t("common.loading")}</p>
        ) : list.length === 0 ? (
          <p className="rounded-brand-md border border-dashed border-neutral-300 bg-white p-8 text-center typo-body2_regular text-neutral-500">
            {t("web.empty")}
          </p>
        ) : (
          <ul className="flex flex-col gap-2">
            {list.map((w) => (
              <li key={w.id}>
                <button
                  type="button"
                  onClick={() => onOpen(w.id)}
                  className="flex w-full items-center gap-3 rounded-brand-md border border-neutral-200 bg-white p-4 text-left hover:border-main-blue"
                >
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate typo-body2_bold text-neutral-800">{w.name}</span>
                    {w.description && <span className="truncate typo-caption1_regular text-neutral-500">{w.description}</span>}
                    {w.updatedAt && (
                      <span className="typo-caption2_regular text-neutral-400">
                        {t("list.updated")} {when(w.updatedAt)}
                      </span>
                    )}
                  </span>
                  <span className={`${PILL} border-neutral-300 bg-white text-neutral-600`}>{w.active ? t("web.active") : t("web.inactive")}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function ReadOnlyPill() {
  return (
    <span className={`${PILL} gap-1 border-neutral-300 bg-white text-neutral-600`}>
      <Eye size={12} aria-hidden="true" />
      {t("web.readOnly")}
    </span>
  );
}

function useWebWorkflow(id: string, setError: (m: string) => void) {
  const guard = useWeb((s) => s.guard);
  const [wf, setWf] = useState<WebWorkflow | null>(null);
  useEffect(() => {
    guard(api.webGetWorkflow(id)).then(setWf, (e) => setError(webErrorText(e)));
  }, [id, guard, setError]);
  return wf;
}

function WebWorkflowView({ id, onBack, onOpenExecution }: { id: string; onBack: () => void; onOpenExecution: (e: WebExecution) => void }) {
  const guard = useWeb((s) => s.guard);
  const [error, setError] = useState<string | null>(null);
  const wf = useWebWorkflow(id, setError);
  const [execs, setExecs] = useState<WebExecution[] | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  // 실행 기록 오류는 따로 — 재시도가 성공하면 지운다(워크플로우 읽기 오류는 남긴다).
  const [pageError, setPageError] = useState<string | null>(null);

  const loadPage = async (from: string | null) => {
    setLoadingMore(true);
    try {
      const p = await guard(api.webListExecutions(id, from));
      setExecs((cur) => [...(from ? (cur ?? []) : []), ...p.content]);
      setCursor(p.hasNext ? p.nextCursor : null);
      setPageError(null);
    } catch (e) {
      setPageError(webErrorText(e));
    } finally {
      setLoadingMore(false);
    }
  };
  useEffect(() => {
    void loadPage(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  return (
    <div className="flex h-full flex-col">
      <Header title={wf?.name ?? ""} onBack={onBack} onError={setError}>
        <ReadOnlyPill />
      </Header>
      <ErrorLine msg={error} />
      <ErrorLine msg={pageError} />
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">{wf && <Canvas workflow={wf} readOnly />}</div>
        <aside className="flex w-96 shrink-0 flex-col gap-3 overflow-y-auto border-l border-neutral-200 bg-white p-4">
          <h2 className="typo-body1_bold text-neutral-800">{t("web.executions")}</h2>
          {execs?.length === 0 && <p className="typo-body3_regular text-neutral-500">{t("web.noExecutions")}</p>}
          <ul className="flex flex-col gap-2">
            {execs?.map((x) => (
              <li key={x.id}>
                <button
                  type="button"
                  onClick={() => onOpenExecution(x)}
                  className="flex w-full items-center gap-2 rounded-brand-sm border border-neutral-200 p-3 text-left hover:border-main-blue"
                >
                  <span className="min-w-0 flex-1 truncate typo-caption1_regular text-neutral-700">{when(x.startedAt)}</span>
                  <ExecPill status={x.status} />
                </button>
              </li>
            ))}
          </ul>
          {cursor && (
            <button type="button" className={BUTTON} disabled={loadingMore} onClick={() => void loadPage(cursor)}>
              {t("web.more")}
            </button>
          )}
        </aside>
      </div>
    </div>
  );
}

function WebExecutionView({ workflowId, execution, onBack }: { workflowId: string; execution: WebExecution; onBack: () => void }) {
  const { guard, expireIf } = useWeb();
  const [error, setError] = useState<string | null>(null);
  const wf = useWebWorkflow(workflowId, setError);
  const [logs, setLogs] = useState<WebNodeLog[] | null>(null);
  const [live, setLive] = useState(() => emptyWebLive(execution.id));
  const exId = execution.id;

  // 노드 로그 — 처음 한 번, 그리고 실행이 끝나면 다시(최종 결과).
  useEffect(() => {
    guard(api.webExecutionLogs(workflowId, exId)).then(setLogs, (e) => setError(webErrorText(e)));
  }, [workflowId, exId, live.status, guard]);

  // 실행 중이면 생중계. 떠나면 해제 — 해제 뒤 늦게 온 이벤트는 `applyWebEvent`가 executionId로 거른다.
  useEffect(() => {
    if (!LIVE_STATUSES.includes(execution.status)) return;
    let left = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const un = await api.onWebExecEvent((p) => {
        if (p.executionId === exId && "error" in p) expireIf(p.error);
        setLive((l) => applyWebEvent(l, p));
      });
      if (left) return un();
      unlisten = un;
      try {
        await guard(api.webWatchExecution(workflowId, exId));
        if (left) void api.webUnwatchExecution(exId);
      } catch (e) {
        if (!left) setError(webErrorText(e));
      }
    })();
    return () => {
      left = true;
      unlisten?.();
      void api.webUnwatchExecution(exId).catch((e) => console.error("[ieum] 구독을 해제하지 못했어요", e));
    };
  }, [workflowId, exId, execution.status, guard, expireIf]);

  // 폴백 logs엔 실행 상태가 없다 — 실행 목록을 다시 읽어 헤더를 맞춘다(첫 쪽에 없으면 그대로).
  const [listedStatus, setListedStatus] = useState<string | null>(null);
  useEffect(() => {
    if (!live.logs) return;
    guard(api.webListExecutions(workflowId, null)).then(
      (p) => setListedStatus(p.content.find((x) => x.id === exId)?.status ?? null),
      (e) => setError(webErrorText(e)),
    );
  }, [workflowId, exId, live.logs, guard]);

  const baseLogs = live.logs ?? logs ?? [];
  // 처음 로그에 없던 노드의 생중계 실패도 문구가 보이게 덧붙인다.
  const shownLogs: WebNodeLog[] = [
    ...baseLogs,
    ...Object.keys(live.errors)
      .filter((id) => !baseLogs.some((l) => l.nodeId === id))
      .map((id) => ({ id: `live-${id}`, nodeId: id, status: "FAILED" })),
  ];
  const statuses = webStatuses(logs ?? [], live);
  const nodeName = (id: string) => {
    const n = wf?.nodes.find((x) => x.id === id);
    return n ? n.label || nodeTitle(n.event) : id;
  };

  return (
    <div className="flex h-full flex-col">
      <Header title={`${wf?.name ?? ""} · ${when(execution.startedAt)}`} onBack={onBack} onError={setError}>
        <ReadOnlyPill />
        <ExecPill status={live.status ?? listedStatus ?? execution.status} />
      </Header>
      <ErrorLine msg={error} />
      {live.stopped && <p className="border-b border-neutral-200 bg-neutral-100 px-4 py-2 typo-caption1_semibold text-neutral-700">{t("web.stopped")}</p>}
      {live.logs && <p className="border-b border-neutral-200 bg-neutral-100 px-4 py-2 typo-caption1_semibold text-neutral-700">{t("web.finalState")}</p>}
      <ErrorLine msg={live.error && webErrorText(live.error)} />
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1">{wf && <Canvas workflow={wf} readOnly statuses={statuses} />}</div>
        <aside className="flex w-96 shrink-0 flex-col gap-3 overflow-y-auto border-l border-neutral-200 bg-white p-4">
          <h2 className="typo-body1_bold text-neutral-800">{t("web.nodeLogs")}</h2>
          {shownLogs.length === 0 && <p className="typo-body3_regular text-neutral-500">{t("web.noLogs")}</p>}
          <ul className="flex flex-col gap-3">
            {shownLogs.map((l) => {
              const msg = live.errors[l.nodeId] ?? (typeof l.errorMessage === "string" ? l.errorMessage : null);
              return (
                <li key={l.id} className="flex flex-col gap-1.5 rounded-brand-sm border border-neutral-200 p-3">
                  <div className="flex items-center gap-2">
                    <StatusIcon status={statuses[l.nodeId]} />
                    <span className="min-w-0 flex-1 truncate typo-body3_semibold text-neutral-800">{nodeName(l.nodeId)}</span>
                  </div>
                  {msg && <p className="typo-caption1_regular text-danger-700">{msg}</p>}
                </li>
              );
            })}
          </ul>
        </aside>
      </div>
    </div>
  );
}
