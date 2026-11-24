// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type WfNode, type WorkflowEntry } from "../api";
import { nodeTitle } from "../editor/Canvas";
import { AUTOSAVE_MS } from "../editor/sync";
import { setLang } from "../i18n";
import { PALETTE } from "../nodes/defs";
import { emptyLive, useRuns } from "../stores/runs";
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
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  // 채팅 패널(갈래 D) — 패널을 여는 테스트가 실제 invoke를 타지 않게.
  vi.spyOn(api, "getChat").mockResolvedValue({ messages: [], state: { outline: [], baseline: null, tests: {}, pendingFinish: null }, usage: [] });
  vi.spyOn(api, "onChatStage").mockResolvedValue(() => {});
  vi.spyOn(api, "listCredentials").mockResolvedValue({
    apiKeys: { anthropic: [], openai: [], gemini: [] },
    claudeAccounts: [],
    codexAccounts: [],
    keychainError: null,
  });
});
afterEach(() => {
  cleanup();
  useRuns.setState({ live: {} });
  vi.useRealTimers();
  vi.clearAllMocks();
});

const open = () => act(async () => render(<Editor id={null} onBack={() => {}} onOpenRuns={() => {}} />));
const palette = () => screen.queryByRole("dialog", { name: "노드 추가" });
const search = () => within(palette()!).getByRole("combobox");
const addNode = async (event: string) => {
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  // 검색으로 찾는다 — 캔버스에 트리거가 있으면 앱 안 트리거가 접혀 카테고리 옆 창엔 안 보일 수 있다.
  await act(async () => fireEvent.change(search(), { target: { value: nodeTitle(event) } }));
  await act(async () => fireEvent.click(palette()!.querySelector(`[data-event="${event}"]`)!));
};

