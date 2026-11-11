import { expect, test } from "vitest";
import type { WfNode } from "../api";
import { newBranchKey, nextPosition } from "./defs";

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
