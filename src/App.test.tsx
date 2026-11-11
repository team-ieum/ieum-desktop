// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import App from "./App";
import { api } from "./api";

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
  vi.spyOn(api, "getSettings").mockResolvedValue({ language: "ko", autostart: false, firstRunDone: true, serverUrl: "" });
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "validateWorkflow").mockResolvedValue([]);
});
afterEach(cleanup);

test("leaving_editor_with_unsaved_changes_asks_in_app", async () => {
  await act(async () => {
    render(<App />);
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "새 워크플로우" }));
  });
  const history = screen.getByRole("button", { name: "기록" });

  // 고치기 전에는 묻지 않고 바로 간다 — 다시 편집기로.
  await act(async () => fireEvent.click(history));
  expect(screen.queryByRole("alert")).toBeNull();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "워크플로우" })));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "새 워크플로우" })));

  await act(async () => fireEvent.change(screen.getByLabelText("워크플로우 이름"), { target: { value: "고침" } }));

  // 탭 이동: 취소하면 편집기와 고친 내용이 남는다.
  await act(async () => fireEvent.click(history));
  expect(screen.getByRole("alert").textContent).toContain("저장하지 않은 변경");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "취소" })));
  expect(screen.queryByRole("alert")).toBeNull();
  expect((screen.getByLabelText("워크플로우 이름") as HTMLInputElement).value).toBe("고침");

  // 뒤로: 버리기를 눌러야 목록으로 간다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "뒤로" })));
  expect(screen.getByLabelText("워크플로우 이름")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "버리기" })));
  expect(screen.queryByLabelText("워크플로우 이름")).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});
