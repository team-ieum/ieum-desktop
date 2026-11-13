import { ArrowLeft, ExternalLink, Eye, Globe, History } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { api, type WebExecution, type WebNodeLog, type WebWorkflow, type WebWorkflowSummary, type WfNode } from "../api";
import { Canvas, nodeTitle, StatusIcon } from "../editor/Canvas";
import { TechSwitch } from "../editor/Toolbar";
import { locale, t, tDyn } from "../i18n";
import { fieldKey } from "../nodes/defs";
import { ListPage } from "../list/ListPage";
import { webCard, WorkflowCard } from "../list/WorkflowCard";
import { applyWebEvent, emptyWebLive, useWeb, webErrorText, webStatuses } from "../stores/web";
import { BUTTON, NodeHead, SidePanel } from "./Editor";

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
  beside,
  children,
}: {
  title: string;
  onBack?: () => void;
  onError: (m: string) => void;
  /** 이름 바로 옆(활성 상태 등). */
  beside?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="flex items-center gap-3 border-b border-neutral-200 bg-white px-4 py-3">
      {onBack && (
        <button type="button" className={BUTTON} onClick={onBack} aria-label={t("common.back")}>
          <ArrowLeft size={16} aria-hidden="true" />
        </button>
      )}
      <h1 className="min-w-0 truncate typo-title3_bold text-neutral-800">{title}</h1>
      {beside}
      <div className="flex-1" />
      {children}
      <button type="button" className={BUTTON} onClick={() => void api.openWebEditor().catch((e) => onError(webErrorText(e)))}>
        <ExternalLink size={16} aria-hidden="true" />
        {t("web.editOnWeb")}
      </button>
    </header>
  );
}

/**
 * 웹 이음 탭 — 읽기 전용. 목록↔워크플로우는 App의 View(`{page:"web", workflow}`)가 쥐고
 * (사이드바 최근 작업이 바로 열 수 있게), 실행 보기만 여기서. 로그인 만료는 `guard`가 App을 관문으로 돌린다.
 */
export function Web({ workflow, onOpen }: { workflow?: string; onOpen: (id: string | null) => void }) {
  // 실행은 그 워크플로우에 묶는다 — 다른 워크플로우나 목록으로 가면 풀린다.
  const [opened, setOpened] = useState<{ workflow: string; execution: WebExecution } | null>(null);
  if (opened && opened.workflow !== workflow) setOpened(null);
  const execution = opened && opened.workflow === workflow ? opened.execution : null;
  if (workflow && execution)
    return <WebExecutionView workflowId={workflow} execution={execution} onBack={() => setOpened(null)} />;
  if (workflow)
    return <WebWorkflowView key={workflow} id={workflow} onBack={() => onOpen(null)} onOpenExecution={(x) => setOpened({ workflow, execution: x })} />;
  return <WebList onOpen={onOpen} />;
}

