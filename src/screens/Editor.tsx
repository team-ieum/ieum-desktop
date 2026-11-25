import { Plus, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type StoredWorkflow, type ValidationError, type WfNode, type WorkflowEntry } from "../api";
import { Canvas, LocalBadge, nodeTitle } from "../editor/Canvas";
import { AssistantPanel } from "../editor/chat/AssistantPanel";
import { FinishCard } from "../editor/chat/FinishCard";
import { HttpConfirm, HttpTestCard } from "../editor/chat/HttpTestCard";
import { ConfigPanel } from "../editor/ConfigPanel";
import { NodeTestSection } from "../editor/NodeTestSection";
import { Palette } from "../editor/Palette";
import { RunErrorCard } from "../editor/RunErrorCard";
import { AUTOSAVE_MS, mergeRemote, STALE } from "../editor/sync";
import { Toolbar } from "../editor/Toolbar";
import { ReplaceTriggerCard } from "../editor/ReplaceTriggerCard";
import { isTrigger, planAdd, prependTrigger, referencingNodes, replaceTrigger } from "../editor/triggerSwap";
import { useClosing } from "../editor/useClosing";
import { useTestRun } from "../editor/useTestRun";
import { t } from "../i18n";
import { isLocal, newNode, nextPosition } from "../nodes/defs";
import { BUTTON, PRIMARY } from "../editor/buttons";
import { nodeMeta, toneStyle } from "../nodes/meta";
import { emptyLive, useRuns } from "../stores/runs";
import { useWorkflows } from "../stores/workflows";

export { BUTTON, PRIMARY };

const strip = ({ watch: _w, watchMessage: _m, ...wf }: WorkflowEntry): StoredWorkflow => wf;

/** 켜져 있는 동안 Esc를 누르면 부른다. */
export function useEscape(onEscape: () => void, active = true) {
  const latest = useRef(onEscape);
  latest.current = onEscape;
  useEffect(() => {
    if (!active) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") latest.current();
    };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [active]);
}

/**
 * 캔버스 위 오른쪽에 떠 있는 카드 — 머리 + [X]. Esc로도 닫힌다.
 * 부모는 `relative`, 캔버스는 그 안에서 `absolute inset-0`(패널 뒤로 전체 폭).
 */
export function SidePanel({
  label,
  header,
  onClose,
  right,
  children,
}: {
  label: string;
  header: ReactNode;
  onClose: () => void;
  /** 오른쪽 여백(px). 없으면 right-4 — 채팅창이 열려 있으면 그 왼쪽으로 비킨다. */
  right?: number;
  children: ReactNode;
}) {
  const { closing, close } = useClosing(onClose);
  useEscape(close);
  return (
    <aside
      aria-label={label}
      style={right === undefined ? undefined : { right }}
      className={`absolute top-4 ${right === undefined ? "right-4" : ""} bottom-4 z-20 flex w-96 flex-col overflow-y-auto rounded-2xl border border-neutral-200 bg-white shadow-[0_14px_40px_rgba(43,72,86,0.18)] ${
        closing ? "ieum-slide-out" : "ieum-slide-in"
      }`}
    >
      <div className="flex items-center gap-2.5 border-b border-neutral-200 px-5 py-4">
        <div className="flex min-w-0 flex-1 items-center gap-2.5">{header}</div>
        <button
          type="button"
          onClick={close}
          aria-label={t("editor.closePanel")}
          title={t("editor.closePanel")}
          className="grid size-8 shrink-0 place-items-center rounded-[10px] text-neutral-500 transition-colors hover:bg-neutral-100"
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
      {children}
    </aside>
  );
}

