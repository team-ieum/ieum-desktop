import { expect, test } from "vitest";
import type { StoredWorkflow } from "../api";
import { mergeRemote } from "./sync";

const node = (id: string, position?: { x: number; y: number }) => ({ id, event: "local.file_trash", type: "ACTION", label: id, config: {}, ...(position ? { position } : {}) });
const wf = (nodes: unknown[], extra = {}) => ({ id: "w", name: "n", enabled: true, updatedAt: "v1", workflow: { nodes, edges: [] }, ...extra }) as StoredWorkflow;

test("merge_keeps_screen_positions_and_places_new_nodes", () => {
  const local = wf([node("a", { x: 10, y: 20 }), node("b", { x: 330, y: 20 }), node("gone", { x: 900, y: 0 })]);
  const remote = wf([node("a", { x: 0, y: 0 }), node("b"), node("n1"), node("n2")], { name: "새 이름", enabled: false, updatedAt: "v2" });
  const m = mergeRemote(local, remote);
  expect([m.name, m.enabled, m.updatedAt]).toEqual(["새 이름", false, "v2"]); // 내용은 저장본
  expect(m.workflow.nodes.map((n) => [n.id, n.position])).toEqual([
    ["a", { x: 10, y: 20 }], // 같은 id = 화면 자리
    ["b", { x: 330, y: 20 }],
    ["n1", { x: 650, y: 20 }], // 가장 오른쪽의 오른쪽(320 간격)
    ["n2", { x: 970, y: 20 }],
  ]);
});

test("merge_uses_stored_position_when_screen_has_none", () => {
  const m = mergeRemote(wf([]), wf([node("a", { x: 5, y: 5 }), node("b")]));
  expect(m.workflow.nodes.map((n) => n.position)).toEqual([{ x: 5, y: 5 }, { x: 325, y: 5 }]);
});
