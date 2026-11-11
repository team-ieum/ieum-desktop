// @vitest-environment jsdom
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Workflow } from "../api";
import { Canvas } from "./Canvas";

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