/** 노드 패널 머리 — 아이콘·이름·종류. */
export function NodeHead({ node, step }: { node: WfNode; step?: number }) {
  const meta = nodeMeta(node.event, node.type);
  return (
    <div className="flex min-w-0 flex-1 items-center gap-2.5" style={toneStyle(meta)}>
      <span className="grid size-8 shrink-0 place-items-center rounded-[0.65rem] bg-(--node-color) text-white [box-shadow:0_4px_0_color-mix(in_srgb,var(--node-color)_78%,black)]">
        {meta.icon}
      </span>
      <div className="min-w-0 flex-1">
        <h2 className="truncate typo-body2_bold text-neutral-800">{node.label || nodeTitle(node.event)}</h2>
        <p className="typo-caption1_medium text-(--node-color)">
          {step ? `${t("editor.step").replace("{n}", String(step))} · ` : ""}
          {meta.label}
        </p>
      </div>
    </div>
  );
}

/**
 * 캔버스 왼쪽 가운데 [+] — 누르면 팔레트가 떠 있는 패널로. 바깥 클릭·노드 고르면 닫힌다.
 * Esc는 Palette가 한 단계씩 처리하고 마지막에 onClose — 여기서 document Esc를 따로 받으면 SidePanel까지 같이 닫힌다.
 */
function PaletteButton({ onAdd, hasTrigger, blocked }: { onAdd: (event: string) => void; hasTrigger: boolean; blocked: boolean }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);
  // `/`로 연다 — 입력 칸·터미널에서 친 /는 글자라 건드리지 않는다. 확인 카드가 떠 있으면(blocked) 그 답이 먼저다.
  useEffect(() => {
    if (blocked) return;
    const h = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey) return;
      if ((e.target as Element | null)?.closest?.('input, textarea, select, [contenteditable], .xterm')) return;
      e.preventDefault();
      setOpen(true);
      // 이미 열려 있으면 검색 칸을 되찾는다 — WKWebView에선 포커스가 body로 빠질 수 있다.
      ref.current?.querySelector<HTMLInputElement>('[role="combobox"]')?.focus();
    };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [blocked]);
  return (
    <div ref={ref} className="absolute left-4 top-1/2 z-40 -translate-y-1/2">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        // 확인 카드가 떠 있는 동안은 [+]도 막는다 — 팔레트(z-40)가 카드(z-30) 위를 덮지 않게.
        disabled={blocked}
        aria-label={t("editor.palette")}
        title={t("palette.openTitle")}
        aria-expanded={open}
        aria-haspopup="dialog"
        className="grid size-11 cursor-pointer place-items-center rounded-full bg-main-deep-blue text-white shadow-[0_8px_20px_rgba(41,83,124,0.3)] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
      >
        <Plus size={22} aria-hidden="true" />
      </button>
      {open && (
        // 스크롤은 목록·옆 창이 각자 한다 — 여기서 overflow를 걸면 옆 창이 잘린다.
        <div role="dialog" aria-label={t("editor.palette")} className="absolute left-14 top-1/2 flex -translate-y-1/2">
          <Palette
            hasTrigger={hasTrigger}
            onClose={() => setOpen(false)}
            onAdd={(e) => {
              onAdd(e);
              setOpen(false);
            }}
          />
        </div>
      )}
    </div>
  );
}

