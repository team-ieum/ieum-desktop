// Tauri 명령 타입 래퍼 — 타입은 러스트 직렬화(camelCase)와 1:1.
import { Channel, invoke } from "@tauri-apps/api/core";
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
  /** AI에게 보여도 되는 오류 코드(`<CODE>` 또는 `<CODE> <칸 key>`). 옛 기록에는 없다. */
  errorCode?: string | null;
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
  trigger?: RunTrigger | null;
}
/** 폴더 감시·수동. 옛 기록은 없음. */
export type RunTrigger = "watch" | "manual" | "test";
export interface RunSummary {
  runId: string;
  workflowId: string;
  status: RunStatus;
  startedAt: string;
  finishedAt: string;
  fileOps: number;
  undone: boolean;
  trigger?: RunTrigger | null;
}
export interface Settings {
  language: "system" | "ko" | "en";
  autostart: boolean;
  firstRunDone: boolean;
  serverUrl: string;
  /** 로그인된(마지막 활성) 계정. 화면이 보내도 Rust가 저장값을 지킨다. */
  currentAccount?: AccountId | null;
  notificationsSeenAt?: string;
  /** 온보딩을 끝냈는가(기기 단위). */
  onboardingDone: boolean;
  notifyManualStart: boolean;
  notifyManualEnd: boolean;
  notifyWatchEnd: boolean;
  /** 확인 창 토글 — 옛 설정·테스트엔 없을 수 있어 `!== false`로 읽는다. */
  confirmDeleteWorkflow?: boolean;
  confirmDiscardUnsaved?: boolean;
}
/** 설정 [업데이트 확인] 결과(Rust `UpdateCheck`). */
export type UpdateCheck = { status: "dev" } | { status: "latest" } | { status: "installed"; version: string };
export type SystemSettingsPane = "files" | "notifications" | "login_items";
/** 시스템 알림 클릭으로 열 화면(Rust `notify::Target`). */
export type OpenTarget = { kind: "run"; runId: string } | { kind: "history" } | { kind: "workflow"; id: string };
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
/** 공급원별 키 저장 여부 — `listCredentials`에서 만든다. */
export type KeyState = "stored" | "missing" | "error";
export type KeychainStatus = Record<ApiProvider, KeyState>;

// credentials.rs — 키 값은 오지 않는다(끝 4자만).
export interface ApiKeyMeta {
  id: string;
  label: string;
  isDefault: boolean;
  createdAt: string;
  last4: string;
}
export interface CliAccountMeta {
  id: string;
  label: string;
  isDefault: boolean;
  createdAt: string;
  email: string | null;
}
export interface CredentialsView {
  apiKeys: Record<ApiProvider, ApiKeyMeta[]>;
  claudeAccounts: CliAccountMeta[];
  codexAccounts: CliAccountMeta[];
  keychainError: string | null;
}
export type CredentialKind = ApiProvider | "claude" | "codex";
/** 여러 계정을 넣을 수 있는 구독. 계정 id `system` = 이 Mac의 CLI 로그인. */
export type SubKind = "claude" | "codex";
/** 계정 추가: 터미널에 붙여 넣을 로그인 명령(경로는 큰따옴표). */
export interface CliAccountBegin {
  id: string;
  command: string;
}

export const keyStates = (v: CredentialsView): KeychainStatus => {
  const state = (p: ApiProvider): KeyState => (v.apiKeys[p]?.length ? "stored" : v.keychainError ? "error" : "missing");
  return { anthropic: state("anthropic"), openai: state("openai"), gemini: state("gemini") };
};

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

// usage.rs(Task 14) — 내 PC AI 노드가 쓴 토큰·예상 금액. usd null = 가격표에 없는 모델·구독.
export type UsageRange = "today" | "week" | "month";
export interface UsageSummary {
  totalUsd: number;
  totalInput: number;
  totalOutput: number;
  unpricedCalls: number;
  bySource: { source: string; input: number; output: number; usd: number | null }[];
  /** 공급원 + 키·계정 id. 옛 기록은 API `default`, 구독 `system`. */
  byCredential: { source: string; credential: string; input: number; output: number; usd: number | null }[];
  byWorkflow: { workflowId: string; input: number; output: number; usd: number | null }[];
  byDay: { date: string; usd: number; input: number; output: number }[];
  asOf: string;
}

