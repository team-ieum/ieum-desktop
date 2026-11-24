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
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger={false} />);
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
  render(<Palette onAdd={onAdd} onClose={() => {}} hasTrigger={false} />);
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
  render(<Palette onAdd={onAdd} onClose={() => {}} hasTrigger={false} />);
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
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger={false} />);
  fireEvent.click(cat("GitHub"));
  expect(chips()[0].getAttribute("aria-current")).toBe("true");
  fireEvent.click(chips().find((c) => c.textContent === "PR 8")!);
  expect(scrollTo).toHaveBeenCalled();
  expect(scrollTo.mock.contexts[0]).toBe(screen.getByRole("listbox").closest("[data-scroll]"));
  // jsdom에선 offsetTop이 0 — 음수로 스크롤하지 않는다.
  expect(scrollTo.mock.calls[0][0].top).toBe(0);
});

test("app_triggers_fold_when_canvas_has_trigger", () => {
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger />);
  fireEvent.click(cat("GitHub"));
  expect(rows()).not.toContain("github.new_issue");
  expect(rows()).toHaveLength(21);
  // 접은 줄은 option이 아니라 listbox 밖에 둔다 — 이름엔 개수와 안내 사이 띄어쓰기.
  const fold = () => screen.getByRole("button", { name: /^트리거 2 시작 조건이/ });
  expect(screen.getByRole("listbox").contains(fold())).toBe(false);
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
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger={false} />);
  fireEvent.click(cat("시작 조건"));
  expect(rows()).toEqual(["manual.run", "schedule.hourly", "schedule.daily", "schedule.weekly", "schedule.monthly", "schedule.every"]);
  const note = screen.getByRole("note");
  expect(note.textContent).toContain("앱 트리거는 GitHub·내 PC에 있어요.");
  fireEvent.click(within(note).getByRole("button", { name: "GitHub" }));
  expect(rows()).toEqual(categoryEvents(github));
});

test("local_nodes_show_badge", () => {
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger={false} />);
  fireEvent.click(cat("내 PC"));
  const options = screen.getAllByRole("option");
  expect(options).toHaveLength(4);
  expect(options.every((o) => within(o).queryByText("내 PC"))).toBe(true);
  expect(screen.queryByText("내 PC 노드는 이 PC에서 실행돼요")).toBeNull();
});

const input = () => screen.getByRole("combobox") as HTMLInputElement;
const type = (q: string) => fireEvent.change(input(), { target: { value: q } });
const key = (k: string, init: object = {}) => fireEvent.keyDown(input(), { key: k, ...init });
const option = (event: string) => screen.getByRole("listbox").querySelector<HTMLElement>(`[data-event="${event}"]`)!;

test("palette_search_filters_by_title_and_desc", () => {
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger={false} />);
  expect(document.activeElement).toBe(input());
  expect(input().placeholder).toBe("노드 찾기");
  type("이슈");
  expect(screen.getByText("‘이슈’ 검색 결과")).toBeTruthy();
  const gh = rows().filter((e) => e!.startsWith("github."));
  expect(gh).toEqual(expect.arrayContaining(["github.new_issue", "github.create_issue", "github.list_issues"]));
  expect(screen.getByText(`${rows().length}개 노드를 찾았어요.`)).toBeTruthy();
  // 줄마다 카테고리 칩, 앱 트리거엔 초록 트리거 칩.
  expect(gh.every((e) => within(option(e!)).queryByText("GitHub"))).toBe(true);
  expect(within(option("github.new_issue")).getByText("트리거")).toBeTruthy();
  expect(within(option("github.create_issue")).queryByText("트리거")).toBeNull();
  // 설명에만 있는 말도, 대소문자 없이 찾는다.
  type("  d-DAY ");
  expect(rows()).toEqual(["formatter.date_compare"]);
  expect(within(option("formatter.date_compare")).getByText("데이터 정리")).toBeTruthy();
});

test("palette_search_shows_triggers_even_when_folded", () => {
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger />);
  fireEvent.click(cat("GitHub"));
  type("새 GitHub");
  expect(rows()).toContain("github.new_issue");
  expect(screen.queryByRole("button", { name: /시작 조건이 이미 있어서/ })).toBeNull();
  // 카테고리를 누르면 검색어를 비우고 그 옆 창으로.
  fireEvent.click(cat("내 PC"));
  expect(input().value).toBe("");
  expect(rows()).not.toContain("github.new_issue");
  expect(rows()).toContain("local.file_move");
});

test("palette_arrow_enter_adds_selected", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} onClose={() => {}} hasTrigger={false} />);
  type("글자");
  const found = rows();
  expect(found.length).toBeGreaterThan(1);
  const opts = screen.getAllByRole("option");
  expect(opts[0].getAttribute("aria-selected")).toBe("true");
  expect(opts[1].getAttribute("aria-selected")).toBe("false");
  expect(input().getAttribute("aria-controls")).toBe(screen.getByRole("listbox").id);
  expect(input().getAttribute("aria-activedescendant")).toBe(opts[0].id);
  key("ArrowUp");
  expect(opts[0].getAttribute("aria-selected")).toBe("true");
  key("ArrowDown");
  expect(input().getAttribute("aria-activedescendant")).toBe(opts[1].id);
  expect(Element.prototype.scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
  key("Enter");
  expect(onAdd).toHaveBeenCalledWith(found[1]);

  // 옆 창에선 처음엔 선택 없음 — 마우스를 올린 줄이 선택, 접힌 줄은 건너뛴다.
  fireEvent.click(cat("시작 조건"));
  expect(input().getAttribute("aria-activedescendant")).toBeNull();
  key("Enter");
  expect(onAdd).toHaveBeenCalledTimes(1);
  fireEvent.mouseEnter(option("schedule.monthly"));
  key("ArrowDown");
  key("ArrowDown");
  key("Enter");
  expect(onAdd).toHaveBeenLastCalledWith("schedule.every");
});