export function Editor({
  id,
  onBack,
  onOpenRuns,
  onDirty,
  onIdChange,
  onOpenIntegrations,
  onOpenAiSettings,
}: {
  id: string | null;
  onBack: () => void;
  onOpenRuns: (id: string) => void;
  /** 저장본과 다른지 — 화면을 떠날 때 확인 줄을 띄우는 데 쓴다. */
  onDirty?: (dirty: boolean) => void;
  /** 지금 보여 주는 워크플로우 id — 새 캔버스는 선저장 뒤에 생긴다. 아직 없거나 편집기를 떠나면 "". */
  onIdChange?: (id: string) => void;
  /** 오류 카드·띠의 [설정 › 연동 열기] — GitHub 연결 오류일 때만 보인다. */
  onOpenIntegrations?: () => void;
  /** 오류 카드·띠의 [설정 › AI 열기] — 키 월 한도 초과일 때만 보인다. */
  onOpenAiSettings?: () => void;
}) {
  const entry = useWorkflows((s) => s.list.find((w) => w.id === id));
  const save = useWorkflows((s) => s.save);
  const [draft, setDraft] = useState<StoredWorkflow>(() =>
    entry ? strip(entry) : { id: "", name: t("editor.untitled"), enabled: true, workflow: { nodes: [], edges: [] }, updatedAt: "" },
  );
  const [base, setBase] = useState(draft);
  const dirty = JSON.stringify(draft) !== JSON.stringify(base);
  // 채팅을 한 번이라도 열었거나 명령이 이 워크플로우를 고친 뒤로는 편집기를 떠날 때까지 자동 저장한다
  // (패널을 닫아도 터미널 세션은 살아 있어 명령이 계속 올 수 있다).
  const [assist, setAssist] = useState(false);
  /** 저장본을 다시 불러오며 새 노드 자리만 잡아 준 draft — 사람이 고친 것이 아니다. */
  const placed = useRef<StoredWorkflow | null>(null);
  /** 저장이 거부된 draft — 같은 내용으로 자동 저장을 되풀이하지 않는다. */
  const rejected = useRef<StoredWorkflow | null>(null);
  const edited = dirty && draft !== placed.current;
  const now = useRef({ draft, base, dirty, edited, assist });
  now.current = { draft, base, dirty, edited, assist };
  // 자동 저장이 맡은 편집은 떠날 때 "버릴까요"를 묻지 않는다. 자동 저장이 맡지 못하는 편집
  // (저장이 거부된 내용 그대로이거나, 선저장이 실패해 id가 없는 새 워크플로우)은 묻는다 — 조용히 사라지면 안 된다.
  const askOnLeave = dirty && (!assist || !draft.id || draft === rejected.current);
  useEffect(() => {
    onDirty?.(askOnLeave);
    return () => onDirty?.(false);
  }, [askOnLeave, onDirty]);
  useEffect(() => {
    onIdChange?.(draft.id);
    return () => onIdChange?.("");
  }, [draft.id, onIdChange]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // 시작 조건을 다른 트리거로 바꿀지 묻는 중.
  // 옛 노드는 id로만 들고 그릴 때마다 지금 워크플로우에서 찾는다 — 카드가 떠 있는 동안 지워지거나 바뀔 수 있다.
  const [swap, setSwap] = useState<{ oldId: string; event: string } | null>(null);
  const [techInfo, setTechInfo] = useState(false);
  /** Assistant 패널이 가리는 캔버스 오른쪽 폭(px). */
  const [cover, setCover] = useState(0);
  /** 오류 카드 [노드 보기] — nonce가 바뀔 때마다 캔버스가 그 노드로 보기를 옮긴다. */
  const [focus, setFocus] = useState<{ id: string; nonce: number; right: number }>();
  const [errors, setErrors] = useState<ValidationError[]>([]);
  const [status, setStatus] = useState<{ saving?: boolean; error?: string }>({});
  const live = useRuns((s) => s.live[draft.id]) ?? emptyLive(draft.id);
  // runNow는 저장본을 돌린다(실행 버튼은 미저장 편집이 있으면 꺼진다) — 확인도 저장본으로 판단한다.
  const testRun = useTestRun(draft.id, base.workflow.nodes);
  /** 빌더의 테스트가 되돌리지 못한 파일(`원래 자리 → 지금 자리`) — 터미널 입구에는 결과 카드가 없어 여기 배너로 보인다. */
  const [stuck, setStuck] = useState<string[]>([]);
  /** 캔버스 자리 — 노드 테스트 확인 카드를 패널 밖 여기에 띄운다. */
  const [area, setArea] = useState<HTMLDivElement | null>(null);

  // 고칠 때마다 검증 결과를 노드 위에 보여 준다.
  useEffect(() => {
    let stale = false;
    api.validateWorkflow(draft.workflow).then(
      (e) => !stale && setErrors(e),
      (e) => !stale && setStatus({ error: String(e) }),
    );
    return () => {
      stale = true;
    };
  }, [draft.workflow]);

  const wf = draft.workflow;
  const selected = wf.nodes.find((n) => n.id === selectedId);
  const known = new Set(wf.nodes.map((n) => n.id));
  const general = errors.filter((e) => !e.nodeId || !known.has(e.nodeId));
  const setWorkflow = (workflow: StoredWorkflow["workflow"]) => setDraft((d) => ({ ...d, workflow }));
  const setNode = (node: WfNode) => setWorkflow({ ...wf, nodes: wf.nodes.map((n) => (n.id === node.id ? node : n)) });
  const swapOld = swap ? wf.nodes.find((n) => n.id === swap.oldId && isTrigger(n) && n.event !== swap.event) : undefined;
  // 옛 트리거가 지워졌거나(휴지통·빌더 AI) 더는 바꿀 대상이 아니면 묻던 것을 접는다.
  if (swap && !swapOld) setSwap(null);

  const add = (event: string) => {
    const plan = planAdd(wf, event);
    if (plan.kind === "select") return showNode(plan.id);
    if (plan.kind === "ask") return setSwap({ oldId: plan.old.id, event });
    if (plan.kind === "prepend") {
      const node = newNode(event, { x: 0, y: 0 }, nodeTitle(event));
      setWorkflow(prependTrigger(wf, node));
      return setSelectedId(node.id);
    }
    const node = newNode(event, nextPosition(wf.nodes), nodeTitle(event));
    setWorkflow({ ...wf, nodes: [...wf.nodes, node] });
    setSelectedId(node.id);
  };

  const answerSwap = (ok: boolean) => {
    if (ok && swap && swapOld) {
      const { oldId, event } = swap;
      // 수동 실행은 켜고 끌 것이 없다 — 그 밖의 트리거는 설정을 확인하기 전까지 돌지 않게 끈다.
      // 그사이 옛 트리거가 사라졌으면 아무것도 바꾸지 않는다(켜짐만 꺼지는 일이 없게).
      setDraft((d) =>
        d.workflow.nodes.some((n) => n.id === oldId)
          ? {
              ...d,
              workflow: replaceTrigger(d.workflow, oldId, newNode(event, { x: 0, y: 0 }, nodeTitle(event))),
              enabled: event === "manual.run" ? d.enabled : false,
            }
          : d,
      );
      setSelectedId(oldId);
    }
    setSwap(null);
  };

  const remove = (nodeId: string) => {
    setWorkflow({
      ...wf,
      nodes: wf.nodes.filter((n) => n.id !== nodeId),
      edges: wf.edges.filter((e) => e.source !== nodeId && e.target !== nodeId),
    });
    setSelectedId(null);
  };

  /** 저장본으로 화면을 바꾼다(자리·살아 있는 선택은 지킨다). */
  const reload = async () => {
    await useWorkflows.getState().load();
    const found = useWorkflows.getState().list.find((w) => w.id === now.current.draft.id);
    if (!found) return;
    const remote = strip(found);
    const merged = mergeRemote(now.current.draft, remote);
    placed.current = merged;
    setBase(remote);
    setDraft(merged);
    setSelectedId((s) => (s && merged.workflow.nodes.some((n) => n.id === s) ? s : null));
  };

  const persist = async (d: StoredWorkflow) => {
    setStatus({ saving: true });
    try {
      const saved = await save(d);
      setBase(saved);
      // 저장 중에 친 글자를 지우지 않는다.
      setDraft((cur) => (cur === d ? saved : { ...cur, id: saved.id, updatedAt: saved.updatedAt }));
      setStatus({});
    } catch (e) {
      rejected.current = d;
      if (String(e) !== STALE) return setStatus({ error: String(e) });
      // 그사이 명령이 저장본을 고쳤다 — 덮지 않고 다시 불러온다. 사람이 고친 것을 잃었을 때만 알린다.
      const lost = d !== placed.current;
      await reload();
      setStatus(lost ? { error: t("sync.reloaded") } : {});
    }
  };
  const onSave = () => void persist(draft);

  const onAssistant = async (open: boolean) => {
    if (!open) return;
    setAssist(true);
    if (draft.id || status.saving) return;
    // 새 워크플로우는 꺼진 채로 먼저 저장해 id를 만든다. 화면 draft도 꺼야 자동 저장이 다시 켜지 않는다.
    const d = { ...draft, enabled: false };
    setDraft(d);
    await persist(d);
  };

  useEffect(() => {
    if (!assist || !dirty || status.saving || !draft.id || draft === rejected.current) return;
    const timer = setTimeout(() => void persist(draft), AUTOSAVE_MS);
    return () => clearTimeout(timer);
  }, [assist, dirty, status.saving, draft]);

  // 사람의 편집 우선: 미저장 편집이 있는 동안 빌더의 고치는 동작은 USER_EDITING으로 거절된다.
  // 저장·버림·다시 불러옴·화면을 떠남으로 풀린다. 새 노드 자리만 잡아 준 것은 편집으로 치지 않는다.
  const editing = edited && !!draft.id;
  useEffect(() => {
    if (!editing) return;
    const id = draft.id;
    api.setEditing(id, true).catch(() => {});
    return () => void api.setEditing(id, false).catch(() => {});
  }, [editing, draft.id]);

  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    const undo = api.onTestUndoFailed((p) => {
      if (p.id !== now.current.draft.id) return;
      api.getRun(p.runId).then(
        (run) => !gone && setStuck((run.undo?.skipped ?? []).map(([op]) => `${op.from} → ${op.to}`)),
        (e) => !gone && setStatus({ error: String(e) }),
      );
    });
    void api
      .onWorkflowChanged((p) => {
        const { draft, base, edited } = now.current;
        if (p.id !== draft.id || p.updatedAt === base.updatedAt) return;
        setAssist(true);
        // 빌더는 미저장 편집이 있으면 고치지 않는다 — 그래도 편집이 있었다면(사람의 [켜기], 확인과 저장 사이 틈새) 알린다.
        void reload().then(() => edited && setStatus({ error: t("sync.reloaded") }));
      })
      .then(
        (u) => (gone ? u() : (off = u)),
        () => {},
      );
    return () => {
      gone = true;
      off?.();
      void undo.then((f) => f(), () => {});
      // 떠날 때 자동 저장을 기다리던 편집은 한 번 던지고 간다.
      const { draft, dirty, assist } = now.current;
      if (assist && dirty && draft.id && draft !== rejected.current) void save(draft).catch(() => {});
    };
  }, []); // 편집기가 떠 있는 동안 한 번만 건다 — 최신 값은 now로 읽는다.

  const banner = [...general.map((e) => e.message), status.error, testRun.error, ...(stuck.length ? [t("sync.undoFailed"), ...stuck] : [])].filter(
    (m): m is string => !!m,
  );
  const unsaved = dirty || !draft.id;
  // 취소한 실행은 실패로 보이지 않는다.
  const showRun = live.status !== "cancelled";
  const nodeRight = cover > 0 ? cover + 12 + 384 : 400;
  const showNode = (nodeId: string) => {
    setSelectedId(nodeId);
    // 노드 패널(w-96 384)은 채팅창 왼쪽(cover + 12)에 놓인다. 채팅창이 없으면 오른쪽 여백 16.
    setFocus((f) => ({ id: nodeId, nonce: (f?.nonce ?? 0) + 1, right: nodeRight }));
  };

  return (
    <div className="flex h-full flex-col bg-white">
      <Toolbar
        name={draft.name}
        onName={(name) => setDraft({ ...draft, name })}
        techInfo={techInfo}
        onTechInfo={setTechInfo}
        dirty={unsaved}
        enabled={draft.enabled}
        onEnabled={(enabled) => setDraft({ ...draft, enabled })}
        hideEnabled={wf.nodes.some((n) => n.event === "manual.run")}
        problems={errors.length}
        onBack={onBack}
        onOpenRuns={draft.id ? () => onOpenRuns(draft.id) : undefined}
        onSave={onSave}
        onRun={() => void testRun.run()}
        running={testRun.running}
        saving={!!status.saving}
      />
      {banner.length > 0 && (
        <ul className="flex flex-col gap-0.5 border-b border-danger-300 bg-danger-100 px-6 py-2 typo-caption1_semibold text-danger-700">
          {banner.map((m, i) => (
            <li key={i}>{m}</li>
          ))}
        </ul>
      )}
      <div ref={setArea} className="relative min-h-0 flex-1">
        <div className="absolute inset-0">
          <Canvas
            workflow={wf}
            errors={errors}
            statuses={live.nodes}
            selectedId={selectedId}
            onChange={setWorkflow}
            onSelect={setSelectedId}
            onNodeClick={setSelectedId}
            techInfo={techInfo}
            rightCover={cover}
            runErrors={showRun ? live.errors : undefined}
            runCodes={showRun ? live.codes : undefined}
            onOpenIntegrations={onOpenIntegrations}
            onOpenAiSettings={onOpenAiSettings}
            focus={focus}
          />
        </div>
        <PaletteButton onAdd={add} hasTrigger={wf.nodes.some(isTrigger)} blocked={!!swapOld || !!testRun.ask} />
        <AssistantPanel
          workflowId={draft.id}
          onOpenChange={(open) => void onAssistant(open)}
          onCoverChange={setCover}
          nodeNames={Object.fromEntries(draft.workflow.nodes.map((n) => [n.id, n.label || nodeTitle(n.event)]))}
        />
        <FinishCard workflowId={draft.id} version={base.updatedAt} />
        <HttpTestCard key={draft.id} workflowId={draft.id} />
        <RunErrorCard
          key={live.runId ?? ""}
          live={live}
          nodes={wf.nodes}
          onShow={showNode}
          onOpenIntegrations={onOpenIntegrations}
          onOpenAiSettings={onOpenAiSettings}
        />
        {swap && swapOld && (
          <ReplaceTriggerCard
            oldName={swapOld.label || nodeTitle(swapOld.event)}
            newName={nodeTitle(swap.event)}
            refs={referencingNodes(wf, swapOld.id).length}
            folder={swapOld.event === "local.folder_new_file" && wf.nodes.some((n) => n.event.startsWith("local.file_"))}
            turnsOff={draft.enabled && swap.event !== "manual.run"}
            onAnswer={answerSwap}
          />
        )}
        {testRun.ask && <HttpConfirm lines={testRun.ask} skipLabel={t("common.cancel")} onAnswer={testRun.answer} />}
        {selected && (
          <SidePanel
            right={cover > 0 ? cover + 12 : undefined}
            label={selected.label || nodeTitle(selected.event)}
            onClose={() => setSelectedId(null)}
            header={
              <>
                <NodeHead node={selected} step={wf.nodes.indexOf(selected) + 1} />
                {isLocal(selected.event) && <LocalBadge />}
                <button
                  type="button"
                  className="grid size-8 shrink-0 place-items-center rounded-[10px] text-neutral-500 transition-colors hover:bg-danger-100 hover:text-danger-700"
                  onClick={() => remove(selected.id)}
                  aria-label={t("editor.deleteNode")}
                  title={t("editor.deleteNode")}
                >
                  <Trash2 size={16} aria-hidden="true" />
                </button>
              </>
            }
          >
            <div className="flex-1 p-5">
              <ConfigPanel key={`${selected.id}:${selected.event}`} node={selected} workflow={wf} onChange={setNode} onOpenIntegrations={onOpenIntegrations} />
            </div>
            <NodeTestSection
              key={`${selected.id}:${selected.event}`}
              workflowId={draft.id || null}
              workflow={wf}
              node={selected}
              onOpen={showNode}
              lastFailed={showRun && live.nodes[selected.id] === "failed"}
              lastError={live.errors[selected.id]}
              chunk={live.chunks[selected.id]}
              host={area}
            />
          </SidePanel>
        )}
      </div>
    </div>
  );
}
