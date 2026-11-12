// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, onTestFinished, test, vi } from "vitest";
import App from "../../App";
import { api } from "../../api";
import { setLang } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { useWeb } from "../../stores/web";
import { SettingsShell } from "./SettingsShell";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

const ACCOUNT = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };
const OPENAI_KEY = { id: "0123abcd", label: "기본", isDefault: true, createdAt: "", last4: "1234" };
const creds = (openai: (typeof OPENAI_KEY)[]) => ({ apiKeys: { anthropic: [], openai, gemini: [] }, claudeAccounts: [], codexAccounts: [], keychainError: null });

beforeAll(() => {
  setLang("ko");
  vi.spyOn(api, "getSettings").mockResolvedValue({ language: "ko", autostart: false, firstRunDone: true, serverUrl: "", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true });
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [{ id: "claude", path: "/usr/local/bin/claude", version: "2.1.0" }], apiKeys: [] });
  vi.spyOn(api, "listCredentials").mockResolvedValue(creds([]));
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "onOpenTarget").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "currentAccount").mockResolvedValue(ACCOUNT);
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "claudePlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "codexPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
});
beforeEach(() => {
  useSettings.setState({ settings: { language: "ko", autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true } });
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
  expect(nav.textContent).toContain("권한·준비");
  expect(document.getElementById("settings-general")).toBeNull();
  expect(nav.textContent).not.toContain("이음 계정");
  expect(nav.textContent).not.toContain("AI 제공자");
  expect(nav.textContent).not.toContain("API 키");
});

const card = (name: string) => within(screen.getByRole("article", { name }));
const inHours = (h: number) => new Date(Date.now() + h * 3600_000 + 30_000).toISOString();

test("ai_providers_section_lists_six_cards", async () => {
  await renderShell();
  const nav = screen.getByRole("navigation").textContent ?? "";
  expect(nav).toContain("AI 제공자");
  expect(nav).not.toContain("API 키");
  const names = within(document.getElementById("settings-ai")!)
    .getAllByRole("article")
    .map((a) => a.getAttribute("aria-label"));
  expect(names).toEqual(["Claude (구독)", "Codex (구독)", "Antigravity (구독)", "Anthropic API", "OpenAI API", "Gemini API"]);
});

test("subscription_card_shows_login_hint_when_not_logged_in", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  vi.spyOn(api, "codexPlanUsage").mockResolvedValue({ state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null });
  await renderShell();
  const codex = card("Codex (구독)");
  expect(codex.getByText("로그인 필요")).toBeTruthy();
  expect(codex.getByText("codex")).toBeTruthy();
  await act(async () => fireEvent.click(codex.getByRole("button", { name: "명령 복사" })));
  expect(writeText).toHaveBeenCalledWith("codex");
  expect(codex.getByRole("button", { name: "복사됨" })).toBeTruthy();
});

test("subscription_card_shows_email_and_windows", async () => {
  const load = vi.spyOn(api, "claudePlanUsage").mockResolvedValue({
    state: "ok",
    windows: [
      { kind: "five_hour", usedPct: 34, resetsAt: inHours(58) },
      { kind: "seven_day", usedPct: 10, resetsAt: inHours(100) },
    ],
    checkedAt: new Date(Date.now() - 3 * 60_000 - 5_000).toISOString(),
    accountEmail: "me@claude.dev",
  });
  await renderShell();
  const claude = card("Claude (구독)");
  expect(claude.getByText("연결됨")).toBeTruthy();
  expect(claude.getByText("me@claude.dev")).toBeTruthy();
  expect(claude.getByText("66% 남음 · 2일 10시간 뒤 초기화")).toBeTruthy();
  expect(claude.getByText("90% 남음 · 4일 4시간 뒤 초기화")).toBeTruthy();
  expect(claude.getByText("3분 전에 조회")).toBeTruthy();
  const calls = load.mock.calls.length;
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "사용량 새로 고침" })));
  expect(load.mock.calls.length).toBe(calls + 1);
});

test("api_key_card_save_and_delete", async () => {
  let finish = () => {};
  const setKey = vi.spyOn(api, "addApiKey").mockReturnValue(new Promise((r) => (finish = () => r(OPENAI_KEY))));
  const del = vi.spyOn(api, "deleteCredential").mockResolvedValue();
  const keychain = vi.spyOn(api, "listCredentials").mockResolvedValue(creds([OPENAI_KEY]));
  onTestFinished(() => void keychain.mockResolvedValue(creds([])));
  await renderShell();

  const anthropic = card("Anthropic API");
  expect(anthropic.getByText("console.anthropic.com/settings/keys")).toBeTruthy();
  expect(anthropic.queryByRole("button", { name: "삭제" })).toBeNull();
  const key = anthropic.getByLabelText("Anthropic API 키") as HTMLInputElement;
  await act(async () => fireEvent.change(key, { target: { value: "sk-secret" } }));
  await act(async () => fireEvent.click(anthropic.getByRole("button", { name: "저장" })));
  expect(setKey).toHaveBeenCalledWith("anthropic", "기본", "sk-secret");
  // 저장이 끝나기 전에 이미 비어 있다 — 키를 상태에 남기지 않는다.
  expect(key.value).toBe("");
  await act(async () => finish());

  const openai = card("OpenAI API");
  expect(openai.getByText("키체인에 저장됨")).toBeTruthy();
  await act(async () => fireEvent.click(openai.getByRole("button", { name: "삭제" })));
  expect(del).toHaveBeenCalledWith("openai", "0123abcd");
});

test("settings_search_matches_section_desc_and_button", async () => {
  await renderShell();
  const search = screen.getByLabelText("설정 검색");

  // 카드 설명 → 그 카드만
  await act(async () => fireEvent.change(search, { target: { value: "API 키" } }));
  expect(screen.queryByText("맞는 설정이 없어요.")).toBeNull();
  expect(screen.getByRole("article", { name: "OpenAI API" })).toBeTruthy();
  expect(screen.getByRole("article", { name: "Gemini API" })).toBeTruthy();
  expect(screen.queryByRole("article", { name: "Codex (구독)" })).toBeNull();
  expect(screen.getByTestId("settings-crumb").textContent).toBe("AI 제공자");

  // 버튼 문구
  await act(async () => fireEvent.change(search, { target: { value: "로그아웃" } }));
  expect(screen.getByText("me@ieum.dev")).toBeTruthy();
  expect(screen.getByTestId("settings-crumb").textContent).toBe("이음 계정");

  // 줄 설명
  await act(async () => fireEvent.change(search, { target: { value: "메뉴 막대" } }));
  expect(screen.getByText("로그인할 때 자동으로 시작")).toBeTruthy();
  expect(screen.getByTestId("settings-crumb").textContent).toBe("권한·준비");
});

test("permissions_pane_opens_files_settings_without_checking_folders", async () => {
  const open = vi.spyOn(api, "openSystemSettings").mockResolvedValue();
  await renderShell();
  expect(open).not.toHaveBeenCalled();
  const perms = document.getElementById("settings-permissions")!;
  expect(perms.textContent).toContain("폴더 권한은 워크플로우가 처음 그 폴더를 쓸 때 macOS가 물어봐요");
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "시스템 설정 열기" })[0]));
  expect(open).toHaveBeenCalledWith("files");
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
