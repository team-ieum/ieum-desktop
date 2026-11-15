// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type WorkflowEntry } from "../api";
import { setLang } from "../i18n";
import { Web } from "../screens/Web";
import { localCard, WorkflowCard } from "./WorkflowCard";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

const entry = (over: Partial<WorkflowEntry> = {}): WorkflowEntry => ({
  id: "l1",
  name: "청구서 정리",
  enabled: true,
  updatedAt: "2026-11-12T00:00:00Z",
  watch: "watching",
  watchMessage: null,
  workflow: {
    nodes: [
      { id: "a", event: "local.folder_new_file", config: { folder: "~/Documents/청구서" } },
      { id: "b", event: "local.ai", config: {} },
      { id: "c", event: "local.file_move", config: {} },
    ],
    edges: [],
  },
  ...over,
});

test("local_card_maps_status_and_trigger", () => {
  const { container } = render(<WorkflowCard data={localCard(entry({ enabled: false }))} onOpen={() => {}} menu={[]} />);
  expect(screen.getByText("일시정지")).toBeTruthy();
  expect(container.querySelector("article > span")!.className).toContain("bg-[#959595]");
  expect(screen.getByText(/폴더 감시/).textContent).toContain("~/Documents/청구서");
  expect(screen.getByText("노드 3개")).toBeTruthy();

  cleanup();
  const err = localCard(entry({ watch: "watch_error" }));
  expect(err.status).toBe("error");
  expect(err.services).toEqual(["local", "ai"]);
  expect(localCard(entry({ workflow: { nodes: [{ id: "x", event: "local.file_trash", config: {} }], edges: [] } })).trigger).toBe("manual");
});

test("web_card_menu_opens_in_browser", async () => {
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([
    { id: "aaaaaaaa-0000-4000-8000-000000000001", name: "브리핑", active: true, services: ["DISCORD"], nodeCount: 4 },
  ]);
  const open = vi.spyOn(api, "openWebWorkflow").mockResolvedValue();
  await act(async () => {
    render(<Web onOpen={() => {}} />);
  });
  expect(screen.getByText("노드 4개")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "메뉴" })));
  await act(async () => fireEvent.click(screen.getByRole("menuitem", { name: "웹에서 열기" })));
  expect(open).toHaveBeenCalledWith("aaaaaaaa-0000-4000-8000-000000000001");
});
