// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test } from "vitest";
import { setLang } from "../i18n";
import { LimitMeter, limitLevel } from "./parts";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

test("limit_level_steps", () => {
  expect(limitLevel(7.9, 10)).toBe(0);
  expect(limitLevel(8, 10)).toBe(80);
  expect(limitLevel(10, 10)).toBe(100);
});

test("meter_colors_and_over_text", () => {
  const bar = () => screen.getByRole("progressbar").firstElementChild!.className;
  const { rerender } = render(<LimitMeter spend={9} limit={10} />);
  expect(bar()).toContain("bg-[#c98a12]");
  expect(screen.queryByText("한도 넘음 · 실행 멈춤")).toBeNull();
  rerender(<LimitMeter spend={10} limit={10} />);
  expect(bar()).toContain("bg-[#c2412d]");
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("100");
  expect(screen.getByText("한도 넘음 · 실행 멈춤")).toBeTruthy();
  rerender(<LimitMeter spend={0.4} limit={null} />);
  expect(screen.getByText("이번 달 $0.40 · 한도 없음")).toBeTruthy();
  expect(screen.queryByRole("progressbar")).toBeNull();
});
