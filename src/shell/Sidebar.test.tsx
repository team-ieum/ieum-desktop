// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type WebWorkflowSummary, type WorkflowEntry } from "../api";
import { recentItems, Sidebar, useSidebarCollapsed } from "./Sidebar";

// 이 환경의 localStorage는 온전하지 않다 — 메모리 저장소를 꽂는다.
const memoryStorage = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};
beforeEach(() => vi.stubGlobal("localStorage", memoryStorage()));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const props = {
  current: "local" as const,
  onNavigate: () => {},
  onNewCanvas: () => {},
  onSearch: () => {},
  onCollapse: () => {},
  recent: [],
  onOpenRecent: () => {},
  version: "0.2.0",
};

function CollapseHarness() {
  const [collapsed, toggle] = useSidebarCollapsed();
  return <button onClick={toggle}>{collapsed ? "접힘" : "펼침"}</button>;
}

test("sidebar_state_remembered", () => {
  const r = render(<CollapseHarness />);
  fireEvent.click(screen.getByText("펼침"));
  r.unmount();
  render(<CollapseHarness />);
  expect(screen.getByText("접힘")).toBeTruthy();
  cleanup();
  // 저장소가 막혀도 펼친 상태로 동작.
  const blocked = () => {
    throw new Error("blocked");
  };
  vi.stubGlobal("localStorage", { getItem: blocked, setItem: blocked });
  render(<CollapseHarness />);
  fireEvent.click(screen.getByText("펼침"));
  expect(screen.getByText("접힘")).toBeTruthy();
});

test("sidebar_marks_current_page_light_blue", () => {
  render(<Sidebar {...props} current="history" />);
  const cur = screen.getByRole("button", { name: "실행 기록" });
  expect(cur.getAttribute("aria-current")).toBe("page");
  expect(cur.className).toContain("bg-main-light-blue");
  expect(cur.className).toContain("text-main-deep-blue");
  expect(screen.getByRole("button", { name: "내 PC" }).className).not.toContain("bg-main-light-blue");
  // 새 캔버스도 같은 줄 모양(남색 큰 버튼 아님).
  expect(screen.getByRole("button", { name: /새 캔버스/ }).className).not.toContain("bg-main-deep-blue");
});

test("sidebar_footer_settings_and_help", () => {
  const help = vi.spyOn(api, "openHelp").mockResolvedValue();
  const onNavigate = vi.fn();
  const onCollapse = vi.fn();
  render(<Sidebar {...props} onNavigate={onNavigate} onCollapse={onCollapse} />);
  fireEvent.click(screen.getByRole("button", { name: "설정" }));
  expect(onNavigate).toHaveBeenCalledWith("settings");
  fireEvent.click(screen.getByRole("button", { name: "도움말" }));
  expect(help).toHaveBeenCalledTimes(1);
  expect(screen.getByText("v0.2.0")).toBeTruthy();
  // 첫 줄: IEUM + 접기.
  expect(screen.getByText("IEUM")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "사이드바 접기" }));
  expect(onCollapse).toHaveBeenCalledTimes(1);
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
  expect(within(items[1]).getByText("웹 이음")).toBeTruthy();
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
