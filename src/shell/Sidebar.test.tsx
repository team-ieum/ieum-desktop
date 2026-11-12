// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { api, type WebWorkflowSummary, type WorkflowEntry } from "../api";
import { recentItems, Sidebar } from "./Sidebar";

afterEach(cleanup);

const props = {
  current: "local" as const,
  onNavigate: () => {},
  onNewCanvas: () => {},
  recent: [],
  onOpenRecent: () => {},
  collapsed: false,
  onToggleCollapse: () => {},
  version: "0.2.0",
};

test("sidebar_marks_current_page_navy", () => {
  render(<Sidebar {...props} current="history" />);
  const cur = screen.getByRole("button", { name: "실행 기록" });
  expect(cur.getAttribute("aria-current")).toBe("page");
  expect(cur.className).toContain("bg-main-deep-blue");
  expect(cur.className).toContain("text-white");
  expect(screen.getByRole("button", { name: "내 PC" }).className).not.toContain("bg-main-deep-blue");
});

test("sidebar_help_opens_install_doc", () => {
  const help = vi.spyOn(api, "openHelp").mockResolvedValue();
  render(<Sidebar {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "도움말" }));
  expect(help).toHaveBeenCalledTimes(1);
});

const local = (id: string, updatedAt: string): WorkflowEntry => ({
  id,
  name: `local ${id}`,
  enabled: true,
  workflow: { nodes: [], edges: [] },
  updatedAt,
  watch: "watching",
  watchMessage: null,
});
const web = (id: string, updatedAt: string): WebWorkflowSummary => ({ id, name: `web ${id}`, active: false, updatedAt });

test("recent_mixes_local_and_web_by_time", () => {
  const recent = recentItems(
    [local("a", "2026-11-10T00:00:00Z"), local("b", "2026-11-12T00:00:00Z")],
    [web("c", "2026-11-11T00:00:00Z"), web("d", "2026-11-09T00:00:00Z")],
  );
  expect(recent.map((r) => r.id)).toEqual(["b", "c", "a", "d"]);

  render(<Sidebar {...props} recent={recent} />);
  const items = screen.getAllByRole("button", { name: /^(local|web) / });
  expect(items.map((b) => b.textContent?.match(/^(local|web) \w/)?.[0])).toEqual(["local b", "web c", "local a", "web d"]);
  expect(within(items[0]).getByText("내 PC")).toBeTruthy();
  expect(within(items[1]).getByText("웹")).toBeTruthy();
});

test("recent_shows_only_five_with_status_text", () => {
  const recent = recentItems(
    ["a", "b", "c", "d", "e", "f"].map((id, i) => local(id, `2026-11-0${i + 1}T00:00:00Z`)),
    [],
  );
  expect(recent.map((r) => r.id)).toEqual(["f", "e", "d", "c", "b"]);
  render(<Sidebar {...props} recent={recent} />);
  expect(screen.getAllByRole("button", { name: /^local / })).toHaveLength(5);
  expect(screen.getAllByText(/^(활성|일시정지|오류)$/)).toHaveLength(5);
});
