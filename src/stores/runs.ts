import { create } from "zustand";
import { api, type ExecPayload, type RunStatus, type RunSummary } from "../api";

export type NodeRunStatus = "running" | "succeeded" | "failed" | "skipped";

/** 한 워크플로우의 지금(또는 마지막) 실행 생중계 상태. */
export interface Live {
  workflowId: string;
  runId: string | null;
  status: RunStatus | "running" | null;
  nodes: Record<string, NodeRunStatus>;
  errors: Record<string, string>;
  chunks: Record<string, string>;
}

export const emptyLive = (workflowId: string): Live => ({
  workflowId,
  runId: null,
  status: null,
  nodes: {},
  errors: {},
  chunks: {},
});

const TERMINAL: NodeRunStatus[] = ["succeeded", "failed", "skipped"];

/**
 * 멱등: 같은 이벤트가 두 번 와도 결과가 같다. 다른 워크플로우·다른 실행·끝난 실행의 이벤트는 거른다.
 * 워크플로우 하나의 실행은 차례로만 돌므로(supervisor 줄) `runStarted` ~ `runFinished` 사이 노드 이벤트는 그 실행 것이다.
 * 취소된 노드는 따로 구분하지 않고 실패 + 오류 메시지 그대로(코디네이터 결정).
 */
export function applyExecEvent(live: Live, { workflowId, event: e }: ExecPayload): Live {
  if (workflowId !== live.workflowId) return live;
  if (e.kind === "runStarted")
    return e.runId === live.runId ? live : { ...emptyLive(workflowId), runId: e.runId, status: "running" };
  if (e.kind === "runFinished")
    return e.runId === live.runId && live.status === "running" ? { ...live, status: e.status } : live;
  if (live.status !== "running") return live;
  const cur = live.nodes[e.nodeId];
  if (cur && TERMINAL.includes(cur)) return live;
  switch (e.kind) {
    case "nodeStarted":
      return cur ? live : { ...live, nodes: { ...live.nodes, [e.nodeId]: "running" } };
    case "nodeChunk":
      // ponytail: 이벤트에 순번이 없어 같은 조각이 두 번 오면 두 번 붙는다 — 구독을 한 번만 걸어 막는다.
      return { ...live, chunks: { ...live.chunks, [e.nodeId]: (live.chunks[e.nodeId] ?? "") + e.text } };
    case "nodeSucceeded":
      return { ...live, nodes: { ...live.nodes, [e.nodeId]: "succeeded" } };
    case "nodeSkipped":
      return { ...live, nodes: { ...live.nodes, [e.nodeId]: "skipped" } };
    case "nodeFailed":
      return {
        ...live,
        nodes: { ...live.nodes, [e.nodeId]: "failed" },
        errors: { ...live.errors, [e.nodeId]: e.message },
      };
  }
}

interface RunsState {
  runs: RunSummary[];
  error: string | null;
  /** workflowId → 생중계 상태 */
  live: Record<string, Live>;
  load: () => Promise<void>;
  apply: (p: ExecPayload) => void;
}

export const useRuns = create<RunsState>((set) => ({
  runs: [],
  error: null,
  live: {},
  load: async () => {
    try {
      set({ runs: await api.listRuns(), error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },
  apply: (p) =>
    set((s) => ({
      live: { ...s.live, [p.workflowId]: applyExecEvent(s.live[p.workflowId] ?? emptyLive(p.workflowId), p) },
    })),
}));

/** 앱 시작 때 한 번만 건다(두 번 걸면 조각이 겹친다). */
let subscribed: Promise<unknown> | null = null;
export function subscribeExecEvents() {
  subscribed ??= api.onExecEvent((p) => {
    useRuns.getState().apply(p);
    if (p.event.kind === "runFinished") void useRuns.getState().load();
  });
}
