// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type NodeTestResult, type Sample, type WfNode, type Workflow } from "../api";
import { setLang } from "../i18n";
import { NodeTestSection } from "./NodeTestSection";

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const node = (id: string, event: string, config: WfNode["config"] = {}, label = id): WfNode => ({ id, event, type: "ACTION", label, config });
const result = (r: Partial<NodeTestResult>): NodeTestResult => ({
  status: "succeeded",
  output: {},
  error: null,
  errorCode: null,
  testedAt: new Date().toISOString(),
  missing: [],
  undone: false,
  undoFailed: 0,
  runId: "r1",
  ...r,
});
const sample = (n: WfNode, s: Partial<Sample> = {}): Sample => ({
  status: "succeeded",
  output: { found: true },
  error: null,
  errorCode: null,
  testedAt: new Date().toISOString(),
  node: { event: n.event, config: n.config },
  ...s,
});

const show = (wf: Workflow, n: WfNode, onOpen = () => {}) =>
  act(async () => render(<NodeTestSection workflowId="w1" workflow={wf} node={n} onOpen={onOpen} />));
const press = () => act(async () => fireEvent.click(screen.getByRole("button", { name: "테스트" })));

test("write_node_asks_one_line", async () => {
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  const testNode = vi.spyOn(api, "testNode").mockResolvedValue(result({ output: { status: 201 } }));
  const a = node("a", "http.request", { method: "POST", url: "https://a.example" }, "보내기 A");
  const b = node("b", "http.request", { method: "POST", url: "https://b.example" }, "보내기 B");
  await show({ nodes: [a, b], edges: [] }, b);
  await press();
  // 확인 카드에는 테스트하는 노드 하나만.
  const card = screen.getByRole("dialog", { name: "테스트로 실제 요청을 보낼까요?" });
  expect(within(card).getAllByRole("listitem")).toHaveLength(1);
  expect(within(card).getByText("보내기 B")).toBeTruthy();
  expect(testNode).not.toHaveBeenCalled();
  await act(async () => fireEvent.click(within(card).getByRole("button", { name: "보내기" })));
  expect(testNode).toHaveBeenCalledWith("w1", { nodes: [a, b], edges: [] }, "b");
  // 취소하면 보내지 않는다.
  testNode.mockClear();
  await press();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "취소" })));
  expect(testNode).not.toHaveBeenCalled();
});

test("confirm_sends_what_card_showed", async () => {
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  const testNode = vi.spyOn(api, "testNode").mockResolvedValue(result({ output: { status: 201 } }));
  const a = node("a", "http.request", { method: "POST", url: "https://a.example" }, "보내기 A");
  const r = await show({ nodes: [a], edges: [] }, a);
  await press();
  expect(screen.getByRole("dialog", { name: "테스트로 실제 요청을 보낼까요?" })).toBeTruthy();
  // 카드가 떠 있는 동안 url을 바꿔도 카드에 보인 요청만 보낸다.
  const changed = { ...a, config: { method: "POST", url: "https://b.example/{{nodes.x.output.token}}" } };
  r.rerender(<NodeTestSection workflowId="w1" workflow={{ nodes: [changed], edges: [] }} node={changed} onOpen={() => {}} />);
  const card = screen.getByRole("dialog", { name: "테스트로 실제 요청을 보낼까요?" });
  await act(async () => fireEvent.click(within(card).getByRole("button", { name: "보내기" })));
  expect(testNode).toHaveBeenCalledTimes(1);
  expect(testNode).toHaveBeenCalledWith("w1", { nodes: [a], edges: [] }, "a");
});

test("missing_lists_open_buttons", async () => {
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  vi.spyOn(api, "testNode").mockResolvedValue(result({ status: "missing", output: null, testedAt: null, missing: ["a"], runId: null }));
  const onOpen = vi.fn();
  const a = node("a", "github.find_pull_request", {}, "GitHub PR 찾기");
  const b = node("b", "local.file_trash", { path: "{{nodes.a.output.items}}" });
  await show({ nodes: [a, b], edges: [{ id: "e", source: "a", target: "b" }] }, b, onOpen);
  await press();
  expect(screen.getByText("이 노드가 참조하는 앞 노드를 먼저 테스트해 주세요.")).toBeTruthy();
  expect(screen.getByText("GitHub PR 찾기")).toBeTruthy();
  expect(screen.getByText("· 테스트 결과 없음")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "열기" })));
  expect(onOpen).toHaveBeenCalledWith("a");
});

test("outdated_band_on_config_change", async () => {
  const band = "설정을 바꿨어요. 결과는 바꾸기 전 기준이에요.";
  const n = node("a", "local.file_trash", { path: "/in/a.pdf", keep: true });
  vi.spyOn(api, "getSamples").mockResolvedValue({ a: sample(n) });
  const r = await show({ nodes: [n], edges: [] }, n);
  expect(screen.getByText("found")).toBeTruthy();
  expect(screen.queryByText(band)).toBeNull();
  // 칸 순서만 다르면 같은 설정이다.
  const reordered = { ...n, config: { keep: true, path: "/in/a.pdf" } };
  r.rerender(<NodeTestSection workflowId="w1" workflow={{ nodes: [reordered], edges: [] }} node={reordered} onOpen={() => {}} />);
  expect(screen.queryByText(band)).toBeNull();
  const changed = { ...n, config: { path: "/in/b.pdf", keep: true } };
  r.rerender(<NodeTestSection workflowId="w1" workflow={{ nodes: [changed], edges: [] }} node={changed} onOpen={() => {}} />);
  expect(screen.getByText(band)).toBeTruthy();
  expect(screen.getByText("found")).toBeTruthy(); // 결과는 그대로 둔다
});

