// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api, type WorkflowEntry } from "../api";
import { setLang } from "../i18n";
import { useWeb } from "../stores/web";
import { useWorkflows } from "../stores/workflows";
import { SearchDialog } from "./SearchDialog";

const local = (id: string, name: string) => ({ id, name }) as WorkflowEntry;

beforeAll(() => setLang("ko"));
beforeEach(() => {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([local("l1", "청구서 PDF 정리"), local("l2", "다운로드 정리")]);
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([{ id: "w1", name: "PDF 요약 메일", active: true }]);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkflows.setState({ list: [] });
  useWeb.setState({ offline: false });
});

const mount = async (onPick = vi.fn()) => {
  await act(async () => {
    render(<SearchDialog open onClose={() => {}} onPick={onPick} />);
  });
  return onPick;
};
const type = (v: string) => fireEvent.change(screen.getByRole("combobox"), { target: { value: v } });

test("search_finds_local_and_web_case_insensitive", async () => {
  await mount();
  type("pdf");
  const opts = screen.getAllByRole("option").map((o) => o.textContent);
  expect(opts).toHaveLength(2);
  expect(opts.some((o) => o?.includes("청구서 PDF 정리"))).toBe(true);
  expect(opts.some((o) => o?.includes("PDF 요약 메일"))).toBe(true);
});

test("search_enter_opens_pick", async () => {
  const onPick = await mount();
  type("PDF");
  fireEvent.keyDown(screen.getByRole("combobox"), { key: "ArrowDown" });
  fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
  expect(onPick).toHaveBeenCalledWith({ kind: "web", id: "w1" });
});

test("search_web_offline_still_lists_local", async () => {
  vi.mocked(api.webListWorkflows).mockRejectedValue({ code: "NETWORK" });
  await mount();
  type("정리");
  expect(screen.getAllByRole("option")).toHaveLength(2);
  expect(screen.getByText("웹 이음 연결 안 됨")).toBeTruthy();
});
