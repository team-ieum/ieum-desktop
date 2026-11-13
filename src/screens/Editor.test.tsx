// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type WfNode, type WorkflowEntry } from "../api";
import { nodeTitle } from "../editor/Canvas";
import { AUTOSAVE_MS } from "../editor/sync";
import { setLang } from "../i18n";
import { PALETTE } from "../nodes/defs";
import { useWorkflows } from "../stores/workflows";
import { Editor } from "./Editor";

let changed: (p: { id: string; updatedAt: string }) => void;
let undoFailed: (p: { id: string; runId: string }) => void;

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
  vi.spyOn(api, "validateWorkflow").mockResolvedValue([]);
  vi.spyOn(api, "onWorkflowChanged").mockImplementation(async (cb) => ((changed = cb), () => {}));
  vi.spyOn(api, "setEditing").mockResolvedValue();
  vi.spyOn(api, "pendingFinish").mockResolvedValue(null);
  vi.spyOn(api, "onTestUndoFailed").mockImplementation(async (cb) => ((undoFailed = cb), () => {}));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

const open = () => act(async () => render(<Editor id={null} onBack={() => {}} onOpenRuns={() => {}} />));
const palette = () => screen.queryByRole("dialog", { name: "노드 추가" });
const addNode = async (event: string) => {
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  await act(async () => fireEvent.click(within(palette()!).getAllByRole("button", { name: new RegExp(`^${nodeTitle(event)}`) })[0]));
};

test("editor_palette_opens_from_plus_button", async () => {
  const r = await open();
  // 왼쪽 고정 팔레트 없음 — [+]를 눌러야 뜬다.
  expect(palette()).toBeNull();
  const plus = screen.getByRole("button", { name: "노드 추가" });
  await act(async () => fireEvent.click(plus));
  expect(plus.getAttribute("aria-expanded")).toBe("true");
  expect(palette()).toBeTruthy();
  // Esc로 닫힌다.
  await act(async () => fireEvent.keyDown(document, { key: "Escape" }));
  expect(palette()).toBeNull();
  // 바깥을 누르면 닫힌다.
  await act(async () => fireEvent.click(plus));
  await act(async () => fireEvent.mouseDown(document.body));
  expect(palette()).toBeNull();
  // 노드를 고르면 추가되고 닫힌다.
  await addNode(PALETTE[0]);
  expect(palette()).toBeNull();
  expect(r.container.querySelectorAll(".react-flow__node")).toHaveLength(1);
});

test("editor_config_panel_only_when_node_selected", async () => {
  const r = await open();
  expect(screen.queryByRole("complementary")).toBeNull();
  expect(screen.queryByLabelText("노드 이름")).toBeNull();
  await addNode(PALETTE[0]);
  // 새 노드가 골라져 설정 패널이 뜬다 — 머리에 이름·종류, 아래 테스트 칸.
  const panel = screen.getByRole("complementary", { name: nodeTitle(PALETTE[0]) });
  expect(within(panel).getByLabelText("노드 이름")).toBeTruthy();
  expect(within(panel).getByRole("heading", { name: "테스트" })).toBeTruthy();
  // [X]로 닫힌다.
  await act(async () => fireEvent.click(within(panel).getByRole("button", { name: "패널 닫기" })));
  expect(screen.queryByRole("complementary")).toBeNull();
  // 노드를 다시 누르면 열리고 Esc로 닫힌다.
  await act(async () => fireEvent.click(r.container.querySelector(".react-flow__node")!));
  expect(screen.getByRole("complementary")).toBeTruthy();
  await act(async () => fireEvent.keyDown(document, { key: "Escape" }));
  expect(screen.queryByRole("complementary")).toBeNull();
});

test("editor_panel_floats_over_canvas", async () => {
  const r = await open();
  const canvas = r.container.querySelector(".react-flow")!.parentElement!;
  await addNode(PALETTE[0]);
  const panel = screen.getByRole("complementary", { name: nodeTitle(PALETTE[0]) });
  // 캔버스 위에 떠 있는 카드 — 캔버스는 뒤에서 전체 폭 그대로.
  expect(panel.className).toMatch(/\babsolute\b/);
  expect(panel.className).toContain("rounded-2xl");
  expect(panel.className).toContain("shadow-");
  expect(r.container.querySelector(".react-flow")!.parentElement).toBe(canvas);
  expect(canvas.className).toContain("inset-0");
  expect(panel.parentElement!.contains(canvas)).toBe(true);
});

test("tech_info_toggle_shows_event_ids", async () => {
  await open();
  await addNode(PALETTE[0]);
  await addNode("local.ai");
  await act(async () =>
    fireEvent.change(within(screen.getByTestId("field-provider")).getByRole("combobox"), { target: { value: "api:anthropic" } }),
  );
  expect(screen.queryByText(PALETTE[0])).toBeNull();
  const sw = screen.getByRole("switch", { name: "기술 정보" });
  await act(async () => fireEvent.click(sw));
  expect(sw.getAttribute("aria-checked")).toBe("true");
  expect(screen.getByText(PALETTE[0])).toBeTruthy();
  expect(screen.getByText("local.ai")).toBeTruthy();
  // AI 노드는 공급원·모델 칸도.
  expect(screen.getByText("공급원")).toBeTruthy();
  expect(screen.getByText("api:anthropic")).toBeTruthy();
  await act(async () => fireEvent.click(sw));
  expect(screen.queryByText(PALETTE[0])).toBeNull();
});

