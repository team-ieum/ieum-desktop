// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import App from "../../App";
import { api } from "../../api";
import { setLang } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { useWeb } from "../../stores/web";
import { SettingsShell } from "./SettingsShell";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

const ACCOUNT = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };

beforeAll(() => {
  setLang("ko");
  vi.spyOn(api, "getSettings").mockResolvedValue({ language: "ko", autostart: false, firstRunDone: true, serverUrl: "", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true });
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [{ id: "claude", path: "/usr/local/bin/claude", version: "2.1.0" }], apiKeys: [] });
  vi.spyOn(api, "keychainStatus").mockResolvedValue({ anthropic: "missing", openai: "missing", gemini: "missing", "ieum-session": "stored" });
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "onOpenTarget").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "currentAccount").mockResolvedValue(ACCOUNT);
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([]);
});
beforeEach(() => {
  useSettings.setState({ settings: { language: "ko", autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true } });
  useWeb.setState({ account: ACCOUNT, expired: false, offline: false });
  vi.spyOn(api, "folderAccess").mockResolvedValue("granted");
});
afterEach(cleanup);

const renderShell = async (onBack = () => {}) => {
  await act(async () => {
    render(<SettingsShell onBack={onBack} version="0.2.0" />);
  });
};

test("settings_search_filters_rows", async () => {
  await renderShell();
  expect(screen.getByText("언어")).toBeTruthy();

  await act(async () => fireEvent.change(screen.getByLabelText("설정 검색"), { target: { value: "자동" } }));

  expect(screen.getByText("로그인할 때 자동으로 시작")).toBeTruthy();
  expect(screen.queryByText("언어")).toBeNull();
  expect(screen.queryByText("me@ieum.dev")).toBeNull();
  const nav = screen.getByRole("navigation");
  expect(nav.textContent).toContain("권한·준비");
  expect(document.getElementById("settings-general")).toBeNull();
  expect(nav.textContent).not.toContain("이음 계정");
  expect(nav.textContent).not.toContain("AI 제공자");
  expect(nav.textContent).not.toContain("API 키");
});

test("settings_clears_key_input_after_save", async () => {
  let finish = () => {};
  const setKey = vi.spyOn(api, "setApiKey").mockReturnValue(new Promise<void>((r) => (finish = r)));
  await renderShell();

  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "추가" })[0]));
  const key = screen.getByLabelText("Anthropic API 키") as HTMLInputElement;
  await act(async () => fireEvent.change(key, { target: { value: "sk-secret" } }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "저장" })));
  expect(setKey).toHaveBeenCalledWith("anthropic", "sk-secret");

  // 저장이 끝나기 전에 이미 접히고 비어 있다 — 다시 펼쳐도 키가 남아 있지 않다.
  expect(screen.queryByLabelText("Anthropic API 키")).toBeNull();
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "추가" })[0]));
  expect((screen.getByLabelText("Anthropic API 키") as HTMLInputElement).value).toBe("");
  await act(async () => finish());
});

test("settings_search_matches_section_desc_and_button", async () => {
  await renderShell();
  const search = screen.getByLabelText("설정 검색");

  // 묶음 이름 → 그 묶음의 줄 전부
  await act(async () => fireEvent.change(search, { target: { value: "API 키" } }));
  expect(screen.queryByText("맞는 설정이 없어요.")).toBeNull();
  expect(screen.getByText("OpenAI")).toBeTruthy();
  expect(screen.getByText("Gemini")).toBeTruthy();
  expect(screen.getByTestId("settings-crumb").textContent).toBe("API 키");

  // 버튼 문구
  await act(async () => fireEvent.change(search, { target: { value: "로그아웃" } }));
  expect(screen.getByText("me@ieum.dev")).toBeTruthy();
  expect(screen.getByTestId("settings-crumb").textContent).toBe("이음 계정");

  // 줄 설명
  await act(async () => fireEvent.change(search, { target: { value: "메뉴 막대" } }));
  expect(screen.getByText("로그인할 때 자동으로 시작")).toBeTruthy();
  expect(screen.getByTestId("settings-crumb").textContent).toBe("권한·준비");
});

test("permissions_pane_rechecks_folders_on_open", async () => {
  const access = vi.spyOn(api, "folderAccess").mockResolvedValue("denied");
  access.mockClear();
  await renderShell();
  expect(access).toHaveBeenCalledTimes(3);
  expect(screen.getAllByText("거부됨")).toHaveLength(3);
});

test("notify_toggles_save_settings", async () => {
  const set = vi.spyOn(api, "setSettings").mockResolvedValue();
  await renderShell();
  await act(async () => fireEvent.click(screen.getByRole("switch", { name: "폴더 감시 끝·실패" })));
  expect(set).toHaveBeenCalledWith(expect.objectContaining({ notifyWatchEnd: false, notifyManualStart: true }));
});

test("autostart_toggle_lives_only_here", async () => {
  await renderShell();
  const general = document.getElementById("settings-general")!;
  const perms = document.getElementById("settings-permissions")!;
  expect(general.textContent).not.toContain("로그인할 때 자동으로 시작");
  expect(perms.querySelector('[role="switch"][aria-label="로그인할 때 자동으로 시작"]')).toBeTruthy();
});

test("replay_onboarding_resets_flag", async () => {
  const set = vi.spyOn(api, "setSettings").mockResolvedValue();
  await renderShell();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "온보딩 다시 보기" })));
  expect(set).toHaveBeenCalledWith(expect.objectContaining({ onboardingDone: false }));
});

test("settings_search_finds_permissions", async () => {
  await renderShell();
  await act(async () => fireEvent.change(screen.getByLabelText("설정 검색"), { target: { value: "알림" } }));
  expect(screen.getByRole("navigation").textContent).toContain("권한·준비");
  expect(screen.getByRole("button", { name: "테스트 알림 보내기" })).toBeTruthy();
});

test("settings_logout_returns_to_gate", async () => {
  const logout = vi.spyOn(api, "webLogout").mockResolvedValue();
  await renderShell();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "로그아웃" })));
  expect(logout).toHaveBeenCalledTimes(1);
  expect(useWeb.getState().account).toBeNull();
});

test("settings_back_returns_to_previous_page", async () => {
  await act(async () => {
    render(<App />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "실행 기록" })));
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "설정" })[0]));
  expect(screen.getByRole("button", { name: "앱으로 돌아가기" })).toBeTruthy();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: "앱으로 돌아가기" })));
  expect(screen.queryByRole("button", { name: "앱으로 돌아가기" })).toBeNull();
  expect(screen.getByRole("button", { name: "실행 기록" }).getAttribute("aria-current")).toBe("page");
});

test("relogin_after_logout_opens_dashboard", async () => {
  vi.spyOn(api, "webLogout").mockResolvedValue();
  vi.spyOn(api, "webLogin").mockResolvedValue({ ...ACCOUNT, expiresAt: "" } as never);
  await act(async () => {
    render(<App />);
  });
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "설정" })[0]));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "로그아웃" })));
  expect(screen.queryByRole("button", { name: "앱으로 돌아가기" })).toBeNull();

  await act(async () => useWeb.getState().login(ACCOUNT.email, "pw"));
  expect(screen.queryByRole("button", { name: "앱으로 돌아가기" })).toBeNull();
  expect(screen.getByRole("button", { name: "대시보드" }).getAttribute("aria-current")).toBe("page");
});
