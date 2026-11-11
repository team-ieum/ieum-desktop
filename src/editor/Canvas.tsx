import {
  applyEdgeChanges,
  applyNodeChanges,
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
  type NodeProps,
  type NodeSelectionChange,
} from "@xyflow/react";
import {
  Bot,
  Check,
  CircleAlert,
  CircleDashed,
  CircleSlash,
  FolderInput,
  FolderOpen,
  GitBranch,
  Laptop,
  LoaderCircle,
  PenLine,
  Trash2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ValidationError, Workflow, WfNode } from "../api";
import { t, tDyn } from "../i18n";
import { branchHandles, DEFS, isLocal } from "../nodes/defs";
import type { NodeRunStatus } from "../stores/runs";

/** 로컬 노드 표시 — 웹 이음 상태 배지(DashboardStatusBadge)와 같은 알약. */
export function LocalBadge() {
  return (
    <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-main-light-blue bg-main-light-blue px-2.5 py-1 typo-caption1_semibold text-main-deep-blue">
      <Laptop size={12} aria-hidden="true" />
      {t("editor.local")}
    </span>
  );
}

export const TONE: Record<string, string> = {
  TRIGGER: "[--node-color:#2e8b68] [--node-tint:#dff2e9]",
  AI: "[--node-color:#6d5ce7] [--node-tint:#eeeafd]",
  ACTION: "[--node-color:#287fa4] [--node-tint:#e2f2f8]",
  CONDITION: "[--node-color:#9a6700] [--node-tint:#fff1d6]",
};
const FALLBACK_TONE = "[--node-color:#596b78] [--node-tint:#eef2f4]";

export function NodeIcon({ event, size = 18 }: { event: string; size?: number }) {
  const Icon =
    {
      "local.folder_new_file": FolderOpen,
      "local.ai": Bot,
      "local.file_move": FolderInput,
      "local.file_rename": PenLine,
      "local.file_trash": Trash2,
      "branch.if": GitBranch,
    }[event] ?? CircleDashed;
  return <Icon size={size} aria-hidden="true" />;
}

const STATUS_CLASS: Record<NodeRunStatus | "idle", string> = {
  idle: "bg-[#eef1f3] text-[#5a6975]",
  running: "bg-[#e1f3f7] text-[#087a95]",
  succeeded: "bg-[#e5f3ec] text-[#287357]",
  failed: "bg-[#fff0ec] text-[#a84c36]",
  skipped: "bg-[#eef1f3] text-[#5a6975]",
};

export function StatusIcon({ status }: { status?: NodeRunStatus }) {
  const Icon = { running: LoaderCircle, succeeded: Check, failed: CircleAlert, skipped: CircleSlash }[status ?? "running"];
  return (
    <span
      className={`inline-flex size-6 shrink-0 items-center justify-center rounded-full ${STATUS_CLASS[status ?? "idle"]}`}
      role="img"
      aria-label={status ? t(`status.${status}`) : undefined}
      title={status ? t(`status.${status}`) : undefined}
    >
      {status ? (
        <Icon size={14} className={status === "running" ? "animate-spin motion-reduce:animate-none" : ""} aria-hidden="true" />
      ) : (
        <CircleDashed size={14} aria-hidden="true" />
      )}
    </span>
  );
}

export const nodeTitle = (event: string) => tDyn(`node.${event}.title`) ?? event;

type Data = { wf: WfNode; errors: string[]; status?: NodeRunStatus; showStatus: boolean };
type IeumNode = Node<Data, "ieum">;

const HANDLE = "!size-3.5 !border-2 !border-white !bg-(--node-color)";

function handleLabel(h: { id: string; name: string }) {
  if (h.id === "true") return t("canvas.true");
  if (h.id === "else" || h.id === "false") return t("canvas.otherwise");
  return h.name || t("editor.branchName");
}

function NodeCard({ data, selected, isConnectable }: NodeProps<IeumNode>) {
  const { wf, errors, status, showStatus } = data;
  const def = DEFS[wf.event];
  const kind = def?.node_type ?? wf.type ?? "";
  const branches = kind === "CONDITION" ? branchHandles(wf) : null;
  return (
    <article
      className={`relative w-64 rounded-[1.2rem] border-2 border-(--node-color) bg-white shadow-[0.4rem_0.5rem_0_var(--node-tint)] ${
        TONE[kind] ?? FALLBACK_TONE
      } ${selected ? "outline-2 outline-offset-4 outline-main-blue" : ""}`}
    >
      {errors.length > 0 && (
        <ul className="absolute bottom-full left-0 right-0 mb-2 flex flex-col gap-0.5 rounded-brand-sm border border-danger-300 bg-danger-100 px-2.5 py-1.5 typo-caption2_semibold text-danger-700">
          {errors.map((m, i) => (
            <li key={i}>{m}</li>
          ))}
        </ul>
      )}
      {kind !== "TRIGGER" && (
        <Handle type="target" position={Position.Left} className={HANDLE} isConnectable={isConnectable} />
      )}
      <header className="flex items-center gap-2 rounded-t-[1rem] border-b border-b-[color-mix(in_srgb,var(--node-color)_28%,white)] bg-(--node-tint) px-2.5 py-2 typo-caption1_bold text-(--node-color)">
        <span className="grid size-7 shrink-0 place-items-center rounded-[0.6rem] bg-(--node-color) text-white">
          <NodeIcon event={wf.event} size={16} />
        </span>
        <span className="min-w-0 truncate">{nodeTitle(wf.event)}</span>
        <span className="ml-auto flex items-center gap-1.5">
          {isLocal(wf.event) && <LocalBadge />}
          {showStatus && <StatusIcon status={status} />}
        </span>
      </header>
      <div className="flex flex-col gap-1 p-3">
        <h3 className="truncate typo-body2_bold text-neutral-800" title={wf.label}>
          {wf.label || nodeTitle(wf.event)}
        </h3>
        <p className="typo-caption1_regular text-neutral-500">{tDyn(`node.${wf.event}.desc`) ?? wf.description}</p>
      </div>
      {branches ? (
        <ul className="flex flex-col gap-1 border-t border-neutral-200 px-3 py-2">
          {branches.map((h) => (
            <li key={h.id} className="relative flex justify-end typo-caption1_semibold text-neutral-600">
              {handleLabel(h)}
              <Handle
                id={h.id}
                type="source"
                position={Position.Right}
                className={`${HANDLE} !-right-[1.2rem]`}
                isConnectable={isConnectable}
              />
            </li>
          ))}
        </ul>
      ) : (
        <Handle type="source" position={Position.Right} className={HANDLE} isConnectable={isConnectable} />
      )}
    </article>
  );
}