test("editor_shows_assistant_button", async () => {
  await open();
  expect(screen.getByRole("button", { name: "IEUM Assistant 열기" })).toBeTruthy();
});

// ── 캔버스 동기화(B) ──
const N = (id: string, x: number): WfNode => ({ id, event: "local.file_trash", type: "ACTION", label: id, config: {}, position: { x, y: 0 } });
const entry = (nodes: WfNode[], updatedAt: string): WorkflowEntry => ({
  id: "w1",
  name: "정리",
  enabled: false,
  updatedAt,
  watch: "off",
  watchMessage: null,
  workflow: { nodes, edges: [] },
});
const openSaved = async (e: WorkflowEntry) => {
  useWorkflows.setState({ list: [e] });
  return act(async () => render(<Editor id="w1" onBack={() => {}} onOpenRuns={() => {}} />));
};
const openAssistant = () => act(async () => fireEvent.click(screen.getByRole("button", { name: "IEUM Assistant 열기" })));
const tick = (ms: number) => act(async () => vi.advanceTimersByTime(ms));
const rename = (v: string) => act(async () => fireEvent.change(screen.getByLabelText("워크플로우 이름"), { target: { value: v } }));

test("opening_assistant_on_new_workflow_saves_it_disabled_first", async () => {
  vi.useFakeTimers();
  const saveSpy = vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, id: "w9", updatedAt: "v1" }));
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  const r = await open();
  await openAssistant();
  expect(saveSpy).toHaveBeenCalledTimes(1);
  expect(saveSpy.mock.calls[0][0]).toMatchObject({ id: "", name: "새 워크플로우", enabled: false });
  expect(r.container.querySelector('[data-slot="chat"]')).toBeTruthy(); // id가 생긴 뒤에 슬롯
  await tick(AUTOSAVE_MS * 3);
  expect(saveSpy).toHaveBeenCalledTimes(1); // 자동 저장이 다시 켜서 저장하지 않는다
  expect(screen.getByRole("switch", { name: "활성" }).getAttribute("aria-checked")).toBe("false"); // 화면도 꺼짐
});

test("failed_first_save_leaves_no_slot_and_does_not_retry_by_itself", async () => {
  vi.useFakeTimers();
  const saveSpy = vi.spyOn(api, "saveWorkflow").mockRejectedValue("저장 실패");
  const r = await open();
  await openAssistant();
  expect(screen.getByText("저장 실패")).toBeTruthy();
  expect(r.container.querySelector("[data-slot]")).toBeNull();
  await tick(AUTOSAVE_MS * 3);
  expect(saveSpy).toHaveBeenCalledTimes(1);
});

test("canvas_edits_autosave_only_after_assistant_opened", async () => {
  vi.useFakeTimers();
  const saveSpy = vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, updatedAt: "v2" }));
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  await openSaved(entry([N("a", 0)], "v1"));
  await addNode(PALETTE[0]);
  await tick(AUTOSAVE_MS * 3);
  expect(saveSpy).not.toHaveBeenCalled(); // 패널을 열기 전엔 손으로 저장
  await openAssistant();
  await tick(AUTOSAVE_MS - 1);
  expect(saveSpy).not.toHaveBeenCalled();
  await tick(1);
  expect(saveSpy).toHaveBeenCalledTimes(1);
  expect(saveSpy.mock.calls[0][0]).toMatchObject({ id: "w1", updatedAt: "v1" }); // 기대값 = 읽은 updatedAt
  await tick(AUTOSAVE_MS * 3);
  expect(saveSpy).toHaveBeenCalledTimes(1); // 저장 뒤엔 조용
});

test("rejected_autosave_waits_for_the_next_edit", async () => {
  vi.useFakeTimers();
  const saveSpy = vi.spyOn(api, "saveWorkflow").mockRejectedValue("거부");
  await openSaved(entry([N("a", 0)], "v1"));
  await openAssistant();
  await rename("하나");
  await tick(AUTOSAVE_MS * 3);
  expect(saveSpy).toHaveBeenCalledTimes(1); // 같은 내용으로 되풀이하지 않는다
  await rename("둘");
  await tick(AUTOSAVE_MS);
  expect(saveSpy).toHaveBeenCalledTimes(2);
});

