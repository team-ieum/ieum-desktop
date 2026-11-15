import { afterEach, expect, test } from "vitest";
import { setLang } from "../i18n";
import { formatCount, formatReset, formatTokens, formatUsd } from "./format";

afterEach(() => setLang("ko"));

test("format_count", () => {
  expect([950, 1_234, 34_567, 1_000_000, 3_400_000, 2_500_000_000].map(formatCount)).toEqual(["950", "1.2k", "34.6k", "1M", "3.4M", "2.5B"]);
});

test("format_usd_edges", () => {
  expect(formatUsd(null)).toBe("—");
  expect(formatUsd(0)).toBe("$0.00");
  expect(formatUsd(0.004)).toBe("<$0.01");
  expect(formatUsd(0.42)).toBe("$0.42");
  expect(formatUsd(1.849)).toBe("$1.85");
});

test("format_tokens_ko_en", () => {
  setLang("ko");
  expect(formatTokens(123_000)).toBe("12.3만");
  expect(formatTokens(950)).toBe("950");
  setLang("en");
  expect(formatTokens(123_000)).toBe("123K");
  expect(formatTokens(950)).toBe("950");
});

test("format_reset_days_hours_minutes", () => {
  const now = new Date("2026-11-12T00:00:00Z");
  setLang("ko");
  expect(formatReset("2026-11-14T10:00:00Z", now)).toBe("2일 10시간");
  expect(formatReset("2026-11-12T00:35:00Z", now)).toBe("35분");
  expect(formatReset("2026-11-11T00:00:00Z", now)).toBe("0분");
  setLang("en");
  expect(formatReset("2026-11-14T10:00:00Z", now)).toBe("2d 10h");
});
