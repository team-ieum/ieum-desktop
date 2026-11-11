// @vitest-environment jsdom
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Workflow } from "../api";
import { Canvas } from "./Canvas";

// ReactFlow는 진짜로 그리되, Canvas가 넘긴 핸들러를 꺼내 직접 부를 수 있게 한다.
const probe = vi.hoisted(() => ({ props: null as unknown as Record<string, any> }));
vi.mock("@xyflow/react", async (orig) => {
  const m = await orig<typeof import("@xyflow/react")>();
  return {
    ...m,
    ReactFlow: (p: Record<string, any>) => {
      probe.props = p;
      return <m.ReactFlow {...p} />;
    },
  };
});

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  // @ts-expect-error jsdom에 없음
  globalThis.DOMMatrixReadOnly = class {
    m22 = 1;
  };
});
afterEach(cleanup);

const wf: Workflow = {
  nodes: [
    { id: "t", event: "local.folder_new_file", type: "TRIGGER", position: { x: 0, y: 0 }, config: { folder: "/x" } },
    { id: "m", event: "local.file_trash", type: "ACTION", position: { x: 300, y: 0 }, config: {} },
  ],
  edges: [{ id: "e", source: "t", target: "m" }],
};

function setup(readOnly: boolean) {
  const onChange = vi.fn();
  const onSelect = vi.fn();
  const r = render(
    <div style={{ width: 800, height: 600 }}>
      <Canvas workflow={wf} readOnly={readOnly} selectedId="m" onChange={onChange} onSelect={onSelect} />
    </div>,
  );
  return { ...r, onChange, onSelect };
}

test("canvas_read_only_blocks_edits", async () => {
  // 대조군: 편집 모드에서는 선택된 노드를 지우면 콜백이 불린다.
  const edit = setup(false);
  await act(async () => {
    fireEvent.keyDown(document.body, { key: "Backspace", code: "Backspace" });
  });
  expect(edit.onChange).toHaveBeenCalled();
  cleanup();

  const ro = setup(true);
  const nodes = ro.container.querySelectorAll(".react-flow__node");
  expect(nodes.length).toBe(2);
  for (const n of nodes) {
    expect(n.classList.contains("draggable")).toBe(false);
    expect(n.classList.contains("selectable")).toBe(false);
  }
  for (const h of ro.container.querySelectorAll(".react-flow__handle")) {
    expect(h.classList.contains("connectable")).toBe(false);
  }
  await act(async () => {
    fireEvent.click(nodes[0]);
    fireEvent.keyDown(document.body, { key: "Backspace", code: "Backspace" });
    fireEvent.keyDown(document.body, { key: "Delete", code: "Delete" });
  });
  expect(ro.onChange).not.toHaveBeenCalled();
  expect(ro.onSelect).not.toHaveBeenCalled();
});

test("canvas_read_only_callbacks_guarded", async () => {
  const ro = setup(true);
  expect(probe.props.deleteKeyCode).toBeNull();
  await act(async () => {
    probe.props.onNodesChange([
      { type: "select", id: "t", selected: true },
      { type: "position", id: "t", position: { x: 9, y: 9 } },
      { type: "remove", id: "t" },
    ]);
    probe.props.onEdgesChange([{ type: "remove", id: "e" }]);
    probe.props.onConnect({ source: "m", target: "t", sourceHandle: null, targetHandle: null });
  });
  expect(ro.onChange).not.toHaveBeenCalled();
  expect(ro.onSelect).not.toHaveBeenCalled();
});

test("delete_node_and_unrelated_edge_together", async () => {
  // xyflow deleteElements는 한 틱에 onEdgesChange → onNodesChange 순으로 부른다.
  const three: Workflow = {
    nodes: [...wf.nodes, { id: "x", event: "local.file_trash", type: "ACTION", position: { x: 600, y: 0 }, config: {} }],
    edges: [
      { id: "tm", source: "t", target: "m" },
      { id: "mx", source: "m", target: "x" },
    ],
  };
  const onChange = vi.fn();
  render(
    <div style={{ width: 800, height: 600 }}>
      <Canvas workflow={three} onChange={onChange} />
    </div>,
  );
  await act(async () => {
    probe.props.onEdgesChange([{ type: "remove", id: "mx" }]);
    probe.props.onNodesChange([{ type: "remove", id: "t" }]);
  });
  const last: Workflow = onChange.mock.lastCall![0];
  expect(last.nodes.map((n) => n.id)).toEqual(["m", "x"]);
  expect(last.edges).toEqual([]);
});