test("palette_arrow_skips_folded_rows", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} onClose={() => {}} hasTrigger />);
  fireEvent.click(cat("내 PC"));
  key("ArrowDown");
  key("ArrowUp");
  key("Enter");
  expect(onAdd).toHaveBeenLastCalledWith("local.file_move");
  // 맨 아래 접힌 트리거(local.folder_new_file)로는 내려가지 않고 마지막 보이는 줄에서 멈춘다.
  for (let i = 0; i < 5; i++) key("ArrowDown");
  key("Enter");
  expect(onAdd).toHaveBeenLastCalledWith("local.file_trash");
});

test("palette_enter_with_no_results_does_nothing", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} onClose={() => {}} hasTrigger={false} />);
  type("없는말zz");
  key("ArrowDown");
  key("Enter");
  expect(onAdd).not.toHaveBeenCalled();
  expect(screen.getByText("맞는 노드가 없어요.")).toBeTruthy();
  expect(screen.getByText("다른 말로 찾아보세요. 예: 이슈, 날짜, 파일")).toBeTruthy();
  expect(input().getAttribute("aria-activedescendant")).toBeNull();
});

test("palette_escape_steps", () => {
  const onClose = vi.fn();
  const outside = vi.fn();
  document.addEventListener("keydown", outside);
  render(<Palette onAdd={() => {}} onClose={onClose} hasTrigger={false} />);
  type("이슈");
  key("Escape");
  expect(input().value).toBe("");
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.click(cat("GitHub"));
  expect(screen.getByRole("listbox")).toBeTruthy();
  key("Escape");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  key("Escape");
  expect(onClose).toHaveBeenCalledTimes(1);
  // 바깥(document)의 Esc 처리는 한 번도 불리지 않는다.
  expect(outside).not.toHaveBeenCalled();
  document.removeEventListener("keydown", outside);
});

test("palette_escape_from_any_focus_inside", () => {
  const onClose = vi.fn();
  const outside = vi.fn();
  document.addEventListener("keydown", outside);
  render(<Palette onAdd={() => {}} onClose={onClose} hasTrigger={false} />);
  // Tab 등으로 포커스가 검색 칸을 떠나 카테고리 버튼에 있어도 Esc는 팔레트가 받는다.
  fireEvent.click(cat("GitHub"));
  cat("GitHub").focus();
  expect(document.activeElement).toBe(cat("GitHub"));
  fireEvent.keyDown(cat("GitHub"), { key: "Escape" });
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.keyDown(cat("GitHub"), { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(outside).not.toHaveBeenCalled();
  document.removeEventListener("keydown", outside);
});

test("palette_ignores_keys_while_composing", () => {
  const onAdd = vi.fn();
  const onClose = vi.fn();
  render(<Palette onAdd={onAdd} onClose={onClose} hasTrigger={false} />);
  type("이슈");
  key("Escape", { isComposing: true });
  key("Enter", { isComposing: true });
  expect(input().value).toBe("이슈");
  expect(onAdd).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});

test("palette_keeps_focus_in_search_after_click", () => {
  const kept = (el: Element) => expect(fireEvent.mouseDown(el)).toBe(false);
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger />);
  // 검색 칸 자체는 막지 않는다 — 커서를 옮길 수 있어야 한다.
  expect(fireEvent.mouseDown(input())).toBe(true);
  kept(cat("GitHub"));
  fireEvent.click(cat("GitHub"));
  kept(chips()[0]);
  kept(option("github.create_issue"));
  kept(screen.getByRole("button", { name: /시작 조건이 이미 있어서/ }));
  fireEvent.click(cat("시작 조건"));
  kept(within(screen.getByRole("note")).getByRole("button", { name: "GitHub" }));
  type("이슈");
  kept(screen.getByRole("button", { name: "검색어 지우기" }));
});

test("palette_folding_clears_selection_of_hidden_row", () => {
  const onAdd = vi.fn();
  render(<Palette onAdd={onAdd} onClose={() => {}} hasTrigger />);
  fireEvent.click(cat("GitHub"));
  const fold = () => screen.getByRole("button", { name: /^트리거 2 시작 조건이/ });
  fireEvent.click(fold());
  fireEvent.mouseEnter(option("github.new_issue"));
  expect(input().getAttribute("aria-activedescendant")).toBe(option("github.new_issue").id);
  // 다시 접으면 사라진 줄은 선택이 아니다 — Enter로 숨은 트리거가 추가되지 않는다.
  fireEvent.click(fold());
  expect(input().getAttribute("aria-activedescendant")).toBeNull();
  key("Enter");
  expect(onAdd).not.toHaveBeenCalled();
  // ↓는 보이는 첫 줄부터.
  key("ArrowDown");
  key("Enter");
  expect(onAdd).toHaveBeenLastCalledWith("github.create_issue");
});

test("palette_search_head_keeps_dollar_patterns", () => {
  render(<Palette onAdd={() => {}} onClose={() => {}} hasTrigger={false} />);
  type("a$&b$$");
  expect(screen.getByText("‘a$&b$$’ 검색 결과")).toBeTruthy();
});
