// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type RunSummary, type Settings } from "../api";
import { setLang } from "../i18n";
import { useRuns } from "../stores/runs";
import { useSettings } from "../stores/settings";
import { NotificationsMenu, POLL_MS } from "./NotificationsMenu";

const SEEN = "2026-11-12T10:00:00Z";
const SETTINGS: Settings = { language: "ko", autostart: false, firstRunDone: true, serverUrl: "", notificationsSeenAt: SEEN };
const failed = (runId: string, at: string): RunSummary => ({
  runId,
  workflowId: "w1",
  status: "failed",
  startedAt: at,
  finishedAt: at,
  fileOps: 0,
  undone: false,
});

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useRuns.setState({ runs: [] });
  useSettings.setState({ settings: null });
});

test("badge_counts_only_after_seen_at", async () => {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([failed("r1", "2026-11-12T11:00:00Z"), failed("r0", "2026-11-12T09:00:00Z")]);
  vi.spyOn(api, "dashboardErrors").mockResolvedValue({
    content: [{ executionId: "e1", workflowId: "x", workflowName: "웹 실패", failedNodeId: null, errorMessage: "boom", startedAt: "2026-11-12T12:00:00Z" }],
    hasNext: false,
    nextCursor: null,
  });
  vi.spyOn(api, "getSettings").mockResolvedValue(SETTINGS);
  const set = vi.spyOn(api, "setSettings").mockResolvedValue();
  useSettings.setState({ settings: SETTINGS });

  await act(async () => {
    render(<NotificationsMenu onOpenRun={() => {}} />);
  });
  expect(screen.getByTestId("bell-badge").textContent).toBe("2");

  await act(async () => fireEvent.click(screen.getByRole("button", { name: "알림" })));
  expect(set).toHaveBeenCalledTimes(1);
  const seen = set.mock.calls[0][0].notificationsSeenAt!;
  expect(Date.parse(seen)).toBeGreaterThan(Date.parse(SEEN));
  expect(screen.getAllByRole("menuitem")).toHaveLength(3);

  // 닫을 때는 저장하지 않는다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "알림" })));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(set).toHaveBeenCalledTimes(1);
});

const mockApis = () => {
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "getSettings").mockResolvedValue(SETTINGS);
  vi.spyOn(api, "setSettings").mockResolvedValue();
  useSettings.setState({ settings: SETTINGS });
  return vi.spyOn(api, "dashboardErrors").mockResolvedValue({ content: [], hasNext: false, nextCursor: null });
};

test("web_errors_poll_while_visible_and_local_follows_runs", async () => {
  vi.useFakeTimers();
  try {
    const errs = mockApis();
    let visibility = "visible";
    vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility as DocumentVisibilityState);
    await act(async () => {
      render(<NotificationsMenu onOpenRun={() => {}} />);
    });
    expect(errs).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTime(POLL_MS));
    expect(errs).toHaveBeenCalledTimes(2);

    visibility = "hidden";
    await act(async () => vi.advanceTimersByTime(POLL_MS));
    expect(errs).toHaveBeenCalledTimes(2);
    visibility = "visible";
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(errs).toHaveBeenCalledTimes(3);

    // runFinished → useRuns.load로 들어온 새 실패가 배지에 바로 뜬다.
    expect(screen.queryByTestId("bell-badge")).toBeNull();
    await act(async () => useRuns.setState({ runs: [failed("r9", "2026-11-12T13:00:00Z")] }));
    expect(screen.getByTestId("bell-badge").textContent).toBe("1");
  } finally {
    vi.useRealTimers();
  }
});

test("menu_keyboard_esc_and_arrows", async () => {
  mockApis();
  useRuns.setState({ runs: [failed("a", "2026-11-12T11:00:00Z"), failed("b", "2026-11-12T12:00:00Z")] });
  vi.mocked(api.listRuns).mockResolvedValue(useRuns.getState().runs);
  await act(async () => {
    render(<NotificationsMenu onOpenRun={() => {}} />);
  });
  const bell = screen.getByRole("button", { name: "알림" });
  await act(async () => fireEvent.click(bell));
  const items = screen.getAllByRole("menuitem");
  expect(document.activeElement).toBe(items[0]);
  fireEvent.keyDown(items[0], { key: "ArrowDown" });
  expect(document.activeElement).toBe(items[1]);
  fireEvent.keyDown(items[1], { key: "ArrowDown" });
  expect(document.activeElement).toBe(items[0]);
  await act(async () => fireEvent.keyDown(items[0], { key: "Escape" }));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(bell);
});