test("remote_change_replaces_canvas_and_clears_deleted_selection", async () => {
  vi.useFakeTimers();
  const saveSpy = vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, updatedAt: "v3" }));
  const r = await openSaved(entry([N("a", 0), N("b", 320)], "v1"));
  await act(async () => fireEvent.click(r.container.querySelectorAll(".react-flow__node")[1])); // b 선택
  expect(screen.getByRole("complementary", { name: "b" })).toBeTruthy();
  // 명령이 b를 지우고 자리 없는 c를 더했다.
  vi.spyOn(api, "listWorkflows").mockResolvedValue([entry([N("a", 0), { ...N("c", 0), position: undefined }], "v2")]);
  await act(async () => changed({ id: "other", updatedAt: "v9" })); // 남의 워크플로우는 무시
  expect(r.container.querySelectorAll(".react-flow__node")).toHaveLength(2);
  await act(async () => changed({ id: "w1", updatedAt: "v2" }));
  expect(screen.queryByRole("complementary", { name: "b" })).toBeNull(); // 삭제된 노드 선택 해제
  expect(r.container.querySelectorAll(".react-flow__node")).toHaveLength(2);
  expect(screen.queryByText("채팅이 워크플로우를 바꿔서 화면을 다시 불러왔어요.")).toBeNull(); // 잃은 편집이 없다
  // 새 노드에 잡아 준 자리는 자동 저장된다(패널을 안 열었어도 — 명령이 오고 있다).
  await tick(AUTOSAVE_MS);
  const sent = saveSpy.mock.calls[0][0];
  expect(sent.updatedAt).toBe("v2");
  expect(sent.workflow.nodes.map((n) => [n.id, n.position])).toEqual([
    ["a", { x: 0, y: 0 }],
    ["c", { x: 320, y: 0 }],
  ]);
});

test("unsaved_edit_marks_user_editing_and_is_kept", async () => {
  const editing = vi.spyOn(api, "setEditing").mockResolvedValue();
  vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, updatedAt: "v2" }));
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  const r = await openSaved(entry([N("a", 0)], "v1"));
  await rename("내가 고친 이름"); // 패널을 안 연 채 미저장
  expect(editing).toHaveBeenLastCalledWith("w1", true); // 이 동안 Builder의 고치는 동작은 USER_EDITING(A-1) → workflow-changed가 오지 않는다
  expect((screen.getByLabelText("워크플로우 이름") as HTMLInputElement).value).toBe("내가 고친 이름"); // 편집은 그대로 남는다
  expect(screen.queryByText("채팅이 워크플로우를 바꿔서 화면을 다시 불러왔어요.")).toBeNull();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "저장" })));
  expect(editing).toHaveBeenLastCalledWith("w1", false); // 저장하면 풀린다
  await rename("또 고침");
  expect(editing).toHaveBeenLastCalledWith("w1", true);
  r.unmount();
  expect(editing).toHaveBeenLastCalledWith("w1", false); // 편집기를 떠나도 풀린다
});

test("auto_placement_is_not_user_editing", async () => {
  const editing = vi.spyOn(api, "setEditing").mockResolvedValue();
  vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, updatedAt: "v3" }));
  const r = await openSaved(entry([N("a", 0)], "v1"));
  // 명령이 자리 없는 새 노드를 더했다 → 화면이 자리를 잡아 dirty가 되지만 사람이 고친 것이 아니다.
  vi.spyOn(api, "listWorkflows").mockResolvedValue([entry([N("a", 0), { ...N("c", 0), position: undefined }], "v2")]);
  await act(async () => changed({ id: "w1", updatedAt: "v2" }));
  expect(r.container.querySelectorAll(".react-flow__node")).toHaveLength(2);
  expect(editing).not.toHaveBeenCalled();
  await rename("내가 고침");
  expect(editing.mock.calls).toEqual([["w1", true]]);
});

test("stale_save_reloads_instead_of_overwriting", async () => {
  vi.spyOn(api, "saveWorkflow").mockRejectedValue("STALE");
  await openSaved(entry([N("a", 0)], "v1"));
  vi.spyOn(api, "listWorkflows").mockResolvedValue([{ ...entry([N("a", 0)], "v2"), name: "빌더가 고친 이름" }]);
  await rename("내가 고친 이름");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "저장" })));
  expect((screen.getByLabelText("워크플로우 이름") as HTMLInputElement).value).toBe("빌더가 고친 이름");
  expect(screen.getByText("채팅이 워크플로우를 바꿔서 화면을 다시 불러왔어요.")).toBeTruthy();
});

test("test_undo_failure_shows_paths_on_canvas", async () => {
  await openSaved(entry([N("a", 0)], "v1"));
  vi.spyOn(api, "getRun").mockResolvedValue({ undo: { restored: [], skipped: [[{ kind: "move", from: "/in/a.pdf", to: "/out/a.pdf" }, { reason: "originalOccupied" }]] } } as never);
  await act(async () => undoFailed({ id: "other", runId: "r0" })); // 남의 워크플로우는 무시
  expect(screen.queryByText("테스트가 옮긴 파일을 되돌리지 못했어요")).toBeNull();
  await act(async () => undoFailed({ id: "w1", runId: "r1" }));
  expect(screen.getByText("테스트가 옮긴 파일을 되돌리지 못했어요")).toBeTruthy();
  expect(screen.getByText("/in/a.pdf → /out/a.pdf")).toBeTruthy();
});