test("editor_palette_opens_from_plus_button", async () => {
  const r = await open();
  // 왼쪽 고정 팔레트 없음 — [+]를 눌러야 뜬다.
  expect(palette()).toBeNull();
  const plus = screen.getByRole("button", { name: "노드 추가" });
  await act(async () => fireEvent.click(plus));
  expect(plus.getAttribute("aria-expanded")).toBe("true");
  expect(palette()).toBeTruthy();
  // 검색 칸에서 Esc로 닫힌다.
  await act(async () => fireEvent.keyDown(search(), { key: "Escape" }));
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

test("palette_escape_does_not_close_side_panel", async () => {
  await open();
  await addNode(PALETTE[0]);
  expect(screen.getByRole("complementary")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  await act(async () => fireEvent.keyDown(search(), { key: "Escape" }));
  // 팔레트만 닫히고 설정 패널은 남는다.
  expect(palette()).toBeNull();
  expect(screen.getByRole("complementary")).toBeTruthy();
});

test("slash_opens_palette_outside_inputs", async () => {
  await open();
  await act(async () => fireEvent.keyDown(document.body, { key: "/" }));
  expect(palette()).toBeTruthy();
  expect(document.activeElement).toBe(search());
  // 포커스가 body로 빠져도 다시 /로 검색 칸을 되찾는다.
  // jsdom에선 body.focus()가 아무 일도 안 한다 — blur로 실제로 뺀다.
  await act(async () => search().blur());
  expect(document.activeElement).not.toBe(search());
  await act(async () => fireEvent.keyDown(document.body, { key: "/" }));
  expect(palette()).toBeTruthy();
  expect(document.activeElement).toBe(search());
  await act(async () => fireEvent.keyDown(search(), { key: "Escape" }));
  expect(palette()).toBeNull();
  // 입력 칸에서 친 /는 글자다.
  await act(async () => fireEvent.keyDown(screen.getByLabelText("워크플로우 이름"), { key: "/" }));
  expect(palette()).toBeNull();
  // ⌘/는 다른 단축키 몫.
  await act(async () => fireEvent.keyDown(document.body, { key: "/", metaKey: true }));
  expect(palette()).toBeNull();
});

test("palette_floats_above_panels", async () => {
  await open();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  const dialog = palette()!;
  // 채팅(z-30)·설정 패널(z-20) 위로, 옆 창이 잘리지 않게 dialog 자체는 스크롤하지 않는다.
  expect(dialog.parentElement!.className).toContain("z-40");
  expect(dialog.className).not.toContain("overflow-y-auto");
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
  await act(async () => fireEvent.click(within(screen.getByTestId("field-provider")).getByRole("button")));
  await act(async () => fireEvent.click(screen.getByRole("option", { name: /^Claude API/ })));
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
const openSaved = async (e: WorkflowEntry, onOpenIntegrations?: () => void) => {
  useWorkflows.setState({ list: [e] });
  return act(async () => render(<Editor id="w1" onBack={() => {}} onOpenRuns={() => {}} onOpenIntegrations={onOpenIntegrations} />));
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

test("rejected_autosave_edit_still_asks_before_leaving", async () => {
  vi.useFakeTimers();
  vi.spyOn(api, "saveWorkflow").mockRejectedValue("거부");
  const onDirty = vi.fn();
  useWorkflows.setState({ list: [entry([N("a", 0)], "v1")] });
  await act(async () => render(<Editor id="w1" onBack={() => {}} onOpenRuns={() => {}} onDirty={onDirty} />));
  await openAssistant();
  await rename("하나");
  expect(onDirty).toHaveBeenLastCalledWith(false); // 곧 자동 저장된다 — 묻지 않는다
  await tick(AUTOSAVE_MS);
  expect(screen.getByText("거부")).toBeTruthy();
  expect(onDirty).toHaveBeenLastCalledWith(true); // 저장되지 못한 편집 — 떠날 때 묻는다
  await rename("둘"); // 다시 고치면 자동 저장이 다시 맡는다
  expect(onDirty).toHaveBeenLastCalledWith(false);
});

test("edits_on_new_workflow_whose_first_save_failed_ask_before_leaving", async () => {
  vi.spyOn(api, "saveWorkflow").mockRejectedValue("저장 실패");
  const onDirty = vi.fn();
  await act(async () => render(<Editor id={null} onBack={() => {}} onOpenRuns={() => {}} onDirty={onDirty} />));
  await openAssistant(); // 선저장 실패 → id가 없어 자동 저장이 돌지 않는다
  await rename("고침");
  expect(onDirty).toHaveBeenLastCalledWith(true);
});

test("editor_reports_the_id_it_shows", async () => {
  vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, id: "w9", updatedAt: "v1" }));
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  const onIdChange = vi.fn();
  const r = await act(async () => render(<Editor id={null} onBack={() => {}} onOpenRuns={() => {}} onIdChange={onIdChange} />));
  expect(onIdChange).toHaveBeenLastCalledWith("");
  await openAssistant(); // 선저장으로 id가 생긴다
  expect(onIdChange).toHaveBeenLastCalledWith("w9");
  r.unmount();
  expect(onIdChange).toHaveBeenLastCalledWith(""); // 떠나면 비운다
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

test("enable_switch_hidden_for_manual_trigger", async () => {
  const trig = (event: string, config: Record<string, unknown>): WfNode => ({ id: "t", event, type: "TRIGGER", label: "t", config, position: { x: 0, y: 0 } });
  await openSaved(entry([trig("manual.run", {})], "v1"));
  expect(screen.queryByRole("switch", { name: "활성" })).toBeNull();
  for (const n of [trig("schedule.daily", { hour: "9" }), trig("local.folder_new_file", { folder: "/in" })]) {
    cleanup();
    await openSaved(entry([n], "v1"));
    expect(screen.getByRole("switch", { name: "활성" })).toBeTruthy();
  }
});

const GH_MSG = "GitHub 연결이 필요해요.";
const failRun = (status: "failed" | "cancelled" = "failed") =>
  useRuns.setState({
    live: { w1: { ...emptyLive("w1"), runId: "r1", status, nodes: { m: "failed" }, errors: { m: GH_MSG }, codes: { m: "GITHUB_AUTH" } } },
  });

test("failed_run_shows_card_and_strip", async () => {
  failRun();
  await openSaved(entry([N("m", 0)], "v1"));
  expect(screen.getByRole("alert")).toBeTruthy();
  expect(screen.getByText(GH_MSG)).toBeTruthy(); // 노드 위 오류 띠
});

test("show_node_opens_panel", async () => {
  failRun();
  await openSaved(entry([N("m", 0)], "v1"));
  expect(screen.queryByRole("complementary", { name: "m" })).toBeNull();
  await act(async () => fireEvent.click(within(screen.getByRole("alert")).getByRole("button", { name: "노드 보기" })));
  expect(screen.getByRole("complementary", { name: "m" })).toBeTruthy();
});

test("open_integrations_calls_prop", async () => {
  failRun();
  const onOpenIntegrations = vi.fn();
  await openSaved(entry([N("m", 0)], "v1"), onOpenIntegrations);
  await act(async () => fireEvent.click(within(screen.getByRole("alert")).getByRole("button", { name: "설정 > 연동 열기" })));
  expect(onOpenIntegrations).toHaveBeenCalledTimes(1);
  cleanup();
  await openSaved(entry([N("m", 0)], "v1"));
  expect(screen.queryByRole("button", { name: "설정 > 연동 열기" })).toBeNull();
});

test("cancelled_run_no_strip", async () => {
  failRun("cancelled");
  await openSaved(entry([N("m", 0)], "v1"));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText(GH_MSG)).toBeNull();
});

test("next_failure_shows_card_again_after_dismiss_with_motion", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  vi.useFakeTimers();
  try {
    failRun();
    await openSaved(entry([N("m", 0)], "v1"));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "오류 알림 닫기" })));
    await act(async () => void vi.advanceTimersByTime(160));
    expect(screen.queryByRole("alert")).toBeNull();
    // 다음 실행(r2)도 실패 — 닫는 동안 다음 실행이 끼어도 새 카드가 닫히지 않게 실행마다 새로 그린다.
    act(() => useRuns.setState({ live: { w1: { ...useRuns.getState().live.w1!, runId: "r2" } } }));
    expect(screen.getByRole("alert").className).toContain("ieum-rise-in");
  } finally {
    vi.unstubAllGlobals();
  }
});

