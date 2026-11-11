import { create } from "zustand";
import { api, type WebAccount, type WebError, type WebExecPayload, type WebNodeLog } from "../api";
import { tDyn } from "../i18n";
import type { NodeRunStatus } from "./runs";

/** BE 노드 상태(`SUCCESS`·`FAILED`·`SKIPPED`·`RUNNING`·`PENDING`) → 화면 상태. */
export const webNodeStatus = (s?: string): NodeRunStatus =>
  s === "SUCCESS" ? "succeeded" : s === "FAILED" ? "failed" : s === "SKIPPED" ? "skipped" : "running";

export const statusesFromLogs = (logs: WebNodeLog[]): Record<string, NodeRunStatus> =>
  Object.fromEntries(logs.map((l) => [l.nodeId, webNodeStatus(l.status)]));

/** 웹 실행 하나의 생중계 상태. */
export interface WebLive {
  executionId: string;
  /** `EXECUTION_COMPLETED`의 `executionStatus` */
  status: string | null;
  nodes: Record<string, NodeRunStatus>;
  errors: Record<string, string>;
  /** 폴백 최종 상태 */
  logs: WebNodeLog[] | null;
  /** 구독 상한으로 밀려남 */
  stopped: boolean;
  error: WebError | null;
}

export const emptyWebLive = (executionId: string): WebLive => ({
  executionId,
  status: null,
  nodes: {},
  errors: {},
  logs: null,
  stopped: false,
  error: null,
});

const TERMINAL: NodeRunStatus[] = ["succeeded", "failed", "skipped"];

/**
 * `nodeId+type`으로 멱등 — 끝난 노드는 다시 바뀌지 않는다. 재연결 스냅샷(실행 중 노드의 NODE_STARTED 없음)이 와도
 * 지우지 않고 덧칠만 한다. 다른 실행(해제 뒤 늦게 온 것) 이벤트는 거른다.
 */
export function applyWebEvent(live: WebLive, p: WebExecPayload): WebLive {
  if (p.executionId !== live.executionId) return live;
  if ("stopped" in p) return { ...live, stopped: true };
  if ("logs" in p) return { ...live, logs: p.logs, nodes: { ...live.nodes, ...statusesFromLogs(p.logs) } };
  if ("error" in p) return { ...live, error: p.error };
  const e = p.event;
  if (e.type === "EXECUTION_COMPLETED") {
    const status = e.executionStatus ?? null;
    return status === live.status ? live : { ...live, status };
  }
  if (!e.nodeId) return live;
  const id = e.nodeId;
  const cur = live.nodes[id];
  if (cur && TERMINAL.includes(cur)) return live;
  switch (e.type) {
    case "NODE_STARTED":
    case "APPROVAL_REQUESTED":
      return cur ? live : { ...live, nodes: { ...live.nodes, [id]: "running" } };
    case "NODE_COMPLETED":
      // 건너뜀도 type은 NODE_COMPLETED, status만 SKIPPED(BE 계약).
      return { ...live, nodes: { ...live.nodes, [id]: e.status === "SKIPPED" ? "skipped" : "succeeded" } };
    case "NODE_FAILED":
      return {
        ...live,
        nodes: { ...live.nodes, [id]: "failed" },
        errors: e.errorMessage ? { ...live.errors, [id]: e.errorMessage } : live.errors,
      };
    default:
      return live;
  }
}

const isWebError = (e: unknown): e is WebError => typeof e === "object" && e !== null && "code" in e;

/** `web_*` 오류(`{code, message?}`)는 사전 문구로, 그 밖(서버 주소 검증 등)은 Rust 문구 그대로. */
export function webErrorText(e: unknown): string {
  if (!isWebError(e)) return String(e);
  const base = tDyn(`web.err.${e.code}`) ?? e.code;
  return e.message ? `${base}: ${e.message}` : base;
}

interface WebState {
  account: WebAccount | null;
  /** 로그인 만료로 나왔다 — 설정의 계정 칸에 알린다. */
  expired: boolean;
  load: () => Promise<void>;
  login: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  /** `web_*` 호출을 감싼다 — `AUTH_EXPIRED`면 로그아웃 상태로(App이 로그인 화면으로 보낸다). */
  guard: <T>(p: Promise<T>) => Promise<T>;
  /** 오류가 `AUTH_EXPIRED`면 로그아웃 상태로. */
  expireIf: (e: unknown) => void;
}

export const useWeb = create<WebState>((set, get) => ({
  account: null,
  expired: false,
  load: async () => {
    try {
      set({ account: await api.webAccount() });
    } catch (e) {
      console.error("[ieum] 이음 계정을 읽지 못했어요", e);
      set({ account: null });
    }
  },
  login: async (email, password) => {
    set({ account: await api.webLogin(email, password), expired: false });
  },
  logout: async () => {
    try {
      await api.webLogout();
    } finally {
      await get().load();
    }
  },
  guard: (p) =>
    p.catch((e: unknown) => {
      get().expireIf(e);
      throw e;
    }),
  expireIf: (e) => {
    if (isWebError(e) && e.code === "AUTH_EXPIRED") set({ account: null, expired: true });
  },
}));
