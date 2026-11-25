// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test } from "vitest";
import { setLang } from "../i18n";
import { OutputView, type OutputField } from "./OutputView";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

const fields: OutputField[] = [
  { key: "found", title: "찾음" },
  { key: "total", title: "개수" },
  { key: "items", title: "파일", children: [{ key: "filename", title: "파일 이름" }] },
];

test("output_view_labels_from_fields", () => {
  render(<OutputView output={{ found: true, total: 2, head_sha: "f9a2", extra: null }} fields={fields} />);
  expect(screen.getByText("찾음")).toBeTruthy();
  expect(screen.getByText("true").className).toContain("text-[#8a3ab9]");
  expect(screen.getByText("개수")).toBeTruthy();
  expect(screen.getByText("2").className).toContain("text-[#1a5fb4]");
  // 정의에 없는 키는 키 이름 그대로, 글은 따옴표.
  expect(screen.getByText("head_sha")).toBeTruthy();
  expect(screen.getByText("f9a2").textContent).toBe('"f9a2"');
  expect(screen.getByText("extra")).toBeTruthy();
});

test("collapses_lists_and_objects", () => {
  const output = {
    items: [
      { filename: "live/app.lock", status: "added", additions: 1 },
      { filename: "live/sample.txt", status: "added", additions: 4 },
    ],
    meta: { a: 1, b: 2, c: 3 },
  };
  render(<OutputView output={output} fields={fields} />);
  const items = screen.getByRole("button", { name: /파일/ });
  expect(items.getAttribute("aria-expanded")).toBe("false");
  expect(screen.getByText("2개")).toBeTruthy();
  expect(screen.getByText("3개 칸")).toBeTruthy();
  expect(screen.queryByText("live/app.lock · added")).toBeNull();

  fireEvent.click(items);
  expect(items.getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByText("live/app.lock · added")).toBeTruthy();
  expect(screen.getByText("live/sample.txt · added")).toBeTruthy();
  const first = screen.getByRole("button", { name: /^▸1/ });
  fireEvent.click(first);
  // 항목 안 칸 이름은 목록 칸의 children 제목.
  expect(screen.getByText("파일 이름")).toBeTruthy();
  expect(screen.getByText("live/app.lock")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: /meta/ }));
  expect(screen.getByText("a")).toBeTruthy();
  expect(screen.getByText("3")).toBeTruthy();
});

test("long_text_clamps_with_more", () => {
  const diff = ["--- a", "@@ -0,0 +1,4 @@", "+line one", "+line two", "+line three", "+한글 줄"].join("\n");
  render(<OutputView output={{ diff }} />);
  const value = screen.getByText(/line two/);
  expect(value.textContent).toContain("+line two");
  expect(value.textContent).not.toContain("line three");
  fireEvent.click(screen.getByRole("button", { name: "전체 보기" }));
  expect(screen.getByText(/한글 줄/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "전체 보기" })).toBeNull();
});

test("json_tab_shows_raw", () => {
  const output = { found: true, items: [{ a: "x" }] };
  render(<OutputView output={output} />);
  expect(screen.getByRole("tab", { name: "목록" }).getAttribute("aria-selected")).toBe("true");
  fireEvent.click(screen.getByRole("tab", { name: "JSON" }));
  expect(screen.getByRole("tab", { name: "JSON" }).getAttribute("aria-selected")).toBe("true");
  expect(document.querySelector("pre")!.textContent).toBe(JSON.stringify(output, null, 2));
  fireEvent.click(screen.getByRole("tab", { name: "목록" }));
  expect(document.querySelector("pre")).toBeNull();
});

test("output_view_large_values", () => {
  let deep: Record<string, unknown> = { leaf: 1 };
  for (let i = 0; i < 8; i++) deep = { [`d${i}`]: deep };
  const list = Array.from({ length: 1000 }, (_, i) => ({ name: `item${i}`, kind: "k" }));
  const big = "가".repeat(200_000);
  const { container } = render(<OutputView output={{ deep, list, big }} />);
  // 첫 렌더는 접힌 채 — 최상위 세 줄만, 큰 글은 잘린 채.
  for (const name of [/deep/, /list/]) expect(screen.getByRole("button", { name }).getAttribute("aria-expanded")).toBe("false");
  expect(screen.getByText("1000개")).toBeTruthy();
  expect(screen.queryByText("item0 · k")).toBeNull();
  expect(screen.getByRole("button", { name: "전체 보기" })).toBeTruthy();
  expect(container.textContent!.length).toBeLessThan(2000);
});
