// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import { beforeAll, expect, test } from "vitest";
import { setLang } from "../i18n";
import { Palette } from "./Palette";

beforeAll(() => setLang("ko"));

test("palette_groups_formatters_by_kind", () => {
  render(<Palette onAdd={() => {}} />);
  const count = (title: string) => within(screen.getByRole("heading", { name: title }).parentElement!).getAllByRole("button").length;
  expect(count("변환")).toBe(1);
  expect(count("글")).toBe(12);
  expect(count("숫자")).toBe(4);
  expect(count("날짜")).toBe(3);
  expect(count("목록")).toBe(3);
  expect(count("HTTP")).toBe(1);
  const titles = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent);
  expect(titles.slice(-6)).toEqual(["변환", "글", "숫자", "날짜", "목록", "HTTP"]);
});
