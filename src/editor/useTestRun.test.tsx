// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { api, type WfNode } from "../api";
import { useTestRun } from "./useTestRun";

afterEach(() => vi.restoreAllMocks());

const trig = (event: string, config: Record<string, unknown> = {}): WfNode => ({ id: "t", event, type: "TRIGGER", config });

test("schedule_and_manual_run_without_file", async () => {
  const pick = vi.spyOn(api, "pickPath").mockResolvedValue("/tmp/a.txt");
  const runNow = vi.spyOn(api, "runNow").mockResolvedValue("r1");
  for (const ev of ["schedule.daily", "manual.run"]) {
    const { result } = renderHook(() => useTestRun("w1", [trig(ev, { hour: "9" })]));
    await act(() => result.current.run());
  }
  expect(pick).not.toHaveBeenCalled();
  expect(runNow.mock.calls).toEqual([["w1"], ["w1"]]);
});

test("folder_run_picks_file", async () => {
  const pick = vi.spyOn(api, "pickPath").mockResolvedValue("/tmp/a.txt");
  const runNow = vi.spyOn(api, "runNow").mockResolvedValue("r1");
  const { result } = renderHook(() => useTestRun("w1", [trig("local.folder_new_file", { folder: "/in" })]));
  await act(() => result.current.run());
  expect(pick).toHaveBeenCalledWith(false);
  expect(runNow).toHaveBeenCalledWith("w1", "/tmp/a.txt");
});
