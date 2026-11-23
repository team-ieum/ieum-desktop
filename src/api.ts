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
  /** 다음 스케줄 차례(Asia/Seoul "YYYY-MM-DDTHH:MM:SS"). 꺼짐·스케줄 아님이면 null. */
  nextRun?: string | null;
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
  /** 응답이 길어 기록에는 앞부분만 남겼다. 옛 기록에는 없다. */
  outputClipped?: boolean;
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
  | { kind: "nodeFailed"; nodeId: string; message: string; code?: string }
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
  /** 스케줄 실행의 차례(Asia/Seoul "YYYY-MM-DDTHH:MM:SS"). */
  scheduledFor?: string | null;
}
/** 폴더 감시·수동·스케줄·GitHub. 옛 기록은 없음. */
export type RunTrigger = "watch" | "manual" | "test" | "schedule" | "github";
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
  /** 한도 도달 시 다른 계정 권하기 — 옛 설정·테스트엔 없을 수 있어 `!== false`로 읽는다. */
  notifyAccountSuggest?: boolean;
}
/** 기본 계정이 한도에 닿아 권하는 다른 계정(Rust `limit_watch::Suggestion`). */
export interface Suggestion {
  company: "claude" | "codex";
  fromId: string;
  fromLabel: string;
  toId: string;
  toLabel: string;
  toUsedPct: number;
  windowKind: string;
  resetsAt: string;
}
/** 설정 [업데이트 확인] 결과(Rust `UpdateCheck`). */
export type UpdateCheck = { status: "dev" } | { status: "latest" } | { status: "available"; version: string };
/** Rust `update-available` 이벤트 — `manual`이면 설정에서 직접 확인한 것(숨긴 버전도 다시 띄운다). */
export type UpdateAvailable = { version: string; manual: boolean };
/** Rust `update-progress` — 받은 바이트·전체(모르면 null), 다 받아 설치 중이면 `installing`. */
export type UpdateProgress = { downloaded: number; total: number | null; installing: boolean };
export type SystemSettingsPane = "files" | "notifications" | "login_items";
/** 시스템 알림 클릭으로 열 화면(Rust `notify::Target`). */
export type OpenTarget =
  | { kind: "run"; runId: string }
  | { kind: "history" }
  | { kind: "suggest" }
  | { kind: "workflow"; id: string }
  | { kind: "aiSettings" };
