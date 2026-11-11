// Tauri 명령 타입 래퍼 — 타입은 러스트 직렬화(camelCase)와 1:1.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type Obj = Record<string, unknown>;

// ieum-engine model.rs — 모르는 키는 그대로 왕복한다.
export interface WfNode {
  id: string;
  event: string;
  type?: string;
  label?: string;
  description?: string;
  position?: { x: number; y: number };
  config: Obj;
  [extra: string]: unknown;
}
export interface WfEdge {
  id?: string;
  source: string;
  target: string;
  conditionType?: string;
  [extra: string]: unknown;
}
export interface Workflow {
  nodes: WfNode[];
  edges: WfEdge[];
  [extra: string]: unknown;
}

// store.rs / supervisor.rs
export interface StoredWorkflow {
  id: string;
  name: string;
  enabled: boolean;
  workflow: Workflow;
  updatedAt: string;
}
export interface WorkflowEntry extends StoredWorkflow {
  watch: "off" | "watching" | "watch_error";
  watchMessage: string | null;
}

// validate.rs
export interface ValidationError {
  nodeId: string | null;
  field: string | null;
  code: string;
  message: string;
}

// event.rs / run.rs
export type RunStatus = "succeeded" | "failed" | "cancelled" | "rejected";
export type NodeStatus = "succeeded" | "failed" | "skipped" | "notRun";
export interface NodeRecord {
  status: NodeStatus;
  output: Obj | null;
  error: string | null;
}
export type RejectReason =
  | { kind: "TOO_LARGE" }
  | { kind: "UNKNOWN_EVENT"; event: string }
  | { kind: "CYCLE" }
  | { kind: "INVALID" };
export interface RunResult {
  runId: string;
  status: RunStatus;
  validationErrors: ValidationError[];
  rejectReason: RejectReason | null;
  nodes: Record<string, NodeRecord>;
}
export type ExecEvent =
  | { kind: "runStarted"; runId: string }
  | { kind: "nodeStarted"; nodeId: string }
  | { kind: "nodeChunk"; nodeId: string; text: string }
  | { kind: "nodeSucceeded"; nodeId: string; output: Obj }
  | { kind: "nodeFailed"; nodeId: string; message: string }
  | { kind: "nodeSkipped"; nodeId: string }
  | { kind: "runFinished"; runId: string; status: RunStatus };
/** lib.rs가 내보내는 `exec-event` 모양 */
export interface ExecPayload {
  workflowId: string;
  event: ExecEvent;
}

// ieum-local journal.rs / undo.rs
export interface FileOp {
  kind: "move" | "rename" | "trash";
  from: string;
  to: string;
  fileId?: { dev: number; ino: number } | null;
}
export type UndoSkip =
  | { reason: "missingNow" }
  | { reason: "originalOccupied" }
  | { reason: "replaced" }
  | { reason: "failed"; message: string };
export interface UndoReport {
  restored: FileOp[];
  skipped: [FileOp, UndoSkip][];
}
export interface StoredRun {
  run: RunResult;
  workflowId: string;
  startedAt: string;
  finishedAt: string;
  fileOps: FileOp[];
  allowedRoots: string[];
  undo: UndoReport | null;
}
export interface RunSummary {
  runId: string;
  workflowId: string;
  status: RunStatus;
  startedAt: string;
  finishedAt: string;
  fileOps: number;
  undone: boolean;
}
export interface Settings {
  language: "system" | "ko" | "en";
  autostart: boolean;
  firstRunDone: boolean;
  serverUrl: string;
}

export const api = {
  listWorkflows: () => invoke<WorkflowEntry[]>("list_workflows"),
  saveWorkflow: (wf: StoredWorkflow) => invoke<StoredWorkflow>("save_workflow", { wf }),
  deleteWorkflow: (id: string) => invoke<void>("delete_workflow", { id }),
  validateWorkflow: (wf: Workflow) => invoke<ValidationError[]>("validate_workflow", { wf }),
  /** 실행이 끝나면 run id를 돌려준다. */
  runNow: (id: string, filePath: string) => invoke<string>("run_now", { id, filePath }),
  listRuns: (workflowId?: string) => invoke<RunSummary[]>("list_runs", { workflowId: workflowId ?? null }),
  getRun: (runId: string) => invoke<StoredRun>("get_run", { runId }),
  undoRun: (runId: string) => invoke<UndoReport>("undo_run", { runId }),
  getSettings: () => invoke<Settings>("get_settings"),
  /** 폴더·파일 고르기(tauri-plugin-dialog, 권한 `dialog:allow-open`). 취소하면 null. */
  pickPath: (directory: boolean) =>
    invoke<string | null>("plugin:dialog|open", { options: { directory, multiple: false } }),
  onExecEvent: (cb: (p: ExecPayload) => void): Promise<UnlistenFn> =>
    listen<ExecPayload>("exec-event", (e) => cb(e.payload)),
};