// usage.rs `report` — 설정 › 통계 및 사용량. 합계는 전체 기간, 나머지는 최근 30일(히트맵 42일).
export type UsageProvider = "claude" | "openai" | "google";
export interface TokenParts {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}
export interface ProviderReport extends TokenParts {
  provider: UsageProvider;
  /** 구독 호출은 빠진 예상 금액. */
  usd: number;
  unpricedCalls: number;
  calls: number;
  runs: number;
  cacheReuse: number;
  noCacheCallShare: number;
  topModel: string | null;
  byDay: ({ date: string } & TokenParts)[];
  byModel: { model: string; tokens: number; calls: number }[];
  byWorkflow: { workflowId: string; name: string | null; tokens: number; calls: number; runs: number }[];
  recent: { at: string; workflowId: string; workflowName: string | null; model: string | null; credential: string; input: number; output: number; cache: number }[];
}
export interface UsageReport {
  updatedAt: string;
  trackingSince: string | null;
  totals: { runs: number; runSeconds: number; aiCalls: number };
  overview: {
    totalTokens: number;
    usd: number;
    unpricedCalls: number;
    activeDays: number;
    cacheShare: number;
    heat: { date: string; tokens: number }[];
    composition: TokenParts;
  };
  /** 늘 claude·openai·google 순. */
  providers: ProviderReport[];
}

// plan_usage.rs(Claude)·sub_usage.rs(Codex·Antigravity) — 구독 연결 상태와 창별 사용 비율.
export interface PlanWindow {
  /** Claude: five_hour·seven_day·seven_day_opus. Codex·Antigravity: session(하루 이하)·weekly(7일 이하)·monthly(Codex 무료 30일). */
  kind: "five_hour" | "seven_day" | "seven_day_opus" | "session" | "weekly" | "monthly";
  usedPct: number;
  resetsAt: string;
}
export interface PlanUsage {
  state: "ok" | "unavailable" | "not_logged_in";
  windows: PlanWindow[];
  checkedAt: string;
  /** 이 컴퓨터에 로그인된 구독 계정. 모르면 null. */
  accountEmail: string | null;
}

// ── 캔버스 채팅(ieum-local builder) ──
export interface BuilderReply {
  ok: boolean;
  code?: string;
  message?: string;
  data?: unknown;
}

export interface PendingFinish {
  hash: string;
  folder: string;
  actions: { nodeId: string; event: string; label: string; target: string | null }[];
}

/** 앱 안 터미널에 띄울 수 있는 CLI. */
export type TermTool = "claude" | "codex" | "agy";

export interface ChatFile {
  messages: { role: "user" | "assistant"; text: string; at: string }[];
  state: {
    outline: string[];
    baseline: Record<string, string> | null;
    tests: Record<string, { hash: string; status: "success" | "failed" | "skipped" }>;
    pendingFinish: PendingFinish | null;
  };
  usage: { at: string; usage: unknown }[];
}

/** 채팅 턴 응답에만 실리는 카드. 테스트 값은 없다 — 화면이 `runId`로 실행 기록을 읽는다. */
export type ChatCard =
  | { kind: "test"; nodeId: string; runId: string; status: string }
  | { kind: "pickFolder"; nodeId: string; field: string }
  | { kind: "finish" };
