import { expect, test } from "vitest";
import { formatDuration, formatSuccessRate, hourlySeries } from "./format";

// 비율은 BE와 로컬 모두 0~100 — 1%를 100%로 짐작해 키우지 않는다.
test("format_success_rate_takes_percent", () => {
  expect(formatSuccessRate(97.8)).toBe("97.8%");
  expect(formatSuccessRate(100)).toBe("100.0%");
  expect(formatSuccessRate(1)).toBe("1.0%");
});

test("format_duration_units", () => {
  expect(formatDuration(0.25)).toBe("250ms");
  expect(formatDuration(3.21)).toBe("3.2s");
  expect(formatDuration(125)).toBe("2m 5s");
});

test("hourly_series_fills_24_hours", () => {
  const s = hourlySeries({ "9": 3, "13": 1 });
  expect(s).toHaveLength(24);
  expect(s[0]).toEqual({ t: "00", count: 0 });
  expect(s[9]).toEqual({ t: "09", count: 3 });
  expect(s[13].count).toBe(1);
});
