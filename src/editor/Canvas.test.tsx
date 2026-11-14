// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Position, ReactFlowProvider } from "@xyflow/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { Workflow } from "../api";
import { setLang } from "../i18n";
import { useCredentials } from "../stores/credentials";
import { AnimatedEdge } from "./AnimatedEdge";
import { Canvas, FOLLOW_QUIET_MS, modelLabel, shouldFollow } from "./Canvas";

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
    // jsdom에선 엣지 라벨 층이 없다 — 보일 때 그 자리에 그린다.
    EdgeToolbar: ({ isVisible, children }: { isVisible?: boolean; children: React.ReactNode }) => (isVisible ? <div>{children}</div> : null),
  };
});

beforeAll(() => {
  setLang("ko");
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

test("canvas_fit_view_caps_zoom", () => {
  setup(false);
  expect(probe.props.fitView).toBe(true);
  expect(probe.props.fitViewOptions).toEqual({ padding: 0.24, minZoom: 0.78, maxZoom: 1 });
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

test("node_card_shows_step_numbers", () => {
  const three: Workflow = {
    nodes: [...wf.nodes, { id: "x", event: "local.ai", type: "AI", position: { x: 600, y: 0 }, config: {} }],
    edges: [],
  };
  const { container } = render(
    <div style={{ width: 800, height: 600 }}>
      <Canvas workflow={three} />
    </div>,
  );
  const steps = [...container.querySelectorAll("[data-step]")].map((e) => e.getAttribute("data-step"));
  expect(steps).toEqual(["1", "2", "3"]);
});

test("ai_node_shows_model_pill", () => {
  const ai = (config: Record<string, unknown>): Workflow => ({
    nodes: [{ id: "a", event: "local.ai", type: "AI", position: { x: 0, y: 0 }, config }],
    edges: [],
  });
  const r = render(<Canvas workflow={ai({ provider: "api:anthropic", model: "claude-sonnet-5-5" })} />);
  expect(screen.getByText("claude-sonnet-5-5")).toBeTruthy();
  r.rerender(<Canvas workflow={ai({ provider: "api:openai" })} />);
  expect(screen.getByText("모델 정보 없음")).toBeTruthy();
  r.rerender(<Canvas workflow={ai({ provider: "cli:claude", model: "x" })} />);
  expect(screen.getByText("Claude (구독)")).toBeTruthy();
});

test("model_label_names_subscription_providers", () => {
  expect(modelLabel({ provider: "cli:codex", model: "gpt-5.6-luna" })).toBe("Codex (구독)");
  expect(modelLabel({ provider: "cli:claude" })).toBe("Claude (구독)");
});

test("canvas_pill_shows_credential_label", () => {
  const sys = { id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: null };
  const work = { id: "abcd1234", label: "회사", isDefault: false, createdAt: "1", email: "w@x.dev" };
  useCredentials.setState({
    view: {
      apiKeys: { anthropic: [{ id: "default", label: "기본", isDefault: true, createdAt: "", last4: "1234" }], openai: [], gemini: [] },
      claudeAccounts: [sys, work],
      codexAccounts: [sys],
      keychainError: null,
    },
  });
  const ai = (config: Record<string, unknown>): Workflow => ({
    nodes: [{ id: "a", event: "local.ai", type: "AI", position: { x: 0, y: 0 }, config }],
    edges: [],
  });
  const r = render(<Canvas workflow={ai({ provider: "cli:claude", credential: "abcd1234" })} />);
  expect(screen.getByText("Claude (구독) · 회사")).toBeTruthy();
  // 기본이면 이름표 생략.
  r.rerender(<Canvas workflow={ai({ provider: "api:anthropic", model: "m", credential: "default" })} />);
  expect(screen.getByText("m")).toBeTruthy();
  r.rerender(<Canvas workflow={ai({ provider: "api:anthropic", model: "m", credential: "deadbeef" })} />);
  expect(screen.getByText("m · 찾을 수 없는 키")).toBeTruthy();
  useCredentials.setState({ view: null });
});

test("read_only_hides_edge_delete", () => {
  const edge = (readOnly: boolean) => (
    <ReactFlowProvider>
      <svg>
        <AnimatedEdge
          {...({} as Parameters<typeof AnimatedEdge>[0])}
          id="e"
          source="t"
          target="m"
          sourceX={0}
          sourceY={0}
          targetX={100}
          targetY={0}
          sourcePosition={Position.Right}
          targetPosition={Position.Left}
          selected
          data={{ readOnly }}
        />
      </svg>
    </ReactFlowProvider>
  );
  const r = render(edge(false));
  expect(screen.getByRole("button", { name: "연결 삭제" })).toBeTruthy();
  r.rerender(edge(true));
  expect(screen.queryByRole("button", { name: "연결 삭제" })).toBeNull();
});

test("패널이 열려 있고 노드가 늘었을 때만 따라간다 — 사용자가 방금 움직였으면 뺏지 않는다", () => {
  const now = 100_000;
  expect(shouldFollow(2, 3, 396, 0, now)).toBe(true);
  expect(shouldFollow(3, 3, 396, 0, now)).toBe(false); // 늘지 않음
  expect(shouldFollow(3, 2, 396, 0, now)).toBe(false); // 줄어듦
  expect(shouldFollow(2, 3, 0, 0, now)).toBe(false); // 패널 닫힘
  expect(shouldFollow(2, 3, 396, now - FOLLOW_QUIET_MS + 1, now)).toBe(false); // 방금 움직임
  expect(shouldFollow(2, 3, 396, now - FOLLOW_QUIET_MS, now)).toBe(true);
});
