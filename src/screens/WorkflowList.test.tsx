// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type WfNode, type WorkflowEntry } from "../api";
import { setLang } from "../i18n";
import { WorkflowList } from "./WorkflowList";

const wf = (id: string, name: string, events: string[]): WorkflowEntry => ({
  id,
  name,
  enabled: true,
  updatedAt: "2026-11-12T00:00:00Z",
  watch: "watching",
  watchMessage: null,
  workflow: { nodes: events.map((event, i): WfNode => ({ id: `n${i}`, event, config: {} })), edges: [] },
});

beforeAll(() => {
  setLang("ko");
  vi.spyOn(api, "listWorkflows").mockResolvedValue([
    wf("1", "청구서 AI 정리", ["local.folder_new_file", "local.ai"]),
    wf("2", "청구서 옮기기", ["local.folder_new_file", "local.file_move"]),
    wf("3", "사진 AI 이름", ["local.ai"]),
  ]);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const names = () => screen.queryAllByRole("heading", { level: 3 }).map((h) => h.textContent);

test("filter_and_search_combine", async () => {
  await act(async () => {
    render(<WorkflowList onOpen={() => {}} />);
  });
  expect(names()).toHaveLength(3);
  await act(async () => fireEvent.click(screen.getByRole("checkbox", { name: /AI/ })));
  expect(names()).toEqual(expect.arrayContaining(["청구서 AI 정리", "사진 AI 이름"]));
  expect(names()).toHaveLength(2);
  await act(async () => fireEvent.change(screen.getByPlaceholderText("이름, 서비스 검색"), { target: { value: "청구서" } }));
  expect(names()).toEqual(["청구서 AI 정리"]);
  expect(screen.getByText("워크플로우 1개")).toBeTruthy();
});

test("list_view_toggle_survives_storage_error", async () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  await act(async () => {
    render(<WorkflowList onOpen={() => {}} />);
  });
  const list = screen.getByRole("button", { name: "목록으로 보기" });
  expect(list.getAttribute("aria-pressed")).toBe("false");
  await act(async () => fireEvent.click(list));
  expect(list.getAttribute("aria-pressed")).toBe("true");
});
