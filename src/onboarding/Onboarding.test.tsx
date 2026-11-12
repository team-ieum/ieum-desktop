// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import App from "../App";
import { api, type Settings } from "../api";
import { setLang } from "../i18n";
import { useSettings } from "../stores/settings";
import { useWeb } from "../stores/web";
import { Onboarding } from "./Onboarding";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

const ACCOUNT = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };
const BASE: Settings = { language: "ko", autostart: false, firstRunDone: true, serverUrl: "", onboardingDone: false, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true };

beforeAll(() => {
  setLang("ko");
  vi.spyOn(api, "onExecEvent").mockResolvedValue(() => {});
  vi.spyOn(api, "onOpenTarget").mockResolvedValue(() => {});
  vi.spyOn(api, "listWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "listRuns").mockResolvedValue([]);
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([]);
  vi.spyOn(api, "dashboardSummary").mockRejectedValue({ code: "NETWORK" });
  vi.spyOn(api, "dashboardExecutions").mockRejectedValue({ code: "NETWORK" });
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [], apiKeys: [] });
  vi.spyOn(api, "listCredentials").mockResolvedValue({ apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [], codexAccounts: [], keychainError: null });
  vi.spyOn(api, "openSystemSettings").mockResolvedValue();
  vi.spyOn(api, "sendTestNotification").mockResolvedValue();
  vi.spyOn(api, "subscriptionUsage").mockRejectedValue("no command");
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
});
beforeEach(() => {
  vi.spyOn(api, "setSettings").mockReset().mockResolvedValue();
  useSettings.setState({ settings: BASE });
  useWeb.setState({ account: ACCOUNT, expired: false, offline: false });
});
afterEach(cleanup);

const click = (name: string) => act(async () => fireEvent.click(screen.getByRole("button", { name })));
const heading = () => screen.getByRole("heading", { level: 1 }).textContent;

test("onboarding_shows_after_login_when_not_done", async () => {
  vi.spyOn(api, "getSettings").mockResolvedValue(BASE);
  vi.spyOn(api, "currentAccount").mockResolvedValue(ACCOUNT);
  await act(async () => {
    render(<App />);
  });
  expect(heading()).toBe("AI 연결");
  expect(screen.getAllByRole("listitem")).toHaveLength(3);
  expect(screen.queryByTestId("crumb")).toBeNull();
});

test("onboarding_ai_step_uses_provider_cards", async () => {
  await act(async () => {
    render(<Onboarding onDone={() => {}} />);
  });
  const names = screen.getAllByRole("article").map((a) => a.getAttribute("aria-label"));
  expect(names).toEqual(["Claude (구독)", "Codex (구독)", "Antigravity (구독)", "Anthropic API", "OpenAI API", "Gemini API"]);
  expect(screen.getByLabelText("Gemini API 키")).toBeTruthy();
});

test("onboarding_not_shown_after_done_even_for_other_account", async () => {
  vi.spyOn(api, "getSettings").mockResolvedValue({ ...BASE, onboardingDone: true });
  vi.spyOn(api, "currentAccount").mockResolvedValue({ email: "other@ieum.dev", serverUrl: ACCOUNT.serverUrl });
  await act(async () => {
    render(<App />);
  });
  expect(screen.getByTestId("crumb").textContent).toBe("대시보드");
  expect(screen.queryByRole("list", { name: "온보딩 진행" })).toBeNull();
});

test("onboarding_restarts_when_not_finished", async () => {
  const first = render(<Onboarding onDone={() => {}} />);
  await click("건너뛰기");
  expect(heading()).toBe("알림");
  first.unmount();
  await act(async () => {
    render(<Onboarding onDone={() => {}} />);
  });
  expect(heading()).toBe("AI 연결");
  expect(api.setSettings).not.toHaveBeenCalled();
});

test("notification_step_has_no_skip", async () => {
  render(<Onboarding onDone={() => {}} />);
  await click("건너뛰기");
  expect(heading()).toBe("알림");
  expect(screen.queryByRole("button", { name: "건너뛰기" })).toBeNull();
  const next = () => screen.getByRole("button", { name: "다음" }) as HTMLButtonElement;
  expect(next().disabled).toBe(true);
  await click("알림 켜기");
  expect(api.sendTestNotification).toHaveBeenCalled();
  expect(next().disabled).toBe(true);
  await click("예");
  expect(next().disabled).toBe(false);
});

test("notification_send_failure_shows_error_not_question", async () => {
  let fail = (_: unknown) => {};
  vi.spyOn(api, "sendTestNotification").mockReturnValueOnce(new Promise<void>((_, r) => (fail = r)));
  vi.spyOn(api, "openSystemSettings").mockRejectedValueOnce("설정 못 엶");
  render(<Onboarding onDone={() => {}} />);
  await click("건너뛰기");
  await click("알림 켜기");
  // 대기 중엔 연타 막힘
  expect((screen.getByRole("button", { name: "알림 켜기" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => fail("보내기 실패"));
  expect(screen.queryByText("알림이 보였나요?")).toBeNull();
  expect(screen.getByText("보내기 실패")).toBeTruthy();
  expect((screen.getByRole("button", { name: "다음" }) as HTMLButtonElement).disabled).toBe(false);
  await click("시스템 설정 열기");
  expect(screen.getByText("설정 못 엶")).toBeTruthy();
});

test("autostart_step_shows_current_value", async () => {
  useSettings.setState({ settings: { ...BASE, autostart: true } });
  const onDone = vi.fn();
  render(<Onboarding onDone={onDone} />);
  await click("건너뛰기");
  await click("알림 켜기");
  await click("아니요");
  await click("다음");
  expect(heading()).toBe("로그인 시 자동 실행");
  expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("true");
  await click("시작하기");
  expect(api.setSettings).toHaveBeenCalledWith(expect.objectContaining({ onboardingDone: true, firstRunDone: true, autostart: true }));
  expect(onDone).toHaveBeenCalled();
});

test("new_install_turning_autostart_off_marks_first_run_done", async () => {
  useSettings.setState({ settings: { ...BASE, firstRunDone: false, autostart: false } });
  render(<Onboarding onDone={() => {}} />);
  await click("건너뛰기");
  await click("알림 켜기");
  await click("예");
  await click("다음");
  // 새 설치는 켜짐 제안 → 끄고 시작하면 firstRunDone도 저장돼 다시 볼 때 꺼짐 그대로.
  expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("true");
  await act(async () => fireEvent.click(screen.getByRole("switch")));
  await click("시작하기");
  expect(api.setSettings).toHaveBeenCalledWith(expect.objectContaining({ onboardingDone: true, firstRunDone: true, autostart: false }));
});
