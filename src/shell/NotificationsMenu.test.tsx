// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type RunSummary, type Settings } from "../api";
import { setLang } from "../i18n";
import { useRuns } from "../stores/runs";
import { useSettings } from "../stores/settings";
import { NotificationsMenu } from "./NotificationsMenu";

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
});