test("card_stays_bottom_right_above_panels", async () => {
  failRun();
  const r = await openSaved(entry([N("m", 0)], "v1"));
  await openAssistant();
  await act(async () => fireEvent.click(r.container.querySelector(".react-flow__node")!));
  // 채팅창(z-30)·노드 패널이 열려 있어도 그 왼쪽으로 밀리지 않고 우측 하단 맨 위에 뜬다.
  const card = screen.getByRole("alert");
  expect(card.style.right).toBe("");
  expect(card.className).toContain("right-4");
  expect(card.className).toContain("z-40");
});

test("node_panel_sits_left_of_open_chat", async () => {
  failRun();
  const r = await openSaved(entry([N("m", 0)], "v1"));
  await openAssistant(); // 채팅 탭 폭 380 + 16 = cover 396
  await act(async () => fireEvent.click(r.container.querySelector(".react-flow__node")!));
  expect(screen.getByRole("complementary", { name: "m" }).style.right).toBe("408px");
});

const panelOf = async (r: { container: HTMLElement }, name: string) => {
  await act(async () => fireEvent.click(r.container.querySelector(".react-flow__node")!));
  return screen.getByRole("complementary", { name });
};

test("test_runs_single_node_with_draft", async () => {
  const testNode = vi.spyOn(api, "testNode").mockResolvedValue({
    status: "succeeded",
    output: { trashed: "/in/a.pdf" },
    error: null,
    errorCode: null,
    testedAt: new Date().toISOString(),
    missing: [],
    undone: false,
    undoFailed: 0,
    runId: "r1",
  });
  const runNow = vi.spyOn(api, "runNow").mockResolvedValue("r0");
  const r = await openSaved(entry([N("a", 0)], "v1"));
  const panel = await panelOf(r, "a");
  // 저장하지 않은 편집이 있어도 막지 않고 편집본을 보낸다.
  await act(async () => fireEvent.change(within(panel).getByLabelText("노드 이름"), { target: { value: "고친 이름" } }));
  const button = within(screen.getByRole("complementary")).getByRole("button", { name: "테스트" });
  expect((button as HTMLButtonElement).disabled).toBe(false);
  await act(async () => fireEvent.click(button));
  expect(testNode).toHaveBeenCalledTimes(1);
  const [id, wf, nodeId] = testNode.mock.calls[0];
  expect([id, nodeId]).toEqual(["w1", "a"]);
  expect(wf.nodes[0].label).toBe("고친 이름");
  expect(runNow).not.toHaveBeenCalled();
  expect(screen.getByText("trashed")).toBeTruthy();
});

