// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { setLang } from "../i18n";
import { Toolbar } from "./Toolbar";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

const props = (over: Partial<Parameters<typeof Toolbar>[0]> = {}) => ({
  name: "흐름",
  onName: vi.fn(),
  dirty: false,
  enabled: true,
  onEnabled: vi.fn(),
  problems: 0,
  onBack: vi.fn(),
  onOpenRuns: vi.fn(),
  onSave: vi.fn(),
  onRun: vi.fn(),
  running: false,
  saving: false,
  ...over,
});

test("toolbar_validation_pill_states", () => {
  const r = render(<Toolbar {...props()} />);
  expect(screen.getByText("문제 없음")).toBeTruthy();
  r.rerender(<Toolbar {...props({ problems: 2 })} />);
  expect(screen.getByText("문제 2")).toBeTruthy();
  expect(screen.queryByText("문제 없음")).toBeNull();
});

test("toolbar_run_disabled_when_dirty", () => {
  const r = render(<Toolbar {...props()} />);
  expect((screen.getByRole("button", { name: "실행" }) as HTMLButtonElement).disabled).toBe(false);
  r.rerender(<Toolbar {...props({ dirty: true })} />);
  const run = screen.getByRole("button", { name: "실행" }) as HTMLButtonElement;
  expect(run.disabled).toBe(true);
  expect(run.title).toBe("먼저 저장해야 실행할 수 있어요.");
  expect(screen.getByRole("status", { name: "저장 안 된 변경" })).toBeTruthy();
});
