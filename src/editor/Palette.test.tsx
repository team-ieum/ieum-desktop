// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { setLang } from "../i18n";
import { Palette } from "./Palette";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

// 펼침 상태는 모듈에 남으므로 각 테스트는 연 묶음을 닫고 끝낸다(마지막 재마운트 테스트 제외).
const toggles = () => screen.getAllByRole("button").filter((b) => b.hasAttribute("aria-expanded"));
const toggle = (title: string) => toggles().find((b) => b.querySelector("[data-title]")?.textContent === title)!;
const nodeButtons = () => screen.queryAllByRole("button").filter((b) => !b.hasAttribute("aria-expanded"));

test("palette_starts_collapsed_with_counts", () => {
  render(<Palette onAdd={() => {}} />);
  expect(nodeButtons()).toHaveLength(0);
  expect(toggles()).toHaveLength(11);
  expect(toggles().every((b) => b.getAttribute("aria-expanded") === "false")).toBe(true);
  expect(toggle("글").textContent).toBe("글12");
  expect(toggle("숫자").textContent).toBe("숫자4");
  expect(toggle("날짜").textContent).toBe("날짜3");
  expect(toggle("목록").textContent).toBe("목록3");
  expect(screen.getByText("노드 추가")).toBeTruthy();
  expect(screen.getByText("내 PC 노드는 이 PC에서 실행돼요")).toBeTruthy();
});

test("palette_toggles_group", () => {
  render(<Palette onAdd={() => {}} />);
  fireEvent.click(toggle("글"));
  expect(toggle("글").getAttribute("aria-expanded")).toBe("true");
  expect(within(screen.getByRole("region", { name: /^글/ })).getAllByRole("button")).toHaveLength(12);
  expect(nodeButtons()).toHaveLength(12);
  fireEvent.click(toggle("글"));
  expect(toggle("글").getAttribute("aria-expanded")).toBe("false");
  expect(nodeButtons()).toHaveLength(0);
});

test("palette_adds_node_from_open_group", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} />);
  fireEvent.click(toggle("HTTP"));
  fireEvent.click(nodeButtons()[0]);
  expect(onAdd).toHaveBeenCalledWith("http.request");
  fireEvent.click(toggle("HTTP"));
});

test("palette_groups_formatters_by_kind", () => {
  render(<Palette onAdd={() => {}} />);
  const count = (title: string) => {
    fireEvent.click(toggle(title));
    const n = within(screen.getByRole("region", { name: new RegExp(`^${title}`) })).getAllByRole("button").length;
    fireEvent.click(toggle(title));
    return n;
  };
  expect(count("변환")).toBe(1);
  expect(count("글")).toBe(12);
  expect(count("숫자")).toBe(4);
  expect(count("날짜")).toBe(3);
  expect(count("목록")).toBe(3);
  expect(count("HTTP")).toBe(1);
  const titles = toggles().map((b) => b.querySelector("[data-title]")?.textContent);
  expect(titles.slice(-6)).toEqual(["변환", "글", "숫자", "날짜", "목록", "HTTP"]);
});

test("palette_keeps_open_groups_after_remount", () => {
  const { unmount } = render(<Palette onAdd={() => {}} />);
  fireEvent.click(toggle("날짜"));
  unmount();
  render(<Palette onAdd={() => {}} />);
  expect(toggle("날짜").getAttribute("aria-expanded")).toBe("true");
  expect(nodeButtons()).toHaveLength(3);
});
