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
  /** 폴더 감시·수동(Rust는 Task 4). 옛 기록은 없음. */
  trigger?: "watch" | "manual" | null;
}
export interface Settings {
  language: "system" | "ko" | "en";
  autostart: boolean;
  firstRunDone: boolean;
  serverUrl: string;
  /** 로그인된(마지막 활성) 계정. 화면이 보내도 Rust가 저장값을 지킨다. */
  currentAccount?: AccountId | null;
  notificationsSeenAt?: string;
}
/** 계정 = 서버 + 이메일. */
export interface AccountId {
  email: string;
  serverUrl: string;
}

// commands.rs
export interface Providers {
  clis: { id: string; path: string; version: string }[];
  apiKeys: string[];
}
export type ApiProvider = "anthropic" | "openai" | "gemini";
/** 키체인 계정별 저장 여부 — 값은 오지 않는다. */
export type KeyState = "stored" | "missing" | "error";
export type KeychainStatus = Record<ApiProvider | "ieum-session", KeyState>;

// web/* — 오류는 `{code, message?}`.
export interface WebError {
  code:
    | "AUTH_FAILED"
    | "AUTH_LOCKED"
    | "AUTH_EXPIRED"
    | "EXTERNAL_EXPIRED"
    | "NETWORK"
    | "SERVER"
    | "EMAIL_TAKEN"
    | "CODE_INVALID"
    | "BAD_INPUT";
  message?: string;
}
export interface WebAccount {
  email: string;
  serverUrl: string;
}
export interface WebWorkflowSummary {
  id: string;
  name: string;
  description?: string | null;
  active: boolean;
  triggerType?: string | null;
  updatedAt?: string | null;
  /** BE `WorkflowServiceType` 이름(`GOOGLE_DRIVE` 등). */
  services?: string[] | null;
  nodeCount?: number | null;
}
// web/dashboard.rs(Task 5)
export interface WebDashboardSummary {
  metrics: { todayRuns: number; percentageChange: number; averageDurationSeconds: number; successRate: number };
  hourlyCounts: Record<string, number>;
  workflowStats: { total: number; active: number; inactive: number; errored: number; running: number };
}
export interface WebDashboardExecution {
  id: string;
  workflowId: string;
  workflowName: string;
  status: string;
  durationSeconds: number | null;
  triggerType: string | null;
  startedAt: string | null;
  finishedAt: string | null;
}
export interface WebDashboardError {
  executionId: string;
  workflowId: string;
  workflowName: string;
  failedNodeId: string | null;
  errorMessage: string | null;
  startedAt: string | null;
}
export interface WebWorkflow extends Workflow {
  id: string;
  name: string;
}
export interface Page<T> {
  content: T[];
  hasNext: boolean;
  nextCursor: string | null;
}
export interface WebExecution {
  id: string;
  workflowId: string;
  status: string;
  startedAt?: string | null;
  finishedAt?: string | null;
  [extra: string]: unknown;
}
export interface WebNodeLog {
  id: string;
  nodeId: string;
  status: string;
  [extra: string]: unknown;
}
/** BE `ExecutionEvent`(type·nodeId·status·errorMessage·executionStatus …). */
export interface WebEvent {
  type: string;
  nodeId?: string;
  status?: string;
  errorMessage?: string;
  executionStatus?: string;
  [extra: string]: unknown;
}
/** `web-exec-event` — 이벤트 하나, 또는 밀려남(`stopped`)·폴백 최종 상태(`logs`)·오류(`error`). */
export type WebExecPayload = { executionId: string } & (
  | { event: WebEvent }
  | { stopped: true }
  | { logs: WebNodeLog[] }
  | { error: WebError }
);

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
  setSettings: (s: Settings) => invoke<void>("set_settings", { s }),
  detectProviders: () => invoke<Providers>("detect_providers"),
  keychainStatus: () => invoke<KeychainStatus>("keychain_status"),
  setApiKey: (provider: ApiProvider, key: string) => invoke<void>("set_api_key", { provider, key }),
  deleteApiKey: (provider: ApiProvider) => invoke<void>("delete_api_key", { provider }),
  webLogin: (email: string, password: string) => invoke<WebAccount>("web_login", { email, password }),
  webLogout: () => invoke<void>("web_logout"),
  webAccount: () => invoke<WebAccount | null>("web_account"),
  /** 관문 판단용 활성(없으면 저장된) 계정 — 네트워크 안 씀. */
  currentAccount: () => invoke<AccountId | null>("current_account"),
  // 가입(Rust 명령은 Task 5).
  sendCode: (email: string) => invoke<void>("web_send_code", { email }),
  verifyCode: (email: string, code: string) => invoke<void>("web_verify_code", { email, code }),
  register: (email: string, password: string, name: string) =>
    invoke<WebAccount>("web_register", { email, password, name }),
  webListWorkflows: () => invoke<WebWorkflowSummary[]>("web_list_workflows"),
  webGetWorkflow: (id: string) => invoke<WebWorkflow>("web_get_workflow", { id }),
  webListExecutions: (workflowId: string, cursor: string | null) =>
    invoke<Page<WebExecution>>("web_list_executions", { workflowId, cursor }),
  webExecutionLogs: (workflowId: string, executionId: string) =>
    invoke<WebNodeLog[]>("web_execution_logs", { workflowId, executionId }),
  webWatchExecution: (workflowId: string, executionId: string) =>
    invoke<void>("web_watch_execution", { workflowId, executionId }),
  webUnwatchExecution: (executionId: string) => invoke<void>("web_unwatch_execution", { executionId }),
  // 대시보드(Rust 명령은 Task 5).
  dashboardSummary: () => invoke<WebDashboardSummary>("web_dashboard_summary"),
  dashboardExecutions: (cursor: string | null) =>
    invoke<Page<WebDashboardExecution>>("web_dashboard_executions", { cursor }),
  dashboardErrors: (cursor: string | null) => invoke<Page<WebDashboardError>>("web_dashboard_errors", { cursor }),
  /** 웹 워크플로우 화면을 기본 브라우저로(Rust 명령은 Task 5). */
  openWebWorkflow: (id: string) => invoke<void>("open_web_workflow", { id }),
  /** 서버 주소 + `/main`을 기본 브라우저로. */
  openWebEditor: () => invoke<void>("open_web_editor"),
  /** 설치·사용 안내 문서를 기본 브라우저로(Rust 명령은 Task 5). */
  openHelp: () => invoke<void>("open_help"),
  onWebExecEvent: (cb: (p: WebExecPayload) => void): Promise<UnlistenFn> =>
    listen<WebExecPayload>("web-exec-event", (e) => cb(e.payload)),
  /** 폴더·파일 고르기(tauri-plugin-dialog, 권한 `dialog:allow-open`). 취소하면 null. */
  pickPath: (directory: boolean) =>
    invoke<string | null>("plugin:dialog|open", { options: { directory, multiple: false } }),
  onExecEvent: (cb: (p: ExecPayload) => void): Promise<UnlistenFn> =>
    listen<ExecPayload>("exec-event", (e) => cb(e.payload)),
};
