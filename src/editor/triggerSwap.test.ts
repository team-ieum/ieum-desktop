import { expect, test } from "vitest";
import type { WfNode, Workflow } from "../api";
import { planAdd, prependTrigger, referencingNodes, replaceTrigger } from "./triggerSwap";

const T = (id: string, event: string, x = 0): WfNode => ({ id, event, type: "TRIGGER", label: id, config: {}, position: { x, y: 0 } });
const A = (id: string, x = 0, config: Record<string, unknown> = {}): WfNode => ({ id, event: "local.file_trash", type: "ACTION", label: id, config, position: { x, y: 0 } });
const wf = (...nodes: WfNode[]): Workflow => ({ nodes, edges: [] });

test("plan_add_cases", () => {
  expect(planAdd(wf(A("a")), "manual.run")).toEqual({ kind: "prepend" });
  const t = T("t", "schedule.daily");
  expect(planAdd(wf(t, A("a")), "schedule.daily")).toEqual({ kind: "select", id: "t" });
  expect(planAdd(wf(t, A("a")), "manual.run")).toEqual({ kind: "ask", old: t });
  expect(planAdd(wf(t, T("u", "manual.run")), "github.new_issue")).toEqual({ kind: "append" });
  expect(planAdd(wf(t), "local.file_trash")).toEqual({ kind: "append" });
});

test("replace_keeps_id_position_edges_and_order", () => {
  const t = { ...T("t", "schedule.daily", 40), description: "옛 설명", config: { hour: "9" } };
  const edges = [
    { id: "e1", source: "t", target: "a" },
    { id: "e2", source: "a", target: "b" },
  ];
  const before: Workflow = { nodes: [t, A("a", 320), A("b", 640)], edges };
  const fresh: WfNode = { id: "new", event: "manual.run", type: "TRIGGER", label: "수동 실행", config: { x: 1 }, position: { x: 999, y: 999 } };
  const after = replaceTrigger(before, "t", fresh);
  expect(after.nodes.map((n) => n.id)).toEqual(["t", "a", "b"]);
  expect(after.edges).toEqual(edges);
  expect(after.nodes[0]).toEqual({ id: "t", event: "manual.run", type: "TRIGGER", label: "수동 실행", config: { x: 1 }, position: { x: 40, y: 0 } });
  expect(after.nodes[0]).not.toHaveProperty("description");
  expect(after.nodes[1]).toBe(before.nodes[1]);
});

test("replace_keeps_missing_position", () => {
  const { position: _p, ...t } = T("t", "schedule.daily");
  const after = replaceTrigger(wf(t), "t", T("new", "manual.run", 500));
  expect(after.nodes[0]).not.toHaveProperty("position");
  expect(after.nodes[0].id).toBe("t");
});

test("prepend_goes_left_of_leftmost", () => {
  const after = prependTrigger(wf(A("a", 0), A("b", 320)), T("n", "manual.run", 777));
  expect(after.nodes[0].id).toBe("n");
  expect(after.nodes[0].position).toEqual({ x: -320, y: 0 });
  expect(after.nodes.map((n) => n.id)).toEqual(["n", "a", "b"]);
  expect(prependTrigger(wf(), T("n", "manual.run", 777)).nodes[0].position).toEqual({ x: 0, y: 0 });
});

test("referencing_nodes_finds_refs", () => {
  const t = { ...T("t", "schedule.daily"), config: { note: "{{nodes.t.output.self}}" } };
  const uses = A("a", 0, { path: "{{nodes.t.output.path}}" });
  const other = A("b", 0, { path: "{{nodes.x.output.path}}" });
  const plain = A("c");
  expect(referencingNodes(wf(t, uses, other, plain), "t")).toEqual([uses]);
});
