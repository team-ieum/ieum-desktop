// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import App from "./App";
import { api } from "./api";
import { useWeb } from "./stores/web";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

const ACCOUNT = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  // @ts-expect-error jsdom에 없음
  globalThis.DOMMatrixReadOnly = class {
    m22 = 1;
  };
  vi.spyOn(api, "getSettings").mockResolvedValue({ language: "ko", autostart: false, firstRunDone: true, serverUrl: "", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true });
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "onOpenTarget").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "validateWorkflow").mockResolvedValue([]);
  vi.spyOn(api, "currentAccount").mockResolvedValue(ACCOUNT);
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "dashboardSummary").mockRejectedValue({ code: "NETWORK" });
  vi.spyOn(api, "dashboardExecutions").mockRejectedValue({ code: "NETWORK" });
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [], apiKeys: [] });
  vi.spyOn(api, "listCredentials").mockResolvedValue({ apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [], codexAccounts: [], keychainError: null });
});
afterEach(() => {
  cleanup();
  useWeb.setState({ expired: false, relogin: false, offline: false });
});

test("leaving_editor_with_unsaved_changes_asks_in_app", async () => {
  await act(async () => {
    render(<App />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "내 PC" })));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "새 워크플로우" }));
  });
  const history = screen.getByRole("button", { name: "실행 기록" });

  // 고치기 전에는 묻지 않고 바로 간다 — 다시 편집기로.
  await act(async () => fireEvent.click(history));
  expect(screen.queryByRole("alert")).toBeNull();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "내 PC" })));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "새 워크플로우" })));

  await act(async () => fireEvent.change(screen.getByLabelText("워크플로우 이름"), { target: { value: "고침" } }));

  // 메뉴 이동: 취소하면 편집기와 고친 내용이 남는다.
  await act(async () => fireEvent.click(history));
  expect(screen.getByRole("alert").textContent).toContain("저장하지 않은 변경");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "취소" })));
  expect(screen.queryByRole("alert")).toBeNull();
  expect((screen.getByLabelText("워크플로우 이름") as HTMLInputElement).value).toBe("고침");

  // 뒤로: 버리기를 눌러야 목록으로 간다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "뒤로" })));
  expect(screen.getByLabelText("워크플로우 이름")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "버리기" })));
  expect(screen.queryByLabelText("워크플로우 이름")).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

test("shell_shows_header_crumb_and_new_canvas_opens_editor", async () => {
  await act(async () => {
    render(<App />);
  });
  // 위쪽 줄: 로고 글자·현재 위치·계정 첫 글자
  expect(screen.getByText("IEUM")).toBeTruthy();
  // 첫 화면 = 대시보드
  expect(screen.getByTestId("crumb").textContent).toBe("대시보드");
  expect(screen.getByLabelText(ACCOUNT.email).textContent).toBe("M");
  expect(await screen.findByText(/v0\.2\.0/)).toBeTruthy();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: /새 캔버스/ })));
  expect(screen.getByLabelText("워크플로우 이름")).toBeTruthy();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: "대시보드" })));
  expect(screen.getByTestId("crumb").textContent).toBe("대시보드");
  expect(screen.getByRole("heading", { name: "대시보드" })).toBeTruthy();
});

test("gate_shows_auth_when_no_account", async () => {
  vi.mocked(api.currentAccount).mockResolvedValueOnce(null);
  await act(async () => {
    render(<App />);
  });
  expect(screen.getByRole("button", { name: "로그인" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "대시보드" })).toBeNull();
});

test("offline_start_opens_app_without_gate", async () => {
  vi.mocked(api.webListWorkflows).mockRejectedValueOnce({ code: "NETWORK" });
  await act(async () => {
    render(<App />);
  });
  expect(screen.getByRole("button", { name: "대시보드" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "로그인" })).toBeNull();
  expect(useWeb.getState().offline).toBe(true);
});

test("auth_expired_keeps_app_and_relogin_from_account_settings", async () => {
  vi.mocked(api.webListWorkflows).mockRejectedValueOnce({ code: "AUTH_EXPIRED" });
  await act(async () => {
    render(<App />);
  });
  // 만료돼도 앱은 그대로 — 관문으로 보내지 않는다.
  expect(screen.queryByRole("button", { name: "로그인" })).toBeNull();
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "설정" })[0]));
  expect(screen.getByText("이음 클라우드를 쓰려면 다시 로그인하세요.")).toBeTruthy();

  // [다시 로그인] → 이메일이 채워진 관문, [뒤로]면 앱으로.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "다시 로그인" })));
  expect((screen.getByLabelText("이메일") as HTMLInputElement).value).toBe(ACCOUNT.email);
  expect(screen.getByText("로그인이 만료됐어요. 다시 로그인해 주세요.")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "뒤로" })));
  expect(screen.queryByLabelText("이메일")).toBeNull();
});

test("gate_waits_for_account_before_rendering", async () => {
  let resolve!: (a: null) => void;
  vi.mocked(api.currentAccount).mockReturnValueOnce(new Promise((r) => (resolve = r)));
  await act(async () => {
    render(<App />);
  });
  // 계정 응답 전에는 관문도 셸도 그리지 않는다(깜빡임 없음).
  expect(screen.queryByRole("button", { name: "로그인" })).toBeNull();
  expect(screen.queryByRole("button", { name: "대시보드" })).toBeNull();
  await act(async () => resolve(null));
  expect(screen.getByRole("button", { name: "로그인" })).toBeTruthy();
});

test("recent_web_item_opens_that_workflow", async () => {
  const id = "aaaaaaaa-0000-4000-8000-000000000001";
  vi.mocked(api.webListWorkflows).mockResolvedValueOnce([{ id, name: "웹 흐름", active: true, updatedAt: "2026-11-12T00:00:00Z" }]);
  const get = vi.spyOn(api, "webGetWorkflow").mockResolvedValue({ id, name: "웹 흐름", nodes: [], edges: [] });
  vi.spyOn(api, "webListExecutions").mockResolvedValue({ content: [], hasNext: false, nextCursor: null });
  await act(async () => {
    render(<App />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /웹 흐름/ })));
  expect(get).toHaveBeenCalledWith(id);
  expect(screen.getByTestId("crumb").textContent).toBe("웹 이음");
  expect(screen.getByRole("heading", { name: "웹 흐름" })).toBeTruthy();
});
