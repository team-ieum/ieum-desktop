import {
  applyEdgeChanges,
  applyNodeChanges,
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
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
import { Check, CircleAlert, CircleDashed, CircleSlash, Laptop, LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ValidationError, Workflow, WfNode } from "../api";
import { t, tDyn } from "../i18n";
import { branchHandles, DEFS, isLocal } from "../nodes/defs";
import { nodeMeta, toneStyle } from "../nodes/meta";
import type { NodeRunStatus } from "../stores/runs";
import { AnimatedEdge, type EdgeData } from "./AnimatedEdge";

/** 로컬 노드 표시 — 웹 이음 상태 배지(DashboardStatusBadge)와 같은 알약. */
export function LocalBadge() {
  return (
    <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-main-light-blue bg-main-light-blue px-2.5 py-1 typo-caption1_semibold text-main-deep-blue">
      <Laptop size={12} aria-hidden="true" />
      {t("editor.local")}
    </span>
  );
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
      className={`inline-flex size-[1.55rem] shrink-0 items-center justify-center rounded-full ${STATUS_CLASS[status ?? "idle"]}`}
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

const isAi = (event: string, kind: string) => kind === "AI" || event === "local.ai" || event.startsWith("ai.");

/** AI 노드 알약 — Claude 구독이면 공급원 이름, API면 모델 이름. */
export function modelLabel(config: WfNode["config"]): string {
  if (config.provider === "cli:claude") return t("editor.claudePlan");
  return typeof config.model === "string" && config.model ? config.model : t("editor.noModel");
}

type Data = { wf: WfNode; step: number; errors: string[]; status?: NodeRunStatus };
type IeumNode = Node<Data, "ieum">;

// 웹 WorkflowNode HANDLE_CLASS.
const HANDLE =
  "!z-[3] !size-4.5 !cursor-crosshair !border-[3px] !border-white !bg-(--node-color) !shadow-[0_0_0_2px_var(--node-color),0_3px_8px_rgba(28,54,65,0.2)]";

function handleLabel(h: { id: string; name: string }) {
  if (h.id === "true") return t("canvas.true");
  if (h.id === "else" || h.id === "false") return t("canvas.otherwise");
  return h.name || t("editor.branchName");
}

/** 웹 이음 WorkflowNode와 같은 카드. */
function NodeCard({ data, selected, isConnectable }: NodeProps<IeumNode>) {
  const { wf, step, errors, status } = data;
  const kind = DEFS[wf.event]?.node_type ?? wf.type ?? "";
  const meta = nodeMeta(wf.event, wf.type);
  const branches = kind === "CONDITION" ? branchHandles(wf) : null;
  const title = wf.label || nodeTitle(wf.event);
  return (
    <article
      style={toneStyle(meta)}
      className={`relative w-66 rounded-[1.2rem] border-2 border-(--node-color) bg-white shadow-[0.5rem_0.6rem_0_var(--node-tint),0_14px_30px_rgba(65,52,139,0.1)] ${
        selected ? "outline-2 outline-offset-4 outline-main-blue" : ""
      }`}
    >
      {errors.length > 0 && (
        <ul className="absolute bottom-full left-0 right-0 mb-2 flex flex-col gap-0.5 rounded-brand-sm border border-danger-300 bg-danger-100 px-2.5 py-1.5 typo-caption2_semibold text-danger-700">
          {errors.map((m, i) => (
            <li key={i}>{m}</li>
          ))}
        </ul>
      )}
      {kind !== "TRIGGER" && <Handle type="target" position={Position.Left} className={HANDLE} isConnectable={isConnectable} />}
      <header className="flex min-h-[2.9rem] items-center gap-[0.45rem] rounded-t-[1rem] border-b border-b-[color-mix(in_srgb,var(--node-color)_28%,white)] bg-(--node-tint) px-[0.65rem] py-[0.45rem] text-[0.72rem] font-bold text-(--node-color)">
        <span className="grid size-8 shrink-0 place-items-center rounded-[0.65rem] bg-(--node-color) text-white [box-shadow:0_4px_0_color-mix(in_srgb,var(--node-color)_78%,black)]">
          {meta.icon}
        </span>
        <span className="min-w-0 truncate">{meta.label}</span>
        <span className="ml-auto flex items-center gap-1.5">
          {isLocal(wf.event) && <LocalBadge />}
          <StatusIcon status={status} />
        </span>
      </header>
      <div className="flex flex-col gap-[0.55rem] p-[0.85rem]">
        <div className="flex items-center gap-[0.55rem]">
          <span data-step={step} className="grid size-6 shrink-0 place-items-center rounded-full bg-(--node-color) text-[0.7rem] font-bold text-white">
            <span aria-hidden="true">{step}</span>
            <span className="sr-only">{t("editor.step").replace("{n}", String(step))}</span>
          </span>
          <h3 className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[0.9rem] font-bold text-[#27273b]" title={title}>
            {title}
          </h3>
        </div>
        <p className="m-0 min-h-[1.95rem] text-[0.68rem] leading-[1.45] text-[#6d6b7c]">{tDyn(`node.${wf.event}.desc`) ?? wf.description}</p>
        {isAi(wf.event, kind) && (
          <span className="inline-flex min-h-8 w-full items-center rounded-[0.65rem] border border-[color-mix(in_srgb,var(--node-color)_30%,white)] bg-(--node-tint) px-[0.6rem] py-[0.28rem] text-[0.67rem] font-bold text-(--node-color)">
            {modelLabel(wf.config)}
          </span>
        )}
      </div>
      {branches ? (
        <ul className="flex flex-col gap-1 border-t border-neutral-200 px-[0.85rem] py-2">
          {branches.map((h) => (
            <li key={h.id} className="relative flex justify-end pr-2 text-[0.72rem] font-semibold text-neutral-600">
              {handleLabel(h)}
              <Handle id={h.id} type="source" position={Position.Right} className={`${HANDLE} !-right-[0.85rem]`} isConnectable={isConnectable} />
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
const edgeTypes = { animated: AnimatedEdge };

// 웹 WorkFlowPage WORKFLOW_CANVAS_CLASS 그대로.
const WORKFLOW_CANVAS_CLASS = [
  "bg-[#f7f6fc]",
  "[&_.react-flow__controls]:overflow-hidden [&_.react-flow__controls]:rounded-[0.7rem]",
  "[&_.react-flow__controls]:border [&_.react-flow__controls]:border-[#d8e3e7]",
  "[&_.react-flow__controls]:shadow-[0_8px_20px_rgba(43,72,86,0.09)]",
  "[&_.react-flow__controls-button]:size-8 [&_.react-flow__controls-button]:border-b-[#e5ecef]",
  "[&_.react-flow__minimap]:rounded-xl [&_.react-flow__minimap]:border [&_.react-flow__minimap]:border-[#d8e3e7]",
  "[&_.react-flow__minimap]:bg-white/90 [&_.react-flow__minimap]:shadow-[0_8px_20px_rgba(43,72,86,0.08)]",
  "[&_.react-flow__edge.selected_.react-flow__edge-path]:stroke-[2px]",
  "max-[560px]:[&_.react-flow__minimap]:hidden",
].join(" ");

const MARKER = { type: MarkerType.ArrowClosed, color: "#a8b0ba" };
const minimapMeta = (n: Node) => nodeMeta((n.data as Data).wf.event, (n.data as Data).wf.type);

export interface CanvasProps {
  workflow: Workflow;
  /** 웹 이음 탭(16b) — 이동·삭제·연결·선택 콜백을 부르지 않는다. */
  readOnly?: boolean;
  errors?: ValidationError[];
  /** 노드별 실행 상태(없으면 대기 동그라미). 실행 중인 노드로 들어가는 엣지에 흐름 점선. */
  statuses?: Record<string, NodeRunStatus>;
  selectedId?: string | null;
  onChange?: (wf: Workflow) => void;
  onSelect?: (id: string | null) => void;
}

const edgeId = (e: Workflow["edges"][number]) => e.id ?? `${e.source}-${e.target}-${e.conditionType ?? ""}`;

export function Canvas({ workflow, readOnly = false, errors, statuses, selectedId, onChange, onSelect }: CanvasProps) {
  const [nodes, setNodes] = useState<IeumNode[]>([]);
  const [edges, setEdges] = useState<Edge<EdgeData>[]>([]);
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
          step: i + 1,
          errors: (errors ?? []).filter((e) => e.nodeId === n.id).map((e) => e.message),
          status: statuses?.[n.id],
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
        type: "animated",
        markerEnd: MARKER,
        data: { flowing: statuses?.[e.target] === "running", readOnly },
        selected: !readOnly && sel.has(edgeId(e)),
      }));
    });
  }, [workflow.edges, statuses, readOnly]);

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

  const onEdgesChange = (changes: EdgeChange<Edge<EdgeData>>[]) => {
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
      className={WORKFLOW_CANVAS_CLASS}
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      nodesDraggable={!readOnly}
      nodesConnectable={!readOnly}
      elementsSelectable={!readOnly}
      deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
      fitView
      // 노드를 0.78보다 작게 줄이지 않는다 — 다 안 들어가면 끌어서(패닝) 본다.
      fitViewOptions={{ padding: 0.24, minZoom: 0.78, maxZoom: 1 }}
      minZoom={0.5}
      maxZoom={1.4}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1.5} color="rgba(74, 101, 124, 0.18)" />
      <Controls showInteractive={false} />
      <MiniMap pannable zoomable nodeStrokeWidth={3} nodeColor={(n) => minimapMeta(n).tint} nodeStrokeColor={(n) => minimapMeta(n).color} />
    </ReactFlow>
  );
}
