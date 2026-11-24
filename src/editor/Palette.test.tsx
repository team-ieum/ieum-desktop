// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { setLang } from "../i18n";
import { Palette } from "./Palette";
import { CATEGORIES, categoryEvents } from "./paletteGroups";

// jsdom엔 스크롤 함수가 없다 — 칩이 스크롤을 부르는지 보려고 이 파일에서만 채운다.
const scrollTo = vi.fn();
Element.prototype.scrollTo = scrollTo;
Element.prototype.scrollIntoView = vi.fn();

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const cat = (name: string) => screen.getByRole("button", { name });
const rows = () => Array.from(screen.getByRole("listbox").querySelectorAll("[data-event]")).map((r) => r.getAttribute("data-event"));
const chips = () => within(screen.getByRole("navigation", { name: "소제목으로 바로 가기" })).getAllByRole("button");
const github = CATEGORIES.find((c) => c.id === "github")!;

test("palette_lists_categories_in_two_zones", () => {
  render(<Palette onAdd={() => {}} hasTrigger={false} />);
  const names = ["시작 조건", "AI", "흐름", "데이터 정리", "HTTP", "GitHub", "내 PC"];
  // 처음엔 카테고리 버튼만 있다 — 이름으로 찾은 버튼이 화면 순서대로인지 본다.
  const all = screen.getAllByRole("button");
  expect(names.map((n) => all.indexOf(cat(n)))).toEqual([0, 1, 2, 3, 4, 5, 6]);
  expect(all).toHaveLength(7);
  expect(screen.getByText("기본")).toBeTruthy();
  expect(screen.getByText("앱·내 PC")).toBeTruthy();
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("single_node_category_adds_directly", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} hasTrigger={false} />);
  fireEvent.click(cat("AI"));
  expect(onAdd).toHaveBeenLastCalledWith("local.ai");
  expect(screen.queryByRole("listbox")).toBeNull();
  fireEvent.click(cat("흐름"));
  expect(onAdd).toHaveBeenLastCalledWith("branch.if");
  fireEvent.click(cat("HTTP"));
  expect(onAdd).toHaveBeenLastCalledWith("http.request");
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("github_flyout_splits_trigger_and_actions", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} hasTrigger={false} />);
  fireEvent.click(cat("GitHub"));
  expect(cat("GitHub").getAttribute("aria-expanded")).toBe("true");
  expect(rows()).toEqual(categoryEvents(github));
  expect(screen.getByRole("listbox").querySelector("[data-sec]")?.textContent).toContain("트리거");
  expect(chips().map((c) => c.textContent)).toEqual(["트리거 2", "이슈 7", "PR 8", "조회 6"]);
  expect(screen.getAllByRole("option")).toHaveLength(23);
  fireEvent.click(screen.getByRole("listbox").querySelector('[data-event="github.create_issue"]')!);
  expect(onAdd).toHaveBeenCalledWith("github.create_issue");
  fireEvent.click(cat("데이터 정리"));
  expect(rows()[0]).toBe("transform.map");
  expect(rows()).not.toContain("github.create_issue");
});

test("chip_scrolls_to_section", () => {
  render(<Palette onAdd={() => {}} hasTrigger={false} />);
  fireEvent.click(cat("GitHub"));
  expect(chips()[0].getAttribute("aria-current")).toBe("true");
  fireEvent.click(chips().find((c) => c.textContent === "PR 8")!);
  expect(scrollTo).toHaveBeenCalled();
  expect(scrollTo.mock.contexts[0]).toBe(screen.getByRole("listbox").closest("[data-scroll]"));
});

test("app_triggers_fold_when_canvas_has_trigger", () => {
  render(<Palette onAdd={() => {}} hasTrigger />);
  fireEvent.click(cat("GitHub"));
  expect(rows()).not.toContain("github.new_issue");
  expect(rows()).toHaveLength(21);
  const fold = () => within(screen.getByRole("listbox")).getByRole("button", { name: /^트리거 2/ });
  expect(fold().getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(fold());
  expect(fold().getAttribute("aria-expanded")).toBe("true");
  expect(rows().slice(-2)).toEqual(["github.new_issue", "github.new_pull_request"]);

  // 접힌 칩은 맨 끝 — 누르면 펼치고 스크롤한다.
  fireEvent.click(cat("내 PC"));
  fireEvent.click(cat("GitHub"));
  expect(rows()).not.toContain("github.new_issue");
  expect(chips().map((c) => c.textContent)).toEqual(["이슈 7", "PR 8", "조회 6", "트리거 2"]);
  fireEvent.click(chips()[3]);
  expect(rows()).toContain("github.new_issue");
  expect(scrollTo).toHaveBeenCalled();

  // 시작 조건은 접지 않는다.
  fireEvent.click(cat("시작 조건"));
  expect(rows()).toHaveLength(6);
});

test("start_flyout_links_to_app_triggers", () => {
  render(<Palette onAdd={() => {}} hasTrigger={false} />);
  fireEvent.click(cat("시작 조건"));
  expect(rows()).toEqual(["manual.run", "schedule.hourly", "schedule.daily", "schedule.weekly", "schedule.monthly", "schedule.every"]);
  const note = screen.getByRole("note");
  expect(note.textContent).toContain("앱 트리거는 GitHub·내 PC에 있어요.");
  fireEvent.click(within(note).getByRole("button", { name: "GitHub" }));
  expect(rows()).toEqual(categoryEvents(github));
});

test("local_nodes_show_badge", () => {
  render(<Palette onAdd={() => {}} hasTrigger={false} />);
  fireEvent.click(cat("내 PC"));
  const options = screen.getAllByRole("option");
  expect(options).toHaveLength(4);
  expect(options.every((o) => within(o).queryByText("내 PC"))).toBe(true);
  expect(screen.queryByText("내 PC 노드는 이 PC에서 실행돼요")).toBeNull();
});
