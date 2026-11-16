// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type CredentialsView, type GithubJob, type GithubStatus } from "../../api";
import { setLang } from "../../i18n";
import { useWeb } from "../../stores/web";
import { IntegrationsPane } from "./IntegrationsPane";
import { SettingsShell } from "./SettingsShell";

const CREDS: CredentialsView = { apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [], codexAccounts: [], keychainError: null };
let onCode: (p: { code: string }) => void = () => {};
let onLine: (p: { line: string }) => void = () => {};

beforeAll(() => {
  setLang("ko");
  vi.spyOn(api, "onGithubLoginCode").mockImplementation(async (cb) => {
    onCode = cb;
    return () => {};
  });
  vi.spyOn(api, "onGithubProgress").mockImplementation(async (cb) => {
    onLine = cb;
    return () => {};
  });
  vi.spyOn(api, "githubJob").mockResolvedValue(null);
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
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((r, j) => ((resolve = r), (reject = j)));
  return { promise, resolve, reject };
}

const renderPane = async (s: GithubStatus, job: GithubJob | null = null) => {
  vi.spyOn(api, "githubStatus").mockResolvedValue(s);
  vi.spyOn(api, "githubJob").mockResolvedValue(job);
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
  await renderPane({ kind: "notConnected", relogin: false });
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
  const disconnect = vi.spyOn(api, "githubDisconnect").mockResolvedValue({ kind: "notConnected", relogin: false });
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
  expect(screen.getByText("연결하면 터미널의 gh 로그인도 새 로그인으로 바뀌어요.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "연결" })).toBeTruthy();
});

test("unmount_cancels_login", async () => {
  const r = await renderPane({ kind: "notConnected", relogin: false });
  vi.spyOn(api, "githubConnect").mockReturnValue(deferred<GithubStatus>().promise);
  const cancel = vi.spyOn(api, "githubCancel").mockResolvedValue();
  await click("연결");
  r.unmount();
  expect(cancel).toHaveBeenCalledTimes(1);
});

test("unmount_keeps_install", async () => {
  const r = await renderPane({ kind: "notInstalled" });
  vi.spyOn(api, "githubInstall").mockReturnValue(deferred<GithubStatus>().promise);
  const cancel = vi.spyOn(api, "githubCancel").mockResolvedValue();
  await click("설치");
  r.unmount();
  expect(cancel).not.toHaveBeenCalled();
});

test("unmount_cancels_resumed_login", async () => {
  const r = await renderPane({ kind: "notConnected", relogin: false }, { kind: "login", code: null });
  const cancel = vi.spyOn(api, "githubCancel").mockResolvedValue();
  r.unmount();
  expect(cancel).toHaveBeenCalledTimes(1);
});

test("resumes_login_job", async () => {
  await renderPane({ kind: "notConnected", relogin: false }, { kind: "login", code: "AB12-CD34" });
  expect(screen.getByText("AB12-CD34")).toBeTruthy();
  expect(screen.getByRole("button", { name: "취소" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "연결" })).toBeNull();
});

test("resumes_install_job_until_done", async () => {
  vi.useFakeTimers();
  try {
    await renderPane({ kind: "notInstalled" }, { kind: "install", code: null });
    expect(screen.getByText("설치하는 중…")).toBeTruthy();
    expect(screen.getByRole("button", { name: "취소" })).toBeTruthy();
    vi.spyOn(api, "githubJob").mockResolvedValue(null);
    vi.spyOn(api, "githubStatus").mockResolvedValue({ kind: "notConnected", relogin: false });
    await act(async () => vi.advanceTimersByTimeAsync(1500));
    expect(screen.queryByText("설치하는 중…")).toBeNull();
    expect(screen.getByRole("button", { name: "연결" })).toBeTruthy();
  } finally {
    vi.useRealTimers();
  }
});

test("cancel_is_quiet", async () => {
  await renderPane({ kind: "notConnected", relogin: false });
  vi.spyOn(api, "githubConnect").mockRejectedValue("취소됨");
  await click("연결");
  expect(screen.queryByText("취소됨")).toBeNull();
  expect(screen.queryByRole("button", { name: "다시 시도" })).toBeNull();
  expect(api.githubStatus).toHaveBeenCalledTimes(2);
});

test("login_failure_shows_output", async () => {
  await renderPane({ kind: "notConnected", relogin: false });
  const done = deferred<GithubStatus>();
  vi.spyOn(api, "githubConnect").mockReturnValue(done.promise);
  await click("연결");
  await act(async () => onLine({ line: "error: device flow expired" }));
  await act(async () => done.reject("error: device flow expired"));
  expect(screen.getAllByText("error: device flow expired").length).toBeGreaterThan(0);
  expect(screen.getByText("출력 보기")).toBeTruthy();
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeTruthy();
});

test("unreachable", async () => {
  await renderPane({ kind: "unreachable", error: "proxyconnect tcp: connection refused" });
  expect(screen.getByText("GitHub에 닿지 못했어요.")).toBeTruthy();
  expect(screen.getByText("proxyconnect tcp: connection refused")).toBeTruthy();
  expect(screen.getByText("연결 확인 안 됨")).toBeTruthy();
  await click("다시 확인");
  expect(api.githubStatus).toHaveBeenCalledTimes(2);
});

test("settings_menu_has_integrations", async () => {
  vi.spyOn(api, "githubStatus").mockResolvedValue({ kind: "notConnected", relogin: false });
  useWeb.setState({ account: null, expired: false, offline: false });
  await act(async () => {
    render(<SettingsShell onBack={() => {}} version="0.2.0" initial="integrations" />);
  });
  const nav = within(screen.getByRole("navigation"));
  expect(nav.getByRole("button", { name: "연동" }).getAttribute("aria-current")).toBe("true");
  await act(async () => fireEvent.change(screen.getByLabelText("설정 검색"), { target: { value: "gh" } }));
  expect(nav.getByRole("button", { name: "연동" })).toBeTruthy();
  // 상태에 따라 나오는 문구(닿지 못함·다시 로그인 경고)로도 찾는다.
  for (const q of ["닿지", "새 로그인으로"]) {
    await act(async () => fireEvent.change(screen.getByLabelText("설정 검색"), { target: { value: q } }));
    expect(nav.getByRole("button", { name: "연동" })).toBeTruthy();
  }
});

test("first_connect_warns_when_relogin", async () => {
  await renderPane({ kind: "notConnected", relogin: true });
  expect(screen.getByText("연결하면 터미널의 gh 로그인도 새 로그인으로 바뀌어요.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "연결" })).toBeTruthy();
  cleanup();
  await renderPane({ kind: "notConnected", relogin: false });
  expect(screen.queryByText("연결하면 터미널의 gh 로그인도 새 로그인으로 바뀌어요.")).toBeNull();
});

test("polling_stops_after_unmount", async () => {
  vi.useFakeTimers();
  try {
    const r = await renderPane({ kind: "notInstalled" }, { kind: "install", code: null });
    await act(async () => vi.advanceTimersByTimeAsync(1100));
    const before = vi.mocked(api.githubJob).mock.calls.length;
    expect(before).toBeGreaterThan(1); // 마운트 1회 + 폴링
    r.unmount();
    await vi.advanceTimersByTimeAsync(5000);
    expect(vi.mocked(api.githubJob).mock.calls.length).toBe(before);
  } finally {
    vi.useRealTimers();
  }
});