test("undone_note", async () => {
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  vi.spyOn(api, "testNode").mockResolvedValue(result({ output: { path: "/in/a.pdf" }, undone: true }));
  const n = node("a", "local.file_move", { to: "/out" });
  await show({ nodes: [n], edges: [] }, n);
  const note = "테스트라 파일은 원래 자리로 되돌렸어요 — 뒤 노드는 원래 경로로 테스트돼요.";
  expect(screen.queryByText(note)).toBeNull();
  await press();
  expect(screen.getByText(note)).toBeTruthy();
  expect(screen.getByText("성공")).toBeTruthy();
});

test("failed_result_replaces_sample_invalid_keeps_it", async () => {
  const n = node("a", "local.file_trash", { path: "/in/a.pdf" });
  vi.spyOn(api, "getSamples").mockResolvedValue({ a: sample(n, { output: { kept: 1 } }) });
  const testNode = vi.spyOn(api, "testNode").mockResolvedValue(result({ status: "invalid", output: null, error: "경로를 입력해 주세요.", testedAt: null, runId: null }));
  await show({ nodes: [n], edges: [] }, n);
  await press();
  // 검증 실패는 실행하지 않았다 — 문구만, 샘플은 아래에 그대로.
  expect(screen.getByText("경로를 입력해 주세요.")).toBeTruthy();
  expect(screen.getByText("kept")).toBeTruthy();
  testNode.mockResolvedValue(result({ status: "failed", output: null, error: "이 저장소·조직을 읽을 권한이 없어요." }));
  await press();
  expect(screen.getByText("실패")).toBeTruthy();
  expect(screen.getByText("이 저장소·조직을 읽을 권한이 없어요.")).toBeTruthy();
  expect(screen.queryByText("kept")).toBeNull();
});

test("tested_time_label", async () => {
  vi.useFakeTimers({ now: new Date("2026-11-19T06:04:30Z"), toFake: ["Date"] }); // 서울 오후 3:04
  try {
    const n = node("a", "local.file_trash");
    const at = (iso: string) => vi.spyOn(api, "getSamples").mockResolvedValue({ a: sample(n, { testedAt: iso }) });
    at("2026-11-19T06:04:00Z");
    await show({ nodes: [n], edges: [] }, n);
    expect(screen.getByText("방금")).toBeTruthy();
    cleanup();
    at("2026-11-19T05:30:00Z");
    await show({ nodes: [n], edges: [] }, n);
    expect(screen.getByText("오후 2:30")).toBeTruthy();
    cleanup();
    at("2026-11-18T12:12:00Z");
    await show({ nodes: [n], edges: [] }, n);
    expect(screen.getByText("11월 18일 오후 9:12")).toBeTruthy();
  } finally {
    vi.useRealTimers();
  }
});

test("folder_trigger_double_click_opens_one_dialog", async () => {
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  let pick!: (p: string | null) => void;
  const pickPath = vi.spyOn(api, "pickPath").mockReturnValue(new Promise((r) => (pick = r)));
  const testNode = vi.spyOn(api, "testNode").mockResolvedValue(result({}));
  const n = node("f", "local.folder_new_file");
  await show({ nodes: [n], edges: [] }, n);
  await press();
  await press();
  expect(pickPath).toHaveBeenCalledTimes(1);
  // 창을 닫으면(취소) 다시 누를 수 있다.
  await act(async () => pick(null));
  expect(testNode).not.toHaveBeenCalled();
  await press();
  expect(pickPath).toHaveBeenCalledTimes(2);
});

test("running_indicator_while_testing", async () => {
  vi.spyOn(api, "getSamples").mockResolvedValue({});
  let done!: (r: NodeTestResult) => void;
  vi.spyOn(api, "testNode").mockReturnValue(new Promise((r) => (done = r)));
  const n = node("a", "http.request", { method: "GET", url: "https://a.example" });
  await show({ nodes: [n], edges: [] }, n);
  await press();
  expect(screen.getByRole("status").textContent).toBe("실행 중");
  await act(async () => done(result({})));
  expect(screen.queryByRole("status")).toBeNull();
});

test("failed_sample_without_error_has_no_empty_band", async () => {
  const n = node("a", "http.request", { method: "GET", url: "https://a.example" });
  vi.spyOn(api, "getSamples").mockResolvedValue({ a: sample(n, { status: "failed", output: null, error: null }) });
  const { container } = await show({ nodes: [n], edges: [] }, n);
  expect(screen.getByText("실패")).toBeTruthy();
  expect(container.querySelector(".bg-danger-100")).toBeNull();
});
