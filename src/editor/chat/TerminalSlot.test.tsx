// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type TermCliState, type TermTool } from "../../api";
import { setLang } from "../../i18n";
import { NOTE_MS, TerminalSlot, TOOL_KEY } from "./TerminalSlot";

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

const ALL: TermTool[] = ["claude", "codex", "agy"];
/** `tools` = 고를 수 있는 것. 나머지는 설치 안 됨으로 둔다(`states`로 덮을 수 있다). */
const tools = (tools: TermTool[], running: TermTool | null = null, states: Partial<Record<TermTool, TermCliState>> = {}) =>
  vi.spyOn(api, "termTools").mockResolvedValue({
    tools,
    clis: ALL.map((tool) => ({ tool, state: states[tool] ?? (tools.includes(tool) ? "ready" : "notInstalled") })),
    running,
  });
const NAME: Record<TermTool, string> = { claude: "Claude Code", codex: "Codex", agy: "Antigravity" };
/** CLI 카드를 누르고 세션이 뜰 때까지(동적 import → termSpawn → 입력 잇기) 기다린다. */
const clickStart = async (tool: TermTool = "claude") => {
  const button = await screen.findByRole("button", { name: `${NAME[tool]} 시작` }); // act 밖에서 찾는다 — 안에서는 화면이 갱신되지 않는다
  await act(async () => fireEvent.click(button));
};
const start = async (tool: TermTool = "claude") => {
  await clickStart(tool);
  await waitFor(() => expect(term.onData).toHaveBeenCalled());
};

test("terminal_shows_three_cli_cards_with_state_and_only_usable_ones_start", async () => {
  tools(["claude", "codex"], null, { agy: "loggedOut" });
  localStorage.setItem(TOOL_KEY, "codex");
  render(<TerminalSlot workflowId="w1" visible />);
  const codex = await screen.findByRole("button", { name: "Codex 시작" });
  expect(codex.textContent).toContain("지난번"); // 지난번 고른 것을 강조
  expect(codex.className).toContain("border-main-blue");
  expect(screen.getByRole("button", { name: "Claude Code 시작" }).textContent).toContain("로그인됨");
  const agy = screen.getByRole("button", { name: "Antigravity 시작" }) as HTMLButtonElement;
  expect([agy.disabled, agy.textContent?.includes("로그인 필요")]).toEqual([true, true]);
  await act(async () => fireEvent.click(agy));
  expect(api.termSpawn).not.toHaveBeenCalled();
  await start("codex");
  expect(api.termSpawn).toHaveBeenCalledWith("w1", "codex", 80, 24, expect.any(Function)); // 도구 이름만 넘긴다
  expect(localStorage.getItem(TOOL_KEY)).toBe("codex");
});

test("terminal_marks_not_installed_and_explains_antigravity_sandbox", async () => {
  tools(["agy"], null, { claude: "notInstalled", codex: "unknown" });
  render(<TerminalSlot workflowId="w1" visible />);
  const claude = (await screen.findByRole("button", { name: "Claude Code 시작" })) as HTMLButtonElement;
  expect([claude.disabled, claude.textContent?.includes("설치 안 됨")]).toEqual([true, true]);
  // 확인 불가는 숨기지 않고 고를 수 있다.
  expect((screen.getByRole("button", { name: "Codex 시작" }) as HTMLButtonElement).disabled).toBe(false);
  expect(screen.getByRole("button", { name: "Antigravity 시작" }).textContent).toContain("샌드박스");
});

test("terminal_says_so_when_no_cli_is_usable", async () => {
  tools([]);
  render(<TerminalSlot workflowId="w1" visible />);
  expect(await screen.findByText(/쓸 수 있는 CLI가 없어요/)).toBeTruthy();
});

test("terminal_does_nothing_until_visible", async () => {
  tools(["claude"]);
  render(<TerminalSlot workflowId="w1" visible={false} />);
  await act(async () => {});
  expect(api.termTools).not.toHaveBeenCalled();
});

test("terminal_reattaches_running_session_without_asking", async () => {
  tools(["claude"], "claude");
  vi.mocked(api.termSpawn).mockResolvedValue({ tool: "claude", attached: true, skillInstalled: false });
  render(<TerminalSlot workflowId="w1" visible />);
  await waitFor(() => expect(api.termSpawn).toHaveBeenCalledWith("w1", "claude", 80, 24, expect.any(Function)));
  expect(screen.queryByRole("button", { name: "Claude Code 시작" })).toBeNull();
  expect(screen.getByText("Claude Code")).toBeTruthy(); // 실행 줄의 CLI 칩
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
  expect(screen.getByRole("button", { name: "Claude Code 시작" })).toBeTruthy(); // 다시 띄울 수 있다
  expect(term.dispose).toHaveBeenCalled();
});

test("terminal_announces_skill_install_once", async () => {
  tools(["claude"]);
  vi.mocked(api.termSpawn).mockResolvedValue({ tool: "claude", attached: false, skillInstalled: true });
  render(<TerminalSlot workflowId="w1" visible />);
  await start();
  // 늘 자리를 차지하지 않는다 — 잠깐 뜨는 알림이고 닫을 수 있다.
  expect(screen.getByRole("status").textContent).toContain("IEUM 스킬을 설치했어요 (~/.agents/skills/ieum).");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "알림 닫기" })));
  expect(screen.queryByRole("status")).toBeNull();
});

test("skill_note_goes_away_by_itself", async () => {
  tools(["claude"]);
  vi.mocked(api.termSpawn).mockResolvedValue({ tool: "claude", attached: false, skillInstalled: true });
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    render(<TerminalSlot workflowId="w1" visible />);
    await start();
    expect(screen.getByRole("status")).toBeTruthy();
    await act(async () => vi.advanceTimersByTime(NOTE_MS));
    expect(screen.queryByRole("status")).toBeNull();
  } finally {
    vi.useRealTimers();
  }
});

test("terminal_shows_why_it_could_not_open", async () => {
  tools(["claude"]);
  vi.mocked(api.termSpawn).mockRejectedValue("이 워크플로우의 터미널이 이미 열려 있어요.");
  render(<TerminalSlot workflowId="w1" visible />);
  await clickStart();
  expect(await screen.findByText(/터미널을 열지 못했어요\..*이미 열려 있어요/)).toBeTruthy();
  expect(term.dispose).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Claude Code 시작" })).toBeTruthy();
});
