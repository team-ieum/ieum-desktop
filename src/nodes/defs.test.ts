import { expect, test } from "vitest";
import type { WfNode } from "../api";
import { DEFS, newBranchKey, newNode, nextPosition, outputKeys } from "./defs";

test("defs_has_36_and_transform_map_outputs_follow_mappings", () => {
  expect(Object.keys(DEFS).length).toBe(36);
  const map = (config: Record<string, unknown>): WfNode => ({ id: "t", event: "transform.map", type: "TRANSFORM", config });
  expect(outputKeys(map({ mappings: { a: "1", b: "2" } }))).toEqual(["a", "b"]);
  expect(outputKeys(map({}))).toEqual([]);
  expect(outputKeys(map({ mappings: "x" }))).toEqual([]);
  expect(outputKeys(map({ mappings: ["a"] }))).toEqual([]);
  expect(newNode("http.request", { x: 0, y: 0 }, "h").type).toBe("HTTP");
  expect(newNode("formatter.text_trim", { x: 0, y: 0 }, "t").type).toBe("TRANSFORM");
});

test("new_branch_key_format", () => {
  for (let i = 0; i < 20; i++) expect(newBranchKey()).toMatch(/^b_[0-9a-f]{8}$/);
});

test("add_places_new_node_right_of_rightmost", () => {
  const n = (id: string, position?: { x: number; y: number }): WfNode => ({ id, event: "local.file_trash", position, config: {} });
  expect(nextPosition([])).toEqual({ x: 0, y: 0 });
  expect(nextPosition([n("a", { x: 0, y: 40 }), n("b", { x: 320, y: 40 })])).toEqual({ x: 640, y: 40 });
  // 끌어 옮긴 노드가 가장 오른쪽이면 그 노드 기준.
  expect(nextPosition([n("a", { x: 900, y: 200 }), n("b", { x: 320, y: 0 })])).toEqual({ x: 1220, y: 200 });
  // 위치 없는 노드는 Canvas 기본값(i*320, 0)으로 본다.
  expect(nextPosition([n("a"), n("b")])).toEqual({ x: 640, y: 0 });
});
