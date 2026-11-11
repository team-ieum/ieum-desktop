import { expect, test, vi } from "vitest";
import { api, type ExecEvent } from "../api";
import { applyExecEvent, emptyLive, subscribeExecEvents, type Live } from "./runs";

test("exec_events_subscribed_once", () => {
  const on = vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  subscribeExecEvents();
  subscribeExecEvents();
  expect(on).toHaveBeenCalledTimes(1);
});

const play = (live: Live, events: [string, ExecEvent][]) =>
  events.reduce((s, [workflowId, event]) => applyExecEvent(s, { workflowId, event }), live);

test("run_view_events_idempotent", () => {
  const W = "wf-1";
  const live = play(emptyLive(W), [
    [W, { kind: "runStarted", runId: "r1" }],
    [W, { kind: "nodeStarted", nodeId: "a" }],
    [W, { kind: "nodeChunk", nodeId: "a", text: "안녕" }],
    [W, { kind: "runStarted", runId: "r1" }], // 같은 이벤트 두 번 — 초기화하면 안 됨
    [W, { kind: "nodeSucceeded", nodeId: "a", output: { x: 1 } }],
    [W, { kind: "nodeSucceeded", nodeId: "a", output: { x: 1 } }],
    [W, { kind: "nodeStarted", nodeId: "a" }], // 늦게 온 시작 — 되돌아가면 안 됨
    [W, { kind: "nodeChunk", nodeId: "a", text: "늦음" }],
    ["wf-other", { kind: "nodeFailed", nodeId: "b", message: "x" }], // 다른 워크플로우
    ["wf-other", { kind: "runStarted", runId: "r9" }],
    [W, { kind: "runFinished", runId: "r0", status: "failed" }], // 다른 실행
    [W, { kind: "nodeFailed", nodeId: "b", message: "boom" }],
    [W, { kind: "runFinished", runId: "r1", status: "failed" }],
    [W, { kind: "runFinished", runId: "r1", status: "failed" }],
    [W, { kind: "nodeSkipped", nodeId: "c" }], // 끝난 뒤 — 무시
  ]);
  expect(live.runId).toBe("r1");
  expect(live.status).toBe("failed");
  expect(live.nodes).toEqual({ a: "succeeded", b: "failed" });
  expect(live.chunks).toEqual({ a: "안녕" });
  expect(live.errors).toEqual({ b: "boom" });

  const next = applyExecEvent(live, { workflowId: W, event: { kind: "runStarted", runId: "r2" } });
  expect(next).toMatchObject({ runId: "r2", status: "running", nodes: {}, chunks: {} });
});