function WebList({ onOpen }: { onOpen: (id: string) => void }) {
  const guard = useWeb((s) => s.guard);
  const [list, setList] = useState<WebWorkflowSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    guard(api.webListWorkflows()).then(setList, (e) => setError(webErrorText(e)));
  }, [guard]);

  const fail = (e: unknown) => setError(webErrorText(e));
  return (
    <ListPage
      title={t("web.title")}
      subtitle={t("web.subtitle")}
      actions={
        <>
          <span className="inline-flex h-7 items-center gap-1.5 rounded-full border border-main-light-blue bg-main-light-blue px-2.5 typo-caption1_semibold text-main-deep-blue">
            <Eye className="size-3.5" aria-hidden="true" />
            {t("web.viewOnly")}
          </span>
          <button
            type="button"
            onClick={() => void api.openWebEditor().catch(fail)}
            className="inline-flex h-10 items-center gap-2 rounded-[10px] border border-neutral-200 bg-white px-4 typo-body2_semibold text-main-deep-blue shadow-[0_1px_2px_rgba(16,24,40,.04)] hover:bg-neutral-50"
          >
            <Globe className="size-4" aria-hidden="true" />
            <span>{t("web.openWeb")}</span>
          </button>
        </>
      }
      banner={error && <p className="rounded-xl border border-danger-300 bg-danger-100 px-4 py-2 typo-caption1_semibold text-danger-700">{error}</p>}
      // 오류면 빈 목록 대신 오류만 — 읽는 중 문구가 남지 않게 []로.
      cards={list ? list.map(webCard) : error ? [] : null}
      empty={error ? "" : t("web.empty")}
      renderCard={(c) => (
        <WorkflowCard
          key={c.id}
          data={c}
          onOpen={() => onOpen(c.id)}
          menu={[{ label: t("list.openOnWeb"), onSelect: () => void api.openWebWorkflow(c.id).catch(fail) }]}
        />
      )}
    />
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

  // 활성 여부는 상세에 없다 — 목록에서 찾는다(못 찾으면 안 보인다).
  const [active, setActive] = useState<boolean | null>(null);
  useEffect(() => {
    guard(api.webListWorkflows()).then(
      (l) => setActive(l.find((w) => w.id === id)?.active ?? null),
      () => {},
    );
  }, [id, guard]);

  // 오른쪽 패널은 한 번에 하나 — 실행 기록 또는 노드 정보.
  const [panel, setPanel] = useState<{ kind: "history" } | { kind: "node"; id: string } | null>(null);
  const [techInfo, setTechInfo] = useState(false);
  const node = panel?.kind === "node" ? wf?.nodes.find((n) => n.id === panel.id) : undefined;
  const historyOpen = panel?.kind === "history";

  return (
    <div className="flex h-full flex-col">
      <Header
        title={wf?.name ?? ""}
        onBack={onBack}
        onError={setError}
        beside={
          <>
            {active !== null && (
              <span className={`${PILL} gap-1.5 ${active ? "border-node-green/15 bg-node-green/10 text-node-green" : "border-neutral-300 bg-white text-neutral-500"}`}>
                <span className={`size-1.5 rounded-full ${active ? "bg-[#006A4E]" : "bg-neutral-400"}`} />
                {active ? t("list.status.active") : t("list.status.paused")}
              </span>
            )}
            <ReadOnlyPill />
          </>
        }
      >
        <TechSwitch on={techInfo} onChange={setTechInfo} />
        <button
          type="button"
          onClick={() => setPanel(historyOpen ? null : { kind: "history" })}
          aria-label={t("web.executions")}
          title={t("web.executions")}
          aria-pressed={historyOpen}
          className={`grid size-9 shrink-0 place-items-center rounded-[10px] transition-colors ${historyOpen ? "bg-main-light-blue text-main-deep-blue" : "text-neutral-500 hover:bg-neutral-100"}`}
        >
          <History size={18} aria-hidden="true" />
        </button>
      </Header>
      <ErrorLine msg={error} />
      <ErrorLine msg={pageError} />
      <div className="relative min-h-0 flex-1">
        <div className="absolute inset-0">
          {wf && (
            <Canvas
              workflow={wf}
              readOnly
              techInfo={techInfo}
              selectedId={node?.id ?? null}
              onNodeClick={(nid) => setPanel({ kind: "node", id: nid })}
            />
          )}
        </div>
        {historyOpen && (
          <SidePanel label={t("web.executions")} onClose={() => setPanel(null)} header={<h2 className="typo-body1_bold text-neutral-800">{t("web.executions")}</h2>}>
            <div className="flex flex-col gap-3 p-4">
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
            </div>
          </SidePanel>
        )}
        {node && (
          <SidePanel label={node.label || nodeTitle(node.event)} onClose={() => setPanel(null)} header={<NodeHead node={node} />}>
            <NodeInfo node={node} />
          </SidePanel>
        )}
      </div>
    </div>
  );
}

/** 웹 노드 설정 값 — 읽기 전용(편집 칸 없음). */
function NodeInfo({ node }: { node: WfNode }) {
  const entries = Object.entries(node.config ?? {});
  return (
    <div className="flex flex-col items-start gap-4 p-5">
      <ReadOnlyPill />
      {(tDyn(`node.${node.event}.desc`) ?? node.description) && (
        <p className="typo-body3_regular text-neutral-600">{tDyn(`node.${node.event}.desc`) ?? node.description}</p>
      )}
      {entries.length === 0 && <p className="typo-body3_regular text-neutral-500">{t("web.noConfig")}</p>}
      <dl className="flex w-full flex-col gap-3">
        {entries.map(([k, v]) => (
          <div key={k} className="flex flex-col gap-1">
            <dt className="typo-label_semibold text-neutral-700">{tDyn(fieldKey(node.event, k)) ?? k}</dt>
            <dd className="m-0">
              {typeof v === "string" || typeof v === "number" || typeof v === "boolean" ? (
                <span className="block break-words rounded-brand-sm bg-neutral-100 px-3 py-2 typo-body3_regular text-neutral-800">{String(v)}</span>
              ) : (
                <pre className="max-h-60 overflow-auto rounded-brand-sm bg-neutral-100 px-3 py-2 font-mono text-xs text-neutral-800">{JSON.stringify(v, null, 2)}</pre>
              )}
            </dd>
          </div>
        ))}
      </dl>
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