const nodeTypes = { ieum: NodeCard };

export interface CanvasProps {
  workflow: Workflow;
  /** 웹 이음 탭(16b) — 이동·삭제·연결·선택 콜백을 부르지 않는다. */
  readOnly?: boolean;
  errors?: ValidationError[];
  /** 주면 노드에 실행 상태 배지를 단다(실행 보기). */
  statuses?: Record<string, NodeRunStatus>;
  selectedId?: string | null;
  onChange?: (wf: Workflow) => void;
  onSelect?: (id: string | null) => void;
}

const edgeId = (e: Workflow["edges"][number]) => e.id ?? `${e.source}-${e.target}-${e.conditionType ?? ""}`;

export function Canvas({ workflow, readOnly = false, errors, statuses, selectedId, onChange, onSelect }: CanvasProps) {
  const [nodes, setNodes] = useState<IeumNode[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  // 한 틱에 여러 변경(엣지 삭제 → 노드 삭제)이 오면 앞 변경 결과 위에 쌓는다 — 렌더의 workflow는 낡았다.
  const latest = useRef(workflow);
  latest.current = workflow;
  const emit = (f: (w: Workflow) => Workflow) => {
    latest.current = f(latest.current);
    onChange?.(latest.current);
  };

  useEffect(() => {
    setNodes((prev) => {
      const measured = new Map(prev.map((n) => [n.id, n.measured]));
      return workflow.nodes.map((n, i) => ({
        id: n.id,
        type: "ieum",
        position: n.position ?? { x: i * 320, y: 0 },
        measured: measured.get(n.id),
        selected: !readOnly && n.id === selectedId,
        data: {
          wf: n,
          errors: (errors ?? []).filter((e) => e.nodeId === n.id).map((e) => e.message),
          status: statuses?.[n.id],
          showStatus: statuses !== undefined,
        },
      }));
    });
  }, [workflow.nodes, errors, statuses, selectedId, readOnly]);

  useEffect(() => {
    setEdges((prev) => {
      const sel = new Set(prev.filter((e) => e.selected).map((e) => e.id));
      return workflow.edges.map((e) => ({
        id: edgeId(e),
        source: e.source,
        target: e.target,
        sourceHandle: e.conditionType ?? null,
        selected: !readOnly && sel.has(edgeId(e)),
      }));
    });
  }, [workflow.edges, readOnly]);

  const onNodesChange = (changes: NodeChange<IeumNode>[]) => {
    setNodes((ns) => applyNodeChanges(readOnly ? changes.filter((c) => c.type === "dimensions") : changes, ns));
    if (readOnly) return;
    const moved = new Map<string, { x: number; y: number }>();
    const removed = new Set<string>();
    for (const c of changes) {
      if (c.type === "position" && c.position) moved.set(c.id, c.position);
      if (c.type === "remove") removed.add(c.id);
    }
    // 새로 고른 노드와 이전 노드 해제가 한 묶음으로 온다 — 고른 쪽이 이긴다.
    const picked = changes.find((c): c is NodeSelectionChange => c.type === "select" && c.selected);
    if (picked) onSelect?.(picked.id);
    else if (changes.some((c) => c.type === "select")) onSelect?.(null);
    if (moved.size || removed.size)
      emit((w) => ({
        ...w,
        nodes: w.nodes
          .filter((n) => !removed.has(n.id))
          .map((n) => (moved.has(n.id) ? { ...n, position: moved.get(n.id) } : n)),
        edges: w.edges.filter((e) => !removed.has(e.source) && !removed.has(e.target)),
      }));
  };

  const onEdgesChange = (changes: EdgeChange[]) => {
    if (readOnly) return;
    setEdges((es) => applyEdgeChanges(changes, es));
    const removed = new Set(changes.flatMap((c) => (c.type === "remove" ? [c.id] : [])));
    if (removed.size) emit((w) => ({ ...w, edges: w.edges.filter((e) => !removed.has(edgeId(e))) }));
  };

  const onConnect = (c: Connection) => {
    if (readOnly) return;
    const edge = { id: crypto.randomUUID(), source: c.source, target: c.target, ...(c.sourceHandle ? { conditionType: c.sourceHandle } : {}) };
    emit((w) => ({ ...w, edges: [...w.edges, edge] }));
  };

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      nodesDraggable={!readOnly}
      nodesConnectable={!readOnly}
      elementsSelectable={!readOnly}
      deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
      fitView
      proOptions={{ hideAttribution: true }}
    >
      <Background />
      <Controls showInteractive={false} />
    </ReactFlow>
  );
}