export interface ChatTurn {
  say: string;
  cards: ChatCard[];
  error: string | null;
}
export interface ChatAi {
  provider: string;
  credential: string | null;
  model: string | null;
}
export type ChatStage = { id: string; stage: "designing" | "testing" };

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
  listCredentials: () => invoke<CredentialsView>("list_credentials"),
  /** 첫 키면 기본. */
  addApiKey: (provider: ApiProvider, label: string, key: string) => invoke<ApiKeyMeta>("add_api_key", { provider, label, key }),
  renameCredential: (kind: CredentialKind, id: string, label: string) => invoke<void>("rename_credential", { kind, id, label }),
  setDefaultCredential: (kind: CredentialKind, id: string) => invoke<void>("set_default_credential", { kind, id }),
  deleteCredential: (kind: CredentialKind, id: string) => invoke<void>("delete_credential", { kind, id }),
  webLogin: (email: string, password: string) => invoke<WebAccount>("web_login", { email, password }),
  webLogout: () => invoke<void>("web_logout"),
  webAccount: () => invoke<WebAccount | null>("web_account"),
  /** 관문 판단용 활성(없으면 저장된) 계정 — 네트워크 안 씀. */
  currentAccount: () => invoke<AccountId | null>("current_account"),
  // 가입(Rust 명령은 Task 5).
  sendCode: (email: string) => invoke<void>("web_send_code", { email }),
  verifyCode: (email: string, code: string) => invoke<void>("web_verify_code", { email, code }),
  register: (email: string, password: string, name: string) =>
    invoke<void>("web_register", { email, password, name }),
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
  openDataFolder: () => invoke<void>("open_data_folder"),
  openSystemSettings: (pane: SystemSettingsPane) => invoke<void>("open_system_settings", { pane }),
  /** 공급자 키 발급 화면을 기본 브라우저로 — 주소는 Rust의 고정값. */
  openProviderConsole: (provider: ApiProvider) => invoke<void>("open_provider_console", { provider }),
  /** 앞 여부·알림 토글과 무관하게 "알림이 켜졌어요". */
  sendTestNotification: () => invoke<void>("send_test_notification"),
  checkUpdateNow: () => invoke<UpdateCheck>("check_update_now"),
  /** 시스템 알림을 누르면 열 화면. */
  onOpenTarget: (cb: (t: OpenTarget) => void): Promise<UnlistenFn> =>
    listen<OpenTarget>("open-target", (e) => cb(e.payload)),
  onWebExecEvent: (cb: (p: WebExecPayload) => void): Promise<UnlistenFn> =>
    listen<WebExecPayload>("web-exec-event", (e) => cb(e.payload)),
  /** 폴더·파일 고르기(tauri-plugin-dialog, 권한 `dialog:allow-open`). 취소하면 null. */
  pickPath: (directory: boolean) =>
    invoke<string | null>("plugin:dialog|open", { options: { directory, multiple: false } }),
  onExecEvent: (cb: (p: ExecPayload) => void): Promise<UnlistenFn> =>
    listen<ExecPayload>("exec-event", (e) => cb(e.payload)),
  // Rust 명령은 다음 세션(Task 14·15) — 그 전엔 실패하고 화면은 빈 상태를 보인다.
  usageSummary: (range: UsageRange) => invoke<UsageSummary>("usage_summary", { range }),
  usageReport: () => invoke<UsageReport>("usage_report"),
  /** 구독 계정 하나의 연결 상태·사용량(계정별 60초 캐시). */
  subscriptionUsage: (kind: SubKind, id: string) => invoke<PlanUsage>("subscription_usage", { kind, id }),
  /** 계정 폴더를 만들고 로그인 명령을 준다(확인 전엔 목록에 안 보임). */
  beginCliAccount: (kind: SubKind, label: string) => invoke<CliAccountBegin>("begin_cli_account", { kind, label }),
  /** 그 계정 폴더로 로그인을 확인 — 이메일이 보이면 목록에 추가, 아니면 `not_logged_in` 그대로. */
  confirmCliAccount: (kind: SubKind, id: string) => invoke<PlanUsage>("confirm_cli_account", { kind, id }),
  cancelCliAccount: (kind: SubKind, id: string) => invoke<void>("cancel_cli_account", { kind, id }),
  /** 이 Mac의 CLI 로그인(`system`)을 IEUM에서 숨기기·되살리기 — CLI 로그아웃은 하지 않는다. */
  setSystemHidden: (kind: SubKind, hidden: boolean) => invoke<void>("set_system_hidden", { kind, hidden }),
  antigravityPlanUsage: () => invoke<PlanUsage>("antigravity_plan_usage"),
  // ── 캔버스 채팅 ──
  getChat: (workflowId: string) => invoke<ChatFile>("get_chat", { workflowId }),
  pendingFinish: (workflowId: string) => invoke<PendingFinish | null>("pending_finish", { workflowId }),
  /** 확인 카드의 [켜기]/[아직] — 워크플로우를 켜는 유일한 길. */
  confirmFinish: (workflowId: string, accept: boolean) =>
    invoke<BuilderReply>("confirm_finish", { workflowId, accept }),
  /** 편집기의 미저장 편집 표시 — 켜져 있는 동안 빌더의 고치는 동작은 `USER_EDITING`으로 거절된다. */
  setEditing: (workflowId: string, editing: boolean) => invoke<void>("set_editing", { workflowId, editing }),
  /** 완료 확인 대기 알림 — 그 편집 화면이 떠 있고(`onScreen`) 창이 앞이면 Rust가 보내지 않는다. */
  finishNotice: (workflowId: string, onScreen: boolean) => invoke<void>("finish_notice", { workflowId, onScreen }),
  /** 테스트 되돌리기 실패 알림 — 판정은 `finishNotice`와 같다. */
  undoFailedNotice: (workflowId: string, onScreen: boolean) => invoke<void>("undo_failed_notice", { workflowId, onScreen }),
  onWorkflowChanged: (cb: (p: { id: string; updatedAt: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; updatedAt: string }>("workflow-changed", (e) => cb(e.payload)),
  onFinishPending: (cb: (p: { id: string; name: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; name: string }>("finish-pending", (e) => cb(e.payload)),
  // ── 채팅 패널 ──
  chatSend: (workflowId: string, text: string, ai: ChatAi) =>
    invoke<ChatTurn>("chat_send", { workflowId, text, provider: ai.provider, credential: ai.credential, model: ai.model }),
  /** 폴더 고르기 카드의 버튼 — 시스템 폴더 창을 연다. */
  chatPickFolder: (workflowId: string, nodeId: string, field: string) =>
    invoke<BuilderReply>("chat_pick_folder", { workflowId, nodeId, field }),
  onChatStage: (cb: (p: ChatStage) => void): Promise<UnlistenFn> => listen<ChatStage>("chat-stage", (e) => cb(e.payload)),
  onTestUndoFailed: (cb: (p: { id: string; runId: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; runId: string }>("test-undo-failed", (e) => cb(e.payload)),
  // ── 캔버스 채팅 C(터미널) — 화면은 도구 이름만 넘긴다. 실행 줄은 Rust가 만든다. ──
  /** 쓸 수 있는 CLI(설치 + 로그인)와 이 워크플로우에 살아 있는 세션. */
  termTools: (workflowId: string) =>
    invoke<{ tools: TermTool[]; running: TermTool | null }>("term_tools", { workflowId }),
  /** 터미널을 띄우거나 살아 있는 세션에 붙는다. 출력은 `onData`로 온다. */
  termSpawn: (workflowId: string, tool: TermTool, cols: number, rows: number, onData: (bytes: Uint8Array) => void) => {
    const ch = new Channel<ArrayBuffer>();
    ch.onmessage = (b) => onData(new Uint8Array(b));
    return invoke<{ tool: TermTool; attached: boolean; skillInstalled: boolean }>("term_spawn", {
      workflowId,
      tool,
      cols,
      rows,
      onData: ch,
    });
  },
  termWrite: (workflowId: string, data: string) => invoke<void>("term_write", { workflowId, data }),
  termResize: (workflowId: string, cols: number, rows: number) =>
    invoke<void>("term_resize", { workflowId, cols, rows }),
  termKill: (workflowId: string) => invoke<void>("term_kill", { workflowId }),
  onTermExit: (cb: (p: { workflowId: string }) => void): Promise<UnlistenFn> =>
    listen<{ workflowId: string }>("term-exit", (e) => cb(e.payload)),
};
