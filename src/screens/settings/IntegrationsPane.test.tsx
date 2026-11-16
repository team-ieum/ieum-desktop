// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type CredentialsView, type GithubStatus } from "../../api";
import { setLang } from "../../i18n";
import { useWeb } from "../../stores/web";
import { IntegrationsPane } from "./IntegrationsPane";
import { SettingsShell } from "./SettingsShell";

const CREDS: CredentialsView = { apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [], codexAccounts: [], keychainError: null };
let onCode: (p: { code: string }) => void = () => {};

beforeAll(() => {
  setLang("ko");
  vi.spyOn(api, "onGithubLoginCode").mockImplementation(async (cb) => {
    onCode = cb;
    return () => {};
  });
  vi.spyOn(api, "onGithubInstallProgress").mockResolvedValue(() => {});
  vi.spyOn(api, "listCredentials").mockResolvedValue(CREDS);
  vi.spyOn(api, "subscriptionUsage").mockRejectedValue("no command");
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

/** 대기 중인 promise와 그 resolve. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

const renderPane = async (s: GithubStatus) => {
  vi.spyOn(api, "githubStatus").mockResolvedValue(s);
  let r!: ReturnType<typeof render>;
  await act(async () => {
    r = render(<IntegrationsPane />);
  });
  return r;
};
const click = (name: string) => act(async () => fireEvent.click(screen.getByRole("button", { name })));

test("shows_install_when_missing", async () => {
  await renderPane({ kind: "notInstalled" });
  expect(screen.getByText("GitHub을 쓰려면 gh CLI가 필요해요. 설치할까요?")).toBeTruthy();
  expect(screen.getByRole("button", { name: "다시 확인" })).toBeTruthy();
  const install = vi.spyOn(api, "githubInstall").mockReturnValue(deferred<GithubStatus>().promise);
  await click("설치");
  expect(install).toHaveBeenCalledTimes(1);
  expect(screen.getByText("설치하는 중…")).toBeTruthy();
  expect(screen.getByRole("button", { name: "취소" })).toBeTruthy();
});

test("install_failure_shows_reason_and_retry", async () => {
  await renderPane({ kind: "outdated", version: "2.60.0" });
  expect(screen.getByText("gh 2.60.0는 너무 오래됐어요(2.81.0 이상 필요).")).toBeTruthy();
  const install = vi.spyOn(api, "githubInstall").mockRejectedValue("체크섬이 맞지 않아요");
  await click("업데이트");
  expect(screen.getByText("체크섬이 맞지 않아요")).toBeTruthy();
  await click("다시 시도");
  expect(install).toHaveBeenCalledTimes(2);
});

test("connect_shows_code_card", async () => {
  await renderPane({ kind: "notConnected" });
  const done = deferred<GithubStatus>();
  const connect = vi.spyOn(api, "githubConnect").mockReturnValue(done.promise);
  const open = vi.spyOn(api, "githubOpenDevice").mockResolvedValue();
  await click("연결");
  expect(connect).toHaveBeenCalledTimes(1);
  await act(async () => onCode({ code: "AB12-CD34" }));
  expect(screen.getByText("일회용 코드")).toBeTruthy();
  expect(screen.getByText("AB12-CD34")).toBeTruthy();
  expect(screen.getByText("브라우저에서 코드를 입력하면 자동으로 연결돼요.")).toBeTruthy();
  await click("브라우저 열기");
  expect(open).toHaveBeenCalledTimes(1);
  await act(async () => done.resolve({ kind: "connected", login: "hodoon", envToken: false }));
  expect(screen.getByText("hodoon")).toBeTruthy();
  expect(screen.getByText("연결됨")).toBeTruthy();
  expect(screen.queryByText("AB12-CD34")).toBeNull();
});

test("disconnect_calls_api_only", async () => {
  await renderPane({ kind: "connected", login: "hodoon", envToken: false });
  const disconnect = vi.spyOn(api, "githubDisconnect").mockResolvedValue({ kind: "notConnected" });
  await click("연결 해제");
  expect(disconnect).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "연결" })).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("env_token_line", async () => {
  await renderPane({ kind: "connected", login: "hodoon", envToken: true });
  expect(screen.getByText("셸 환경변수 토큰을 쓰는 중이에요.")).toBeTruthy();
});

test("account_changed", async () => {
  await renderPane({ kind: "accountChanged", recorded: "hodoon", current: "other" });
  expect(screen.getByText("gh 계정이 other(으)로 바뀌었어요.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "연결 해제" })).toBeTruthy();
  const connect = vi.spyOn(api, "githubConnect").mockResolvedValue({ kind: "connected", login: "other", envToken: false });
  await click("이 계정으로 연결");
  expect(connect).toHaveBeenCalledTimes(1);
});

test("needs_login", async () => {
  await renderPane({ kind: "needsLogin", recorded: "hodoon" });
  expect(screen.getByText("gh 로그인이 필요해요.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "연결" })).toBeTruthy();
});

test("unmount_cancels", async () => {
  const r = await renderPane({ kind: "notConnected" });
  vi.spyOn(api, "githubConnect").mockReturnValue(deferred<GithubStatus>().promise);
  const cancel = vi.spyOn(api, "githubCancel").mockResolvedValue();
  await click("연결");
  r.unmount();
  expect(cancel).toHaveBeenCalledTimes(1);
});

test("settings_menu_has_integrations", async () => {
  vi.spyOn(api, "githubStatus").mockResolvedValue({ kind: "notConnected" });
  useWeb.setState({ account: null, expired: false, offline: false });
  await act(async () => {
    render(<SettingsShell onBack={() => {}} version="0.2.0" initial="integrations" />);
  });
  const nav = within(screen.getByRole("navigation"));
  expect(nav.getByRole("button", { name: "연동" }).getAttribute("aria-current")).toBe("true");
  await act(async () => fireEvent.change(screen.getByLabelText("설정 검색"), { target: { value: "gh" } }));
  expect(nav.getByRole("button", { name: "연동" })).toBeTruthy();
});