/** 키별 월 한도와 이번 달 지출(Rust `KeyLimitsView`) — 키는 `"<source>:<id>"`. spend에는 이번 달 쓴 키만 있다. */
export interface KeyLimitsView {
  limits: Record<string, number>;
  spend: Record<string, number>;
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

// usage.rs `report` — 설정 › 통계 및 사용량. 전부 최근 30일, 추적 시작만 전체 기간.
export type UsageProvider = "claude" | "openai" | "google";
/** 오늘 포함 30일, 빈 날 0. usd는 구독 호출이 빠진 예상 금액. */
export interface DayUse { date: string; tokens: number; usd: number }
export interface ModelUse { provider: UsageProvider; model: string; tokens: number; calls: number; usd: number }
export interface WorkflowUse { workflowId: string; name: string | null; tokens: number; calls: number; runs: number; usd: number }
export interface ProviderReport {
  provider: UsageProvider;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  usd: number;
  unpricedCalls: number;
  calls: number;
  runs: number;
  topModel: string | null;
  byDay: DayUse[];
  byModel: ModelUse[];
  byWorkflow: WorkflowUse[];
  recent: { at: string; workflowId: string; workflowName: string | null; model: string | null; credential: string; input: number; output: number }[];
}
export interface UsageReport {
  updatedAt: string;
  trackingSince: string | null;
  overview: { totalTokens: number; usd: number; unpricedCalls: number; calls: number; byDay: DayUse[]; byModel: ModelUse[]; byWorkflow: WorkflowUse[] };
  /** 늘 claude·openai·google 순. */
  providers: ProviderReport[];
}

// plan_usage.rs(Claude)·sub_usage.rs(Codex·Antigravity) — 구독 연결 상태와 창별 사용 비율.
export interface PlanWindow {
  /** Claude: five_hour·seven_day·seven_day_opus·seven_day_model(모델별 주간). Codex·Antigravity: session(하루 이하)·weekly(7일 이하)·monthly(Codex 무료 30일). */
  kind: "five_hour" | "seven_day" | "seven_day_opus" | "seven_day_model" | "session" | "weekly" | "monthly";
  usedPct: number;
  resetsAt: string;
  /** 화면에 그대로 쓰는 이름(Claude 모델·Antigravity 그룹별 창). 없으면 kind 번역. */
  label?: string;
}
export interface PlanUsage {
  state: "ok" | "unavailable" | "not_logged_in" | "expired";
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
  /** 폴더 트리거일 때만. */
  folder: string | null;
  /** 끝내는 버튼 — `save`(수동 실행 워크플로우)면 [저장]. 옛 확인 대기에는 없다(= enable). */
  button?: "enable" | "save";
  /** `http`는 HTTP 줄만 — 이 칸이 생기기 전에 저장된 확인 대기에는 없다. */
  actions: { nodeId: string; event: string; label: string; target: string | null; http?: HttpLine }[];
  /** AI 노드마다 한 줄. 이 칸이 생기기 전에 저장된 확인 대기에는 없다. */
  ai?: { nodeId: string; label: string; provider: string; sendContent: boolean }[];
}

/** HTTP 테스트 확인 카드의 한 줄 — `url`은 참조식 그대로(치환한 값은 오지 않는다). `method`는 대문자. */
export interface HttpLine {
  nodeId: string;
  label: string;
  method: string;
  url: string;
  /** 머리말 이름(이름순). 값은 오지 않는다. */
  headerKeys: string[];
  /** url·headers·body의 참조식이 가리키는 앞 노드 출력 — 값은 오지 않는다. */
  sends: { nodeId: string; nodeLabel: string; output: string }[];
  hasBody: boolean;
  /** GitHub 쓰기 노드 줄 — 있으면 method·url 대신 노드 제목·저장소·요약을 보인다. */
  github?: { event: string; repo: string; summary: string };
}
export interface HttpTestConfirm {
  id: string;
  lines: HttpLine[];
}

/** 앱 안 터미널에 띄울 수 있는 CLI. */
export type TermTool = "claude" | "codex" | "agy";
/** CLI 카드의 상태 — `ready`·`unknown`(확인 불가)은 고를 수 있다. */
export type TermCliState = "ready" | "unknown" | "notInstalled" | "loggedOut";
export interface TermTools {
  tools: TermTool[];
  /** 세 CLI 모두, 카드 순서대로. */
  clis: { tool: TermTool; state: TermCliState }[];
  running: TermTool | null;
}
/** 모델 드롭다운 한 줄. */
export interface ModelChoice {
  id: string;
  label: string;
  /** 고를 수 있는 추론 수준, 낮은 것부터. 비면 못 고른다. */
  efforts: string[];
}
/** 모델 목록 + "기본 모델"일 때 고를 수 있는 수준. */
export interface ModelList {
  models: ModelChoice[];
  defaultEfforts: string[];
}

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
  | { kind: "test"; nodeId: string; runId: string; status: string; httpSent?: boolean }
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
  effort: string | null;
}
export type ChatStage = { id: string; stage: "designing" | "testing" };
/** GitHub 칸 목록 한 항목(소유자·저장소·브랜치·마일스톤). */
export interface OptionItem {
  id: string;
  name: string;
  /** "내 계정"·"조직"·"공개"·"비공개" */
  hint: string | null;
  updatedAt: string | null;
}
export interface OptionList {
  items: OptionItem[];
  /** 300개를 다 채워 더 있을 수 있다. */
  truncated: boolean;
}
/** 목록 조회 실패 — 그대로 던져진다. */
export interface OptionError {
  code?: string | null;
  message: string;
}

/** 설정 > 연동의 GitHub 줄 상태(Rust `GithubStatus`). */
export type GithubStatus =
  | { kind: "notInstalled" }
  | { kind: "outdated"; version: string }
  /** relogin = [연결]이 gh 로그인을 새로 돌린다(활성 계정 토큰이 죽음) — 누르기 전에 경고. */
  | { kind: "notConnected"; relogin: boolean }
  | { kind: "needsLogin"; recorded: string }
  | { kind: "accountChanged"; recorded: string; current: string }
  | { kind: "connected"; login: string; envToken: boolean }
  | { kind: "unreachable"; error: string };
export type GithubJob = { kind: "login" | "install"; code: string | null };

