import { expect, test } from "vitest";
import type { WebExecPayload } from "../api";
import { applyWebEvent, emptyWebLive } from "./web";

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
