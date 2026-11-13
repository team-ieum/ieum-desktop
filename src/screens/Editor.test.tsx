// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api } from "../api";
import { nodeTitle } from "../editor/Canvas";
import { setLang } from "../i18n";
import { PALETTE } from "../nodes/defs";
import { Editor } from "./Editor";

beforeAll(() => {
  setLang("ko");
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  // @ts-expect-error jsdom에 없음
  globalThis.DOMMatrixReadOnly = class {
    m22 = 1;
  };
  vi.spyOn(api, "validateWorkflow").mockResolvedValue([]);
});
afterEach(cleanup);

const open = () => act(async () => render(<Editor id={null} onBack={() => {}} onOpenRuns={() => {}} />));
const palette = () => screen.queryByRole("dialog", { name: "노드 추가" });
const addNode = async (event: string) => {
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "노드 추가" })));
  await act(async () => fireEvent.click(within(palette()!).getAllByRole("button", { name: new RegExp(`^${nodeTitle(event)}`) })[0]));
};

test("editor_palette_opens_from_plus_button", async () => {
  const r = await open();
  // 왼쪽 고정 팔레트 없음 — [+]를 눌러야 뜬다.
  expect(palette()).toBeNull();
  const plus = screen.getByRole("button", { name: "노드 추가" });
  await act(async () => fireEvent.click(plus));
  expect(plus.getAttribute("aria-expanded")).toBe("true");
  expect(palette()).toBeTruthy();
  // Esc로 닫힌다.
  await act(async () => fireEvent.keyDown(document, { key: "Escape" }));
  expect(palette()).toBeNull();
  // 바깥을 누르면 닫힌다.
  await act(async () => fireEvent.click(plus));
  await act(async () => fireEvent.mouseDown(document.body));
  expect(palette()).toBeNull();
  // 노드를 고르면 추가되고 닫힌다.
  await addNode(PALETTE[0]);
  expect(palette()).toBeNull();
  expect(r.container.querySelectorAll(".react-flow__node")).toHaveLength(1);
});

test("editor_config_panel_only_when_node_selected", async () => {
  const r = await open();
  expect(screen.queryByRole("complementary")).toBeNull();
  expect(screen.queryByLabelText("노드 이름")).toBeNull();
  await addNode(PALETTE[0]);
  // 새 노드가 골라져 설정 패널이 뜬다 — 머리에 이름·종류, 아래 테스트 칸.
  const panel = screen.getByRole("complementary", { name: nodeTitle(PALETTE[0]) });
  expect(within(panel).getByLabelText("노드 이름")).toBeTruthy();
  expect(within(panel).getByRole("heading", { name: "테스트" })).toBeTruthy();
  // [X]로 닫힌다.
  await act(async () => fireEvent.click(within(panel).getByRole("button", { name: "패널 닫기" })));
  expect(screen.queryByRole("complementary")).toBeNull();
  // 노드를 다시 누르면 열리고 Esc로 닫힌다.
  await act(async () => fireEvent.click(r.container.querySelector(".react-flow__node")!));
  expect(screen.getByRole("complementary")).toBeTruthy();
  await act(async () => fireEvent.keyDown(document, { key: "Escape" }));
  expect(screen.queryByRole("complementary")).toBeNull();
});

test("editor_panel_floats_over_canvas", async () => {
  const r = await open();
  const canvas = r.container.querySelector(".react-flow")!.parentElement!;
  await addNode(PALETTE[0]);
  const panel = screen.getByRole("complementary", { name: nodeTitle(PALETTE[0]) });
  // 캔버스 위에 떠 있는 카드 — 캔버스는 뒤에서 전체 폭 그대로.
  expect(panel.className).toMatch(/\babsolute\b/);
  expect(panel.className).toContain("rounded-2xl");
  expect(panel.className).toContain("shadow-");
  expect(r.container.querySelector(".react-flow")!.parentElement).toBe(canvas);
  expect(canvas.className).toContain("inset-0");
  expect(panel.parentElement!.contains(canvas)).toBe(true);
});

test("tech_info_toggle_shows_event_ids", async () => {
  await open();
  await addNode(PALETTE[0]);
  await addNode("local.ai");
  await act(async () =>
    fireEvent.change(within(screen.getByTestId("field-provider")).getByRole("combobox"), { target: { value: "api:anthropic" } }),
  );
  expect(screen.queryByText(PALETTE[0])).toBeNull();
  const sw = screen.getByRole("switch", { name: "기술 정보" });
  await act(async () => fireEvent.click(sw));
  expect(sw.getAttribute("aria-checked")).toBe("true");
  expect(screen.getByText(PALETTE[0])).toBeTruthy();
  expect(screen.getByText("local.ai")).toBeTruthy();
  // AI 노드는 공급원·모델 칸도.
  expect(screen.getByText("공급원")).toBeTruthy();
  expect(screen.getByText("api:anthropic")).toBeTruthy();
  await act(async () => fireEvent.click(sw));
  expect(screen.queryByText(PALETTE[0])).toBeNull();
});