/** 노드별 마지막 테스트 결과 — 러스트 `store::Sample`. */
export interface Sample {
  status: "succeeded" | "failed";
  output: Obj | null;
  error: string | null;
  errorCode: string | null;
  /** RFC3339. */
  testedAt: string;
  /** 테스트한 때의 `{event, config}`. */
  node: { event: string; config: Obj };
}
/** 노드 패널 테스트 결과 — 러스트 `supervisor::NodeTestResult`. */
export interface NodeTestResult {
  status: "succeeded" | "failed" | "invalid" | "missing";
  output: Obj | null;
  error: string | null;
  errorCode: string | null;
  testedAt: string | null;
  /** `missing`일 때 성공 샘플이 없는 앞 노드 id. */
  missing: string[];
  undone: boolean;
  undoFailed: number;
  runId: string | null;
}

export const api = {
  listWorkflows: () => invoke<WorkflowEntry[]>("list_workflows"),
  saveWorkflow: (wf: StoredWorkflow) => invoke<StoredWorkflow>("save_workflow", { wf }),
  deleteWorkflow: (id: string) => invoke<void>("delete_workflow", { id }),
  validateWorkflow: (wf: Workflow) => invoke<ValidationError[]>("validate_workflow", { wf }),
  /** 실행이 끝나면 run id를 돌려준다. */
  /** 폴더 트리거는 `filePath` 필수, 스케줄·수동은 없이. */
  runNow: (id: string, filePath?: string) => invoke<string>("run_now", { id, filePath: filePath ?? null }),
  /** 편집본의 노드 하나만 돌린다. 앞 노드 값은 앞 노드의 샘플. 폴더 트리거 이후 노드는 `filePath` 선택. */
  testNode: (id: string, workflow: Workflow, nodeId: string, filePath?: string) =>
    invoke<NodeTestResult>("test_node", { id, workflow, nodeId, filePath: filePath ?? null }),
  getSamples: (id: string) => invoke<Record<string, Sample>>("get_samples", { id }),
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
  keyLimits: () => invoke<KeyLimitsView>("key_limits"),
  /** `usd`가 null이면 한도를 푼다. */
  setKeyLimit: (source: ApiProvider, id: string, usd: number | null) => invoke<void>("set_key_limit", { source, id, usd }),
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
  githubStatus: () => invoke<GithubStatus>("github_status"),
  /** 이미 로그인돼 있으면 바로 기록, 아니면 device 로그인(코드는 github-login-code 이벤트)이 끝날 때까지 기다린다. */
  githubConnect: () => invoke<GithubStatus>("github_connect"),
  /** IEUM 기록만 지운다 — gh 로그인은 그대로. */
  githubDisconnect: () => invoke<GithubStatus>("github_disconnect"),
  githubInstall: () => invoke<GithubStatus>("github_install"),
  githubCancel: () => invoke<void>("github_cancel"),
  githubOpenDevice: () => invoke<void>("github_open_device"),
  /** source 예: "github.repos". 실패는 {code?, message}를 그대로 던진다. */
  githubOptions: (source: string, inputs: Record<string, string>) =>
    invoke<OptionList>("github_options", { source, inputs }),
  /** 진행 중인 로그인·설치(없으면 null) — 다시 들어온 화면이 [취소]·코드 카드를 보이게. */
  githubJob: () => invoke<GithubJob | null>("github_job"),
  onGithubLoginCode: (cb: (p: { code: string }) => void): Promise<UnlistenFn> =>
    listen<{ code: string }>("github-login-code", (e) => cb(e.payload)),
  /** 로그인·설치 공용 출력 줄. */
  onGithubProgress: (cb: (p: { line: string }) => void): Promise<UnlistenFn> =>
    listen<{ line: string }>("github-progress", (e) => cb(e.payload)),
  /** 앞 여부·알림 토글과 무관하게 "알림이 켜졌어요". */
  sendTestNotification: () => invoke<void>("send_test_notification"),
  checkUpdateNow: () => invoke<UpdateCheck>("check_update_now"),
  /** 이미 찾아 둔 새 버전(없으면 null) — 이벤트를 놓친 화면용. */
  pendingUpdate: () => invoke<string | null>("pending_update"),
  /** 받아 설치하고 앱을 다시 시작한다(성공하면 돌아오지 않는다). */
  installUpdateAndRestart: () => invoke<void>("install_update_and_restart"),
  onUpdateAvailable: (cb: (p: UpdateAvailable) => void): Promise<UnlistenFn> =>
    listen<UpdateAvailable>("update-available", (e) => cb(e.payload)),
  onUpdateProgress: (cb: (p: UpdateProgress) => void): Promise<UnlistenFn> =>
    listen<UpdateProgress>("update-progress", (e) => cb(e.payload)),
  /** 시스템 알림을 누르면 열 화면. */
  onOpenTarget: (cb: (t: OpenTarget) => void): Promise<UnlistenFn> =>
    listen<OpenTarget>("open-target", (e) => cb(e.payload)),
  /** 백그라운드 감시 한 회차의 구독 사용량(`claude:<id>`·`codex:<id>`·`antigravity` → 사용량). */
  onPlansUpdated: (cb: (p: Record<string, PlanUsage>) => void): Promise<UnlistenFn> =>
    listen<Record<string, PlanUsage>>("plans-updated", (e) => cb(e.payload)),
  /** 앱이 앞에 있을 때 권하기(띠). */
  onAccountSuggest: (cb: (s: Suggestion) => void): Promise<UnlistenFn> =>
    listen<Suggestion>("account-suggest", (e) => cb(e.payload)),
  /** 대기 중인 제안(회사별 하나)을 전부 꺼낸다(꺼내면 비워짐). 다른 이음 계정 때 쌓인 것은 Rust가 버린다. */
  takeAccountSuggestion: () => invoke<Suggestion[]>("take_account_suggestion"),
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
  /** `fresh` = [새로 고침] — 60초 캐시를 건너뛴다(429 대기 중·성공 직후 10초는 그대로). */
  subscriptionUsage: (kind: SubKind, id: string, fresh = false) => invoke<PlanUsage>("subscription_usage", { kind, id, fresh }),
  /** 계정 폴더를 만들고 로그인 명령을 준다(확인 전엔 목록에 안 보임). */
  beginCliAccount: (kind: SubKind, label: string) => invoke<CliAccountBegin>("begin_cli_account", { kind, label }),
  /** 그 계정 폴더로 로그인을 확인 — 이메일이 보이면 목록에 추가, 아니면 `not_logged_in` 그대로. */
  confirmCliAccount: (kind: SubKind, id: string) => invoke<PlanUsage>("confirm_cli_account", { kind, id }),
  cancelCliAccount: (kind: SubKind, id: string) => invoke<void>("cancel_cli_account", { kind, id }),
  /** 이 Mac의 CLI 로그인(`system`)을 IEUM에서 숨기기·되살리기 — CLI 로그아웃은 하지 않는다. */
  setSystemHidden: (kind: SubKind, hidden: boolean) => invoke<void>("set_system_hidden", { kind, hidden }),
  antigravityPlanUsage: (fresh = false) => invoke<PlanUsage>("antigravity_plan_usage", { fresh }),
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
  /** 빌더 테스트가 HTTP 요청을 보내기 전 사람 확인을 기다린다. */
  onHttpTestConfirm: (cb: (p: HttpTestConfirm) => void): Promise<UnlistenFn> =>
    listen<HttpTestConfirm>("http-test-confirm", (e) => cb(e.payload)),
  /** [보내기]/[테스트 없이 넘어가기]. 기다리는 대기가 없으면(시간 초과·덮어씀) false. */
  answerHttpConfirm: (workflowId: string, send: boolean) => invoke<boolean>("answer_http_confirm", { workflowId, send }),
  // ── 채팅 패널 ──
  chatSend: (workflowId: string, text: string, ai: ChatAi) =>
    invoke<ChatTurn>("chat_send", {
      workflowId,
      text,
      provider: ai.provider,
      credential: ai.credential,
      model: ai.model,
      effort: ai.effort,
    }),
  /** 폴더 고르기 카드의 버튼 — 시스템 폴더 창을 연다. */
  chatPickFolder: (workflowId: string, nodeId: string, field: string) =>
    invoke<BuilderReply>("chat_pick_folder", { workflowId, nodeId, field }),
  onChatStage: (cb: (p: ChatStage) => void): Promise<UnlistenFn> => listen<ChatStage>("chat-stage", (e) => cb(e.payload)),
  onTestUndoFailed: (cb: (p: { id: string; runId: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; runId: string }>("test-undo-failed", (e) => cb(e.payload)),
  // ── 캔버스 채팅 C(터미널) — 화면은 도구 이름만 넘긴다. 실행 줄은 Rust가 만든다. ──
  /** 쓸 수 있는 CLI(설치 + 로그인)와 이 워크플로우에 살아 있는 세션. */
  termTools: (workflowId: string) => invoke<TermTools>("term_tools", { workflowId }),
  /** 공급원의 모델 목록(가격표·codex 내장 목록·agy models). 못 읽으면 reject. */
  listModels: (provider: string) => invoke<ModelList>("list_models", { provider }),
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
