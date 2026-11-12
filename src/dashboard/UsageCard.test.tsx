// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type UsageSummary } from "../api";
import { setLang } from "../i18n";
import { UsageCard } from "./UsageCard";

const SUMMARY: UsageSummary = {
  totalUsd: 1.85,
  totalInput: 100_000,
  totalOutput: 23_000,
  unpricedCalls: 0,
  bySource: [{ source: "anthropic", input: 100_000, output: 23_000, usd: 1.85 }],
  byWorkflow: [{ workflowId: "w1", input: 100_000, output: 23_000, usd: 1.85 }],
  byDay: [],
  asOf: "2026-11-01",
};

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test("usage_card_hidden_on_web_tab", async () => {
  const summary = vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await act(async () => {
    render(<UsageCard hidden />);
  });
  expect(screen.queryByText("AI 사용량")).toBeNull();
  expect(summary).not.toHaveBeenCalled();
});

test("usage_card_switches_range", async () => {
  const summary = vi.spyOn(api, "usageSummary").mockResolvedValue(SUMMARY);
  await act(async () => {
    render(<UsageCard hidden={false} />);
  });
  expect(summary).toHaveBeenLastCalledWith("today");
  expect(screen.getByText("$1.85")).toBeTruthy();
  expect(screen.getByText(/2026-11-01/)).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("tab", { name: "이번 주" })));
  expect(summary).toHaveBeenLastCalledWith("week");
});

test("usage_card_empty_when_unavailable", async () => {
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
  await act(async () => {
    render(<UsageCard hidden={false} />);
  });
  expect(screen.getByText("사용량을 아직 불러오지 못했어요.")).toBeTruthy();
});