test("test_disabled_without_id", async () => {
  await open();
  await addNode(PALETTE[0]);
  const button = within(screen.getByRole("complementary")).getByRole("button", { name: "테스트" }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  expect(button.title).toBe("먼저 저장해야 실행할 수 있어요.");
  expect(api.getSamples).not.toHaveBeenCalled();
});

test("sample_loaded_on_open", async () => {
  vi.mocked(api.getSamples).mockResolvedValueOnce({
    a: { status: "succeeded", output: { kept: 3 }, error: null, errorCode: null, testedAt: new Date().toISOString(), node: { event: "local.file_trash", config: {} } },
  });
  const r = await openSaved(entry([N("a", 0)], "v1"));
  const panel = await panelOf(r, "a");
  expect(api.getSamples).toHaveBeenCalledWith("w1");
  expect(within(panel).getByText("성공")).toBeTruthy();
  expect(within(panel).getByText("kept")).toBeTruthy();
  expect(within(panel).queryByText("설정을 바꿨어요. 결과는 바꾸기 전 기준이에요.")).toBeNull();
});

test("last_run_failure_line", async () => {
  failRun();
  const r = await openSaved(entry([N("m", 0)], "v1"));
  const panel = await panelOf(r, "m");
  expect(within(panel).getByText(`마지막 실행: 실패 · ${GH_MSG}`)).toBeTruthy();
  cleanup();
  failRun("cancelled");
  const r2 = await openSaved(entry([N("m", 0)], "v1"));
  const panel2 = await panelOf(r2, "m");
  expect(within(panel2).queryByText(/마지막 실행/)).toBeNull();
});

// ── 시작 조건 넣기·바꾸기 ──
const TRIG: WfNode = { id: "t", event: "schedule.daily", type: "TRIGGER", label: "매일 아침 9시", config: { hour: "9", weekends: true }, position: { x: 0, y: 0 } };
const USES: WfNode = { ...N("a", 320), config: { note: "{{nodes.t.output.triggeredAt}}" } };
const EDGES = [{ id: "e1", source: "t", target: "a" }];
const openWithTrigger = (nodes: WfNode[] = [TRIG, USES]) => openSaved({ ...entry(nodes, "v1"), enabled: true, workflow: { nodes, edges: EDGES } });
const replaceCard = () => screen.queryByRole("dialog", { name: /^시작 조건 '/ });
const unsavedDot = () => screen.queryByRole("status", { name: "저장 안 된 변경" });
const saveNow = async () => {
  const saveSpy = vi.spyOn(api, "saveWorkflow").mockImplementation(async (wf) => ({ ...wf, updatedAt: "v2" }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "저장" })));
  return saveSpy.mock.calls[0][0];
};

test("replacing_trigger_keeps_id_and_edges", async () => {
  vi.spyOn(api, "githubOptions").mockResolvedValue([] as never);
  await openWithTrigger();
  // 시작 조건이 있으면 팔레트의 앱 안 트리거는 접힌다 — hasTrigger가 팔레트까지 간다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  await act(async () => fireEvent.click(within(palette()!).getByRole("button", { name: "GitHub" })));
  expect(palette()!.querySelector('[data-event="github.new_issue"]')).toBeNull();
  await act(async () => fireEvent.keyDown(search(), { key: "Escape" }));
  await act(async () => fireEvent.keyDown(search(), { key: "Escape" }));
  expect(palette()).toBeNull();

  await addNode("github.new_issue");
  const card = replaceCard()!;
  expect(card.textContent).toContain("매일 아침 9시");
  expect(card.textContent).toContain("새 GitHub 이슈");
  expect(within(card).getByText("뒤 노드 1개가 이 시작 조건의 결과를 쓰고 있어요. 바꾸면 그 참조가 끊길 수 있어요.")).toBeTruthy();
  expect(within(card).getByText("바꾸면 워크플로우가 꺼져요. 설정을 확인한 뒤 다시 켜 주세요.")).toBeTruthy();
  await act(async () => fireEvent.click(within(card).getByRole("button", { name: "바꾸기" })));
  expect(replaceCard()).toBeNull();
  expect(screen.getByRole("complementary", { name: "새 GitHub 이슈" })).toBeTruthy();
  const sent = await saveNow();
  expect(sent.enabled).toBe(false);
  expect(sent.workflow.nodes).toHaveLength(2);
  expect(sent.workflow.nodes[0]).toMatchObject({ id: "t", event: "github.new_issue", label: "새 GitHub 이슈", position: { x: 0, y: 0 } });
  expect(sent.workflow.edges).toEqual(EDGES);
});

test("cancel_keeps_workflow", async () => {
  await openWithTrigger();
  await addNode("manual.run");
  await act(async () => fireEvent.click(within(replaceCard()!).getByRole("button", { name: "취소" })));
  expect(replaceCard()).toBeNull();
  expect(unsavedDot()).toBeNull();
});

test("same_trigger_selects_without_card", async () => {
  await openWithTrigger();
  await addNode("schedule.daily");
  expect(replaceCard()).toBeNull();
  expect(screen.getByRole("complementary", { name: "매일 아침 9시" })).toBeTruthy();
  expect(unsavedDot()).toBeNull();
});

test("switch_to_manual_keeps_enabled", async () => {
  await openWithTrigger();
  await addNode("manual.run");
  const card = replaceCard()!;
  expect(within(card).queryByText("바꾸면 워크플로우가 꺼져요. 설정을 확인한 뒤 다시 켜 주세요.")).toBeNull();
  await act(async () => fireEvent.click(within(card).getByRole("button", { name: "바꾸기" })));
  const sent = await saveNow();
  expect(sent.enabled).toBe(true);
  expect(sent.workflow.nodes[0]).toMatchObject({ id: "t", event: "manual.run" });
});

test("two_triggers_just_append", async () => {
  await openWithTrigger([TRIG, { ...TRIG, id: "u", label: "또 하나" }, USES]);
  await addNode("manual.run");
  expect(replaceCard()).toBeNull();
  const sent = await saveNow();
  expect(sent.workflow.nodes).toHaveLength(4);
  expect(sent.workflow.nodes[3].event).toBe("manual.run");
});

test("first_trigger_goes_to_front", async () => {
  await openSaved(entry([N("a", 0)], "v1"));
  await addNode("manual.run");
  expect(within(screen.getByRole("complementary", { name: "수동 실행" })).getByText(/^1단계/)).toBeTruthy();
  const sent = await saveNow();
  expect(sent.workflow.nodes.map((n) => [n.event, n.position])).toEqual([
    ["manual.run", { x: -320, y: 0 }],
    ["local.file_trash", { x: 0, y: 0 }],
  ]);
});

test("panel_remounts_after_replace", async () => {
  const r = await openWithTrigger();
  await panelOf(r, "매일 아침 9시");
  const before = vi.mocked(api.getSamples).mock.calls.length;
  await addNode("manual.run");
  await act(async () => fireEvent.click(within(replaceCard()!).getByRole("button", { name: "바꾸기" })));
  // 설정·테스트 칸이 새 트리거 기준으로 다시 그려진다(같은 id여도).
  expect(vi.mocked(api.getSamples).mock.calls.length).toBe(before + 1);
});

test("slash_ignored_while_replace_card_open", async () => {
  await openWithTrigger();
  await addNode("manual.run");
  expect(replaceCard()).toBeTruthy();
  await act(async () => fireEvent.keyDown(document.body, { key: "/" }));
  expect(palette()).toBeNull();
});

test("palette_escape_from_body_keeps_side_panel", async () => {
  await open();
  await addNode(PALETTE[0]);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  await act(async () => fireEvent.change(search(), { target: { value: "이슈" } }));
  // 빈 곳 클릭·스크롤바·Tab으로 포커스가 검색 칸을 떠나 body에 있는 상태.
  await act(async () => search().blur());
  expect(document.activeElement).toBe(document.body);
  await act(async () => fireEvent.keyDown(document.body, { key: "Escape" }));
  expect((search() as HTMLInputElement).value).toBe("");
  await act(async () => fireEvent.keyDown(document.body, { key: "Escape" }));
  expect(palette()).toBeNull();
  // 팔레트가 한 단계씩 받았고 설정 패널은 남는다.
  expect(screen.getByRole("complementary")).toBeTruthy();
});

