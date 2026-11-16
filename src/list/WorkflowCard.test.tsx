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
  nextRun: null,
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

const sched = (event: string, config: Record<string, unknown>, nextRun: string | null = null) =>
  localCard(entry({ nextRun, workflow: { nodes: [{ id: "t", event, config }], edges: [] } }));

test("schedule_card_summaries", () => {
  expect(sched("schedule.hourly", { weekends: true }).triggerDetail).toBe("매시 정각");
  expect(sched("schedule.hourly", { weekends: false }).triggerDetail).toBe("평일 매시 정각");
  expect(sched("schedule.daily", { hour: "9", weekends: true }).triggerDetail).toBe("매일 오전 9시");
  expect(sched("schedule.daily", { hour: "9", weekends: false }).triggerDetail).toBe("평일 매일 오전 9시");
  expect(sched("schedule.daily", { hour: "9", weekends: "false" }).triggerDetail).toBe("평일 매일 오전 9시");
  expect(sched("schedule.hourly", { weekends: "false" }).triggerDetail).toBe("평일 매시 정각");
  expect(sched("schedule.weekly", { weekday: "MON", hour: "9" }).triggerDetail).toBe("매주 월요일 오전 9시");
  expect(sched("schedule.monthly", { day: "15", hour: "0" }).triggerDetail).toBe("매월 15일 자정");
  expect(sched("schedule.monthly", { day: "L", hour: "9" }).triggerDetail).toBe("매월 마지막 날 오전 9시");
  expect(sched("schedule.every", { cron: "0 9 * * 5" }).triggerDetail).toBe("cron 0 9 * * 5");
  expect(sched("schedule.daily", { hour: "9" }).trigger).toBe("schedule");
  expect(sched("manual.run", {}).trigger).toBe("manual");
  // 수동 실행은 켜고 끌 스위치가 없다 — 꺼진 채 저장돼도 "활성".
  expect(localCard(entry({ enabled: false, workflow: { nodes: [{ id: "t", event: "manual.run", config: {} }], edges: [] } })).status).toBe("active");
  expect(localCard(entry({ enabled: false })).status).toBe("paused");
});

test("schedule_card_shows_next_run", () => {
  render(<WorkflowCard data={sched("schedule.daily", { hour: "9" }, "2026-11-17T09:00:00")} onOpen={() => {}} menu={[]} />);
  expect(screen.getByText(/매일 오전 9시/)).toBeTruthy();
  expect(screen.getByText("다음 11/17 09:00")).toBeTruthy();
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
  await act(async () => fireEvent.click(screen.getByRole("menuitem", { name: "이음 클라우드에서 열기" })));
  expect(open).toHaveBeenCalledWith("aaaaaaaa-0000-4000-8000-000000000001");
});
