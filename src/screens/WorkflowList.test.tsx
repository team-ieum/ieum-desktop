// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type WfNode, type WorkflowEntry } from "../api";
import { setLang } from "../i18n";
import { saveView } from "../list/ListToolbar";
import { useSettings } from "../stores/settings";
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
  // 저장 실패는 삼킨다 — 핸들러 밖으로 던지지 않는다.
  expect(() => saveView("grid")).not.toThrow();
});

test("delete_asks_unless_confirmation_turned_off", async () => {
  const del = vi.spyOn(api, "deleteWorkflow").mockResolvedValue();
  const base = { language: "ko", autostart: false, firstRunDone: true, serverUrl: "", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true } as const;
  const deleteFirst = async () => {
    await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "메뉴" })[0]));
    await act(async () => fireEvent.click(screen.getByRole("menuitem", { name: "삭제" })));
  };

  useSettings.setState({ settings: base });
  await act(async () => {
    render(<WorkflowList onOpen={() => {}} />);
  });
  await deleteFirst();
  expect(del).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "정말 삭제" })).toBeTruthy();
  cleanup();

  useSettings.setState({ settings: { ...base, confirmDeleteWorkflow: false } });
  await act(async () => {
    render(<WorkflowList onOpen={() => {}} />);
  });
  await deleteFirst();
  expect(del).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "정말 삭제" })).toBeNull();
});
