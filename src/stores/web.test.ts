import { expect, test, vi } from "vitest";
import { api, type WebExecPayload } from "../api";
import { applyWebEvent, emptyWebLive, useWeb, webStatuses } from "./web";

const EX = "ex-1";
const ev = (type: string, extra: Record<string, unknown> = {}): WebExecPayload => ({
  executionId: EX,
  event: { type, executionId: EX, ...extra },
});

test("web_exec_events_idempotent_and_filtered", () => {
  let live = emptyWebLive(EX);
  const run = (p: WebExecPayload) => (live = applyWebEvent(live, p));

  run(ev("NODE_STARTED", { nodeId: "a" }));
  run(ev("NODE_COMPLETED", { nodeId: "a", status: "SUCCESS" }));
  run(ev("NODE_STARTED", { nodeId: "b" }));
  expect(live.nodes).toEqual({ a: "succeeded", b: "running" });

  // 재연결 스냅샷: 끝난 노드만 다시 온다(실행 중 b의 NODE_STARTED 없음) — 화면 상태를 지우지 않는다.
  const before = live;
  run(ev("NODE_COMPLETED", { nodeId: "a", status: "SUCCESS" }));
  expect(live).toBe(before);
  expect(live.nodes.b).toBe("running");

  // 끝난 노드는 NODE_STARTED가 다시 와도 되돌아가지 않는다.
  run(ev("NODE_STARTED", { nodeId: "a" }));
  expect(live.nodes.a).toBe("succeeded");

  run(ev("NODE_FAILED", { nodeId: "b", status: "FAILED", errorMessage: "터짐" }));
  run(ev("NODE_COMPLETED", { nodeId: "c", status: "SKIPPED" }));
  expect(live.nodes).toEqual({ a: "succeeded", b: "failed", c: "skipped" });
  expect(live.errors.b).toBe("터짐");

  // 다른 실행(해제 뒤 늦게 온 것) 이벤트는 거른다.
  const other = live;
  run({ executionId: "ex-2", event: { type: "NODE_FAILED", nodeId: "a" } });
  run({ executionId: "ex-2", stopped: true });
  expect(live).toBe(other);

  run(ev("EXECUTION_COMPLETED", { executionStatus: "FAILED" }));
  expect(live.status).toBe("FAILED");

  // stopped·logs·error payload는 각각 남는다.
  run({ executionId: EX, stopped: true });
  expect(live.stopped).toBe(true);
  run({ executionId: EX, logs: [{ id: "l1", nodeId: "d", status: "SUCCESS" }] });
  expect(live.logs?.[0].nodeId).toBe("d");
  expect(live.nodes.d).toBe("succeeded");
  run({ executionId: EX, error: { code: "NETWORK", message: "끊김" } });
  expect(live.error).toEqual({ code: "NETWORK", message: "끊김" });
});

test("web_ended_execution_stops_running_nodes", () => {
  let live = emptyWebLive(EX);
  const run = (p: WebExecPayload) => (live = applyWebEvent(live, p));
  run(ev("NODE_STARTED", { nodeId: "a" }));
  run(ev("APPROVAL_REQUESTED", { nodeId: "gate" }));
  run(ev("NODE_STARTED", { nodeId: "lost" }));
  const logs = [
    { id: "1", nodeId: "a", status: "FAILED" },
    { id: "2", nodeId: "gate", status: "PENDING" },
  ];
  // 실행 중엔 생중계가 앞선다.
  expect(webStatuses(logs, live)).toEqual({ a: "running", gate: "running", lost: "running" });

  // 끝 이벤트 뒤: running은 로그의 끝난 값, 없으면(승인 대기·로그 없음) 표시 없음.
  run(ev("EXECUTION_COMPLETED", { executionStatus: "FAILED" }));
  expect(webStatuses(logs, live)).toEqual({ a: "failed" });

  // 폴백 logs도 끝으로 본다.
  live = emptyWebLive(EX);
  run(ev("NODE_STARTED", { nodeId: "a" }));
  run(ev("NODE_STARTED", { nodeId: "b" }));
  run({ executionId: EX, logs: [{ id: "1", nodeId: "a", status: "SUCCESS" }] });
  expect(webStatuses([], live)).toEqual({ a: "succeeded" });
});

test("expire_keeps_account_logout_clears", async () => {
  const acc = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };
  useWeb.setState({ account: acc, expired: false, offline: false });

  useWeb.getState().expireIf({ code: "AUTH_EXPIRED" });
  expect(useWeb.getState().account).toEqual(acc);
  expect(useWeb.getState().expired).toBe(true);

  // 네트워크·서버 오류는 만료가 아니라 오프라인.
  useWeb.setState({ expired: false });
  useWeb.getState().expireIf({ code: "NETWORK" });
  expect(useWeb.getState()).toMatchObject({ account: acc, expired: false, offline: true });
  await useWeb.getState().guard(Promise.resolve(1));
  expect(useWeb.getState().offline).toBe(false);

  vi.spyOn(api, "webLogout").mockResolvedValue();
  await useWeb.getState().logout();
  expect(useWeb.getState().account).toBeNull();
  expect(useWeb.getState().expired).toBe(false);
});
