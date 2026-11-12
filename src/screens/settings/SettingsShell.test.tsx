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
  vi.spyOn(api, "getSettings").mockResolvedValue({ language: "ko", autostart: false, firstRunDone: true, serverUrl: "" });
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [{ id: "claude", path: "/usr/local/bin/claude", version: "2.1.0" }], apiKeys: [] });
  vi.spyOn(api, "keychainStatus").mockResolvedValue({ anthropic: "missing", openai: "missing", gemini: "missing", "ieum-session": "stored" });
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "currentAccount").mockResolvedValue(ACCOUNT);
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([]);
});
beforeEach(() => {
  useSettings.setState({ settings: { language: "ko", autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr" } });
  useWeb.setState({ account: ACCOUNT, expired: false, offline: false });
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
  expect(nav.textContent).toContain("일반");
  expect(nav.textContent).not.toContain("이음 계정");
  expect(nav.textContent).not.toContain("AI 제공자");
  expect(nav.textContent).not.toContain("API 키");
});

test("settings_clears_key_input_after_save", async () => {
  const setKey = vi.spyOn(api, "setApiKey").mockResolvedValue();
  await renderShell();

  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "추가" })[0]));
  const key = screen.getByLabelText("Anthropic API 키") as HTMLInputElement;
  await act(async () => fireEvent.change(key, { target: { value: "sk-secret" } }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "저장" })));
  expect(setKey).toHaveBeenCalledWith("anthropic", "sk-secret");

  // 저장하면 입력이 접히고, 다시 펼쳐도 비어 있다.
  expect(screen.queryByLabelText("Anthropic API 키")).toBeNull();
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "추가" })[0]));
  expect((screen.getByLabelText("Anthropic API 키") as HTMLInputElement).value).toBe("");
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
