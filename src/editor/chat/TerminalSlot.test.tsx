// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type TermTool } from "../../api";
import { setLang } from "../../i18n";
import { TerminalSlot, TOOL_KEY } from "./TerminalSlot";

const term = { open: vi.fn(), write: vi.fn(), loadAddon: vi.fn(), dispose: vi.fn(), cols: 80, rows: 24, onData: vi.fn() };
// `new`로 부르므로 화살표 함수가 아니라 function이어야 한다.
vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn(function () {
    return term;
  }),
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn(function () {
    return { fit: vi.fn() };
  }),
}));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

// 이 환경의 localStorage는 온전하지 않다 — 메모리 저장소를 꽂는다(AssistantPanel.test.tsx와 같은 방식).
const memoryStorage = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};
let exit: (p: { workflowId: string }) => void = () => {};

beforeEach(() => {
  setLang("ko");
  vi.clearAllMocks();
  vi.stubGlobal("localStorage", memoryStorage());
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(api, "termSpawn").mockResolvedValue({ tool: "codex", attached: false, skillInstalled: false });
  vi.spyOn(api, "termWrite").mockResolvedValue(undefined);
  vi.spyOn(api, "termResize").mockResolvedValue(undefined);
  vi.spyOn(api, "termKill").mockResolvedValue(undefined);
  vi.spyOn(api, "onTermExit").mockImplementation(async (cb) => {
    exit = cb;
    return () => {};
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const tools = (tools: TermTool[], running: TermTool | null = null) =>
  vi.spyOn(api, "termTools").mockResolvedValue({ tools, running });
/** [시작]을 누르고 세션이 뜰 때까지(동적 import → termSpawn → 입력 잇기) 기다린다. */
const clickStart = async () => {
  const button = await screen.findByRole("button", { name: "시작" }); // act 밖에서 찾는다 — 안에서는 화면이 갱신되지 않는다
  await act(async () => fireEvent.click(button));
};
const start = async () => {
  await clickStart();
  await waitFor(() => expect(term.onData).toHaveBeenCalled());
};

test("terminal_lists_only_usable_tools_and_remembers_last", async () => {
  tools(["claude", "codex"]);
  localStorage.setItem(TOOL_KEY, "codex");
  render(<TerminalSlot workflowId="w1" visible />);
  const sel = (await screen.findByLabelText("CLI")) as HTMLSelectElement;
  expect([...sel.options].map((o) => o.value)).toEqual(["claude", "codex"]); // agy 없음
  expect(sel.value).toBe("codex");
  await start();
  expect(api.termSpawn).toHaveBeenCalledWith("w1", "codex", 80, 24, expect.any(Function)); // 도구 이름만 넘긴다
  expect(localStorage.getItem(TOOL_KEY)).toBe("codex");
});

test("terminal_does_nothing_until_visible", async () => {
  tools(["claude"]);
  render(<TerminalSlot workflowId="w1" visible={false} />);
  await act(async () => {});
  expect(api.termTools).not.toHaveBeenCalled();
});

test("terminal_reattaches_running_session_without_asking", async () => {
  tools(["claude"], "claude");
  render(<TerminalSlot workflowId="w1" visible />);
  await waitFor(() => expect(api.termSpawn).toHaveBeenCalledWith("w1", "claude", 80, 24, expect.any(Function)));
  expect(screen.queryByRole("button", { name: "시작" })).toBeNull();
});

test("terminal_survives_hiding_and_unmount_ends_only_on_stop", async () => {
  tools(["claude"]);
  const r = render(<TerminalSlot workflowId="w1" visible />);
  await start();
  r.rerender(<TerminalSlot workflowId="w1" visible={false} />);
  expect(term.dispose).not.toHaveBeenCalled(); // 숨김 = xterm 그대로
  r.rerender(<TerminalSlot workflowId="w1" visible />);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "세션 종료" })));
  expect(api.termKill).toHaveBeenCalledWith("w1");
  vi.mocked(api.termKill).mockClear();
  r.unmount();
  expect(api.termKill).not.toHaveBeenCalled(); // 화면을 떠나도 세션은 산다
});

test("terminal_forwards_keys_and_shows_end", async () => {
  tools(["claude"]);
  render(<TerminalSlot workflowId="w1" visible />);
  await start();
  act(() => term.onData.mock.calls[0][0]("ls\r"));
  expect(api.termWrite).toHaveBeenCalledWith("w1", "ls\r");
  await act(async () => exit({ workflowId: "other" }));
  expect(screen.queryByText("세션이 끝났어요.")).toBeNull(); // 다른 워크플로우의 종료는 무시
  await act(async () => exit({ workflowId: "w1" }));
  expect(screen.getByText("세션이 끝났어요.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "시작" })).toBeTruthy(); // 다시 띄울 수 있다
  expect(term.dispose).toHaveBeenCalled();
});

test("terminal_announces_skill_install_once", async () => {
  tools(["claude"]);
  vi.mocked(api.termSpawn).mockResolvedValue({ tool: "claude", attached: false, skillInstalled: true });
  render(<TerminalSlot workflowId="w1" visible />);
  await start();
  expect(screen.getByText("IEUM 스킬을 설치했어요 (~/.agents/skills/ieum).")).toBeTruthy();
});

test("terminal_shows_why_it_could_not_open", async () => {
  tools(["claude"]);
  vi.mocked(api.termSpawn).mockRejectedValue("이 워크플로우의 터미널이 이미 열려 있어요.");
  render(<TerminalSlot workflowId="w1" visible />);
  await clickStart();
  expect(await screen.findByText(/터미널을 열지 못했어요\..*이미 열려 있어요/)).toBeTruthy();
  expect(term.dispose).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "시작" })).toBeTruthy();
});
