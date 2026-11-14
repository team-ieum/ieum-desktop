import { Play, Plus, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type StoredWorkflow, type ValidationError, type WfNode, type WorkflowEntry } from "../api";
import { Canvas, LocalBadge, nodeTitle, StatusIcon } from "../editor/Canvas";
import { AssistantPanel } from "../editor/chat/AssistantPanel";
import { FinishCard } from "../editor/chat/FinishCard";
import { ConfigPanel } from "../editor/ConfigPanel";
import { Palette } from "../editor/Palette";
import { AUTOSAVE_MS, mergeRemote, STALE } from "../editor/sync";
import { Toolbar } from "../editor/Toolbar";
import { useTestRun } from "../editor/useTestRun";
import { t } from "../i18n";
import { isLocal, newNode, nextPosition } from "../nodes/defs";
import { nodeMeta, toneStyle } from "../nodes/meta";
import { emptyLive, useRuns } from "../stores/runs";
import { useWorkflows } from "../stores/workflows";

export const BUTTON =
  "inline-flex items-center gap-1.5 rounded-brand-sm border border-neutral-300 bg-white px-3 py-2 typo-button1_semibold text-neutral-700 hover:bg-neutral-100 disabled:opacity-40";
export const PRIMARY =
  "inline-flex items-center gap-1.5 rounded-brand-sm bg-main-blue px-3 py-2 typo-button1_semibold text-white hover:bg-main-deep-blue disabled:opacity-40";

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
export function SidePanel({ label, header, onClose, children }: { label: string; header: ReactNode; onClose: () => void; children: ReactNode }) {
  const [closing, setClosing] = useState(false);
  // 열 때는 CSS(@starting-style)로 밀려 들어오고, 닫을 때는 밀려 나간 뒤 없앤다. 움직임 줄이기면 바로.
  const close = () => {
    if (!window.matchMedia?.("(prefers-reduced-motion: no-preference)").matches) return onClose();
    setClosing(true);
    setTimeout(onClose, 150);
  };
  useEscape(close);
  return (
    <aside
      aria-label={label}
      className={`absolute top-4 right-4 bottom-4 z-20 flex w-96 flex-col overflow-y-auto rounded-2xl border border-neutral-200 bg-white shadow-[0_14px_40px_rgba(43,72,86,0.18)] transition-[translate,opacity] duration-150 ease-out starting:translate-x-6 starting:opacity-0 motion-reduce:transition-none ${
        closing ? "translate-x-6 opacity-0" : ""
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

/** 캔버스 왼쪽 가운데 [+] — 누르면 팔레트가 떠 있는 패널로. 바깥 클릭·Esc·노드 고르면 닫힌다. */
function PaletteButton({ onAdd }: { onAdd: (event: string) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEscape(() => setOpen(false), open);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);
  return (
    <div ref={ref} className="absolute left-4 top-1/2 z-10 -translate-y-1/2">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-label={t("editor.palette")}
        title={t("editor.palette")}
        aria-expanded={open}
        aria-haspopup="dialog"
        className="grid size-11 cursor-pointer place-items-center rounded-full bg-main-deep-blue text-white shadow-[0_8px_20px_rgba(41,83,124,0.3)] transition-opacity hover:opacity-90"
      >
        <Plus size={22} aria-hidden="true" />
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t("editor.palette")}
          className="absolute left-14 top-1/2 max-h-[min(40rem,80vh)] w-64 -translate-y-1/2 overflow-y-auto rounded-2xl border border-neutral-200 bg-white p-4 shadow-[0_14px_40px_rgba(43,72,86,0.18)]"
        >
          <Palette
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
}: {
  id: string | null;
  onBack: () => void;
  onOpenRuns: (id: string) => void;
  /** 저장본과 다른지 — 화면을 떠날 때 확인 줄을 띄우는 데 쓴다. */
  onDirty?: (dirty: boolean) => void;
  /** 지금 보여 주는 워크플로우 id — 새 캔버스는 선저장 뒤에 생긴다. 아직 없거나 편집기를 떠나면 "". */
  onIdChange?: (id: string) => void;
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
  const [techInfo, setTechInfo] = useState(false);
  const [errors, setErrors] = useState<ValidationError[]>([]);
  const [status, setStatus] = useState<{ saving?: boolean; error?: string }>({});
  const live = useRuns((s) => s.live[draft.id]) ?? emptyLive(draft.id);
  const testRun = useTestRun(draft.id);
  /** 빌더의 테스트가 되돌리지 못한 파일(`원래 자리 → 지금 자리`) — 터미널 입구에는 결과 카드가 없어 여기 배너로 보인다. */
  const [stuck, setStuck] = useState<string[]>([]);

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

  const add = (event: string) => {
    const node = newNode(event, nextPosition(wf.nodes), nodeTitle(event));
    setWorkflow({ ...wf, nodes: [...wf.nodes, node] });
    setSelectedId(node.id);
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
  const nodeStatus = selected && live.nodes[selected.id];

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
      <div className="relative min-h-0 flex-1">
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
          />
        </div>
        <PaletteButton onAdd={add} />
        <AssistantPanel
          workflowId={draft.id}
          onOpenChange={(open) => void onAssistant(open)}
          nodeNames={Object.fromEntries(draft.workflow.nodes.map((n) => [n.id, n.label || nodeTitle(n.event)]))}
        />
        <FinishCard workflowId={draft.id} version={base.updatedAt} />
        {selected && (
          <SidePanel
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
              <ConfigPanel key={selected.id} node={selected} workflow={wf} onChange={setNode} />
            </div>
            {/* 시험 실행은 워크플로우 전체 — 결과는 이 노드 것만(마지막 실행의 상태·오류·출력 조각). */}
            <section className="flex flex-col gap-2 border-t border-neutral-200 bg-neutral-50 p-5">
              <div className="flex items-center justify-between gap-2">
                <h3 className="typo-body2_bold text-neutral-800">{t("editor.test")}</h3>
                <button
                  type="button"
                  className={BUTTON}
                  onClick={() => void testRun.run()}
                  disabled={unsaved || testRun.running}
                  title={unsaved ? t("run.saveFirst") : t("editor.testHint")}
                >
                  <Play size={14} aria-hidden="true" />
                  {t("editor.test")}
                </button>
              </div>
              <p className="typo-caption1_regular text-neutral-500">{t("editor.testHint")}</p>
              {nodeStatus ? (
                <p className="flex items-center gap-2 typo-body3_semibold text-neutral-800">
                  <StatusIcon status={nodeStatus} />
                  {t(`status.${nodeStatus}`)}
                </p>
              ) : (
                <p className="typo-body3_regular text-neutral-500">{t("editor.noResult")}</p>
              )}
              {live.errors[selected.id] && <p className="typo-caption1_semibold text-danger-700">{live.errors[selected.id]}</p>}
              {live.chunks[selected.id] && (
                <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-brand-sm border border-neutral-200 bg-white p-2.5 font-mono text-xs text-neutral-700">
                  {live.chunks[selected.id]}
                </pre>
              )}
            </section>
          </SidePanel>
        )}
      </div>
    </div>
  );
}
