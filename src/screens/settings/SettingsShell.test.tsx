// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, onTestFinished, test, vi } from "vitest";
import App from "../../App";
import { api, type CliAccountMeta, type CredentialsView } from "../../api";
import { setLang } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { useWeb } from "../../stores/web";
import { SettingsShell, type SettingsSection } from "./SettingsShell";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

const ACCOUNT = { email: "me@ieum.dev", serverUrl: "https://green.kiacms.co.kr" };
const OPENAI_KEY = { id: "default", label: "기본", isDefault: true, createdAt: "", last4: "1234" };
const WORK_KEY = { id: "0123abcd", label: "회사", isDefault: false, createdAt: "2026-11-13T00:00:00Z", last4: "9876" };
const SYS: CliAccountMeta = { id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: null };
const WORK: CliAccountMeta = { id: "abcd1234", label: "회사", isDefault: false, createdAt: "2026-11-13T00:00:00Z", email: "work@claude.dev" };
const creds = (openai: (typeof OPENAI_KEY)[], more: Partial<CredentialsView> = {}): CredentialsView => ({
  apiKeys: { anthropic: [], openai, gemini: [] },
  claudeAccounts: [SYS],
  codexAccounts: [SYS],
  keychainError: null,
  ...more,
});
/** 이 테스트 동안만 목록을 바꾼다. */
const useCreds = (v: CredentialsView) => {
  const spy = vi.spyOn(api, "listCredentials").mockResolvedValue(v);
  onTestFinished(() => void spy.mockResolvedValue(creds([])));
  return spy;
};

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
  vi.spyOn(api, "subscriptionUsage").mockRejectedValue("no command");
  vi.spyOn(api, "antigravityPlanUsage").mockRejectedValue("no command");
  vi.spyOn(api, "usageSummary").mockRejectedValue("no command");
});
beforeEach(() => {
  useSettings.setState({ settings: { language: "ko", autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr", onboardingDone: true, notifyManualStart: true, notifyManualEnd: true, notifyWatchEnd: true } });
  useWeb.setState({ account: ACCOUNT, expired: false, offline: false });
});
afterEach(cleanup);

const renderShell = async (initial?: SettingsSection) => {
  await act(async () => {
    render(<SettingsShell onBack={() => {}} version="0.2.0" initial={initial} />);
  });
};
const menu = () => within(screen.getByRole("navigation")).getAllByRole("button").map((b) => b.textContent);
const title = () => screen.getByRole("heading", { level: 2 }).textContent;
const pages = () => ["account", "general", "permissions", "ai"].filter((id) => document.getElementById(`settings-${id}`));

test("settings_shows_only_selected_page", async () => {
  await renderShell();
  expect(pages()).toEqual(["account"]);
  expect(title()).toBe("이음 계정");
  expect(screen.getByText("me@ieum.dev")).toBeTruthy();
  expect(screen.queryByText("언어")).toBeNull();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: "일반" })));
  expect(pages()).toEqual(["general"]);
  expect(title()).toBe("일반");
  expect(screen.getByText("언어")).toBeTruthy();
  expect(screen.queryByText("me@ieum.dev")).toBeNull();

  await act(async () => fireEvent.click(screen.getByRole("button", { name: "AI 제공자" })));
  expect(pages()).toEqual(["ai"]);
  expect(screen.getByRole("button", { name: "AI 제공자" }).getAttribute("aria-current")).toBe("true");
});

test("deep_link_opens_ai_page", async () => {
  await renderShell("ai");
  expect(pages()).toEqual(["ai"]);
  expect(title()).toBe("AI 제공자");
  expect(screen.getByTestId("settings-crumb").textContent).toBe("AI 제공자");
  cleanup();

  // 상태 줄 팝오버 [AI 설정 열기] → AI 제공자 페이지만.
  await act(async () => {
    render(<App />);
  });
  await act(async () => fireEvent.click(screen.getByText(/오늘 API 예상/)));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "AI 설정 열기" })));
  expect(pages()).toEqual(["ai"]);
  expect(title()).toBe("AI 제공자");
});

test("search_narrows_menu_and_switches_page", async () => {
  await renderShell("general");
  const search = screen.getByLabelText("설정 검색");

  // 지금 페이지(일반)가 걸러지면 남은 첫 메뉴로.
  await act(async () => fireEvent.change(search, { target: { value: "알림" } }));
  expect(menu()).toEqual(["권한·준비"]);
  expect(pages()).toEqual(["permissions"]);
  expect(screen.getByRole("button", { name: "테스트 알림 보내기" })).toBeTruthy();
  expect(screen.queryByText("로그인할 때 자동으로 시작")).toBeNull();

  // 페이지 안에서는 맞는 부분만 — "API 키"면 구독 칸은 숨는다.
  await act(async () => fireEvent.change(search, { target: { value: "API 키" } }));
  expect(menu()).toEqual(["AI 제공자"]);
  expect(pages()).toEqual(["ai"]);
  expect(part(box("OpenAI"), "API 키")).toBeTruthy();
  expect(box("OpenAI").queryByRole("region", { name: "구독" })).toBeNull();

  // 지우면 모든 메뉴, 고른 페이지로.
  await act(async () => fireEvent.change(search, { target: { value: "" } }));
  expect(menu()).toEqual(["이음 계정", "일반", "권한·준비", "AI 제공자"]);
  expect(pages()).toEqual(["general"]);
});

test("settings_search_filters_rows", async () => {
  await renderShell("general");
  expect(screen.getByText("언어")).toBeTruthy();

  await act(async () => fireEvent.change(screen.getByLabelText("설정 검색"), { target: { value: "로그인할" } }));

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

/** 공급자 박스 하나, 그 안의 "구독"·"API 키" 칸. */
const box = (name: string) => within(screen.getByRole("article", { name }));
const part = (b: ReturnType<typeof box>, name: string) => within(b.getByRole("region", { name }));
const sub = (name: string) => part(box(name), "구독");
const keys = (name: string) => part(box(name), "API 키");
const inHours = (h: number) => new Date(Date.now() + h * 3600_000 + 30_000).toISOString();

test("claude_box_contains_subscription_and_anthropic_keys", async () => {
  useCreds(creds([], { apiKeys: { anthropic: [WORK_KEY], openai: [], gemini: [] }, claudeAccounts: [SYS, WORK] }));
  const open = vi.spyOn(api, "openProviderConsole").mockResolvedValue();
  await renderShell("ai");
  expect(screen.getAllByRole("article").map((a) => a.getAttribute("aria-label"))).toEqual(["Claude", "OpenAI", "Google"]);
  const s = sub("Claude");
  expect(s.getByRole("listitem", { name: "이 Mac의 로그인" })).toBeTruthy();
  expect(s.getByRole("listitem", { name: "회사" })).toBeTruthy();
  expect(s.getByRole("button", { name: "계정 연결" })).toBeTruthy();
  const k = keys("Claude");
  expect(k.getByText("…9876")).toBeTruthy();
  expect(k.getByRole("button", { name: "키 추가" })).toBeTruthy();
  // 키 받기 → 브라우저로(주소는 Rust 고정값). 주소 복사는 없다.
  await act(async () => fireEvent.click(box("Claude").getByRole("button", { name: /키 받기/ })));
  expect(open).toHaveBeenCalledWith("anthropic");
  expect(screen.queryByRole("button", { name: "주소 복사" })).toBeNull();
});

test("openai_box_contains_codex_and_openai_keys", async () => {
  useCreds(creds([OPENAI_KEY], { codexAccounts: [SYS, WORK] }));
  const open = vi.spyOn(api, "openProviderConsole").mockResolvedValue();
  await renderShell("ai");
  const s = sub("OpenAI");
  expect(s.getByRole("listitem", { name: "회사" })).toBeTruthy();
  expect(s.getByRole("button", { name: "계정 연결" })).toBeTruthy();
  expect(s.getByText("ChatGPT 구독 — 연결 상태와 사용량만 보여요")).toBeTruthy();
  expect(keys("OpenAI").getByText("…1234")).toBeTruthy();
  expect(sub("Claude").queryByRole("listitem", { name: "회사" })).toBeNull();
  await act(async () => fireEvent.click(box("OpenAI").getByRole("button", { name: /키 받기/ })));
  expect(open).toHaveBeenCalledWith("openai");
});

test("google_box_contains_antigravity_and_gemini_keys", async () => {
  vi.spyOn(api, "openProviderConsole").mockRejectedValue("브라우저를 열지 못했어요: exit 1");
  await renderShell("ai");
  const s = sub("Google");
  expect(s.getByText("Google Antigravity 구독 — 연결 상태와 사용량만 보여요")).toBeTruthy();
  expect(s.queryByRole("button", { name: "계정 연결" })).toBeNull();
  expect(s.getByRole("button", { name: "사용량 새로 고침" })).toBeTruthy();
  expect(keys("Google").getByLabelText("Gemini API 키")).toBeTruthy();
  // 열지 못하면 박스 안에 오류.
  await act(async () => fireEvent.click(box("Google").getByRole("button", { name: /키 받기/ })));
  expect(box("Google").getByText("브라우저를 열지 못했어요: exit 1")).toBeTruthy();
});

test("subscription_card_shows_login_hint_when_not_logged_in", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  vi.spyOn(api, "subscriptionUsage").mockImplementation((k) =>
    k === "codex" ? Promise.resolve({ state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null }) : Promise.reject("no command"),
  );
  await renderShell("ai");
  const codex = sub("OpenAI");
  expect(codex.getByText("로그인 필요")).toBeTruthy();
  expect(codex.getByText("codex")).toBeTruthy();
  await act(async () => fireEvent.click(codex.getByRole("button", { name: "명령 복사" })));
  expect(writeText).toHaveBeenCalledWith("codex");
  expect(codex.getByRole("button", { name: "복사됨" })).toBeTruthy();
});

test("subscription_card_shows_email_and_windows", async () => {
  const load = vi.spyOn(api, "subscriptionUsage").mockResolvedValue({
    state: "ok",
    windows: [
      { kind: "five_hour", usedPct: 34, resetsAt: inHours(58) },
      { kind: "seven_day", usedPct: 10, resetsAt: inHours(100) },
    ],
    checkedAt: new Date(Date.now() - 3 * 60_000 - 5_000).toISOString(),
    accountEmail: "me@claude.dev",
  });
  await renderShell("ai");
  const claude = sub("Claude");
  expect(claude.getByText("연결됨")).toBeTruthy();
  expect(claude.getByText("me@claude.dev")).toBeTruthy();
  expect(claude.getByText("66% 남음 · 2일 10시간 뒤 초기화")).toBeTruthy();
  expect(claude.getByText("90% 남음 · 4일 4시간 뒤 초기화")).toBeTruthy();
  expect(claude.getByText("3분 전에 조회")).toBeTruthy();
  const calls = load.mock.calls.length;
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "사용량 새로 고침" })));
  expect(load.mock.calls.length).toBe(calls + 1);
});

const row = (c: ReturnType<typeof box>, name: string) => within(c.getByRole("listitem", { name }));

test("api_card_lists_keys_with_last4_and_default", async () => {
  useCreds(creds([OPENAI_KEY, WORK_KEY]));
  const sum = vi.spyOn(api, "usageSummary").mockResolvedValue({
    totalUsd: 1.85,
    totalInput: 100_000,
    totalOutput: 23_000,
    unpricedCalls: 0,
    bySource: [{ source: "openai", input: 100_000, output: 23_000, usd: 1.85 }],
    byCredential: [{ source: "openai", credential: "0123abcd", input: 100_000, output: 23_000, usd: 1.85 }],
    byWorkflow: [],
    byDay: [],
    asOf: "2026-11-01",
  });
  onTestFinished(() => void sum.mockRejectedValue("no command"));
  await renderShell("ai");
  const openai = keys("OpenAI");
  expect(openai.getByText("…1234")).toBeTruthy();
  expect(openai.getByText("…9876")).toBeTruthy();
  // 기본 줄엔 [기본으로]가 없고 배지가 있다.
  expect(row(openai, "기본").queryByRole("button", { name: "기본으로" })).toBeNull();
  expect(row(openai, "기본").getByTestId("default-badge")).toBeTruthy();
  expect(row(openai, "회사").queryByTestId("default-badge")).toBeNull();
  expect(row(openai, "회사").getByText(/예상 \$1\.85/)).toBeTruthy();
  expect(row(openai, "기본").getByText(/예상 \$0\.00/)).toBeTruthy();
  expect(document.getElementById("settings-ai")!.textContent).not.toMatch(/sk-/);
});

test("add_key_then_set_default_and_delete_with_confirm", async () => {
  let finish = () => {};
  const add = vi.spyOn(api, "addApiKey").mockReturnValue(new Promise((r) => (finish = () => r(WORK_KEY))));
  const setDefault = vi.spyOn(api, "setDefaultCredential").mockResolvedValue();
  const del = vi.spyOn(api, "deleteCredential").mockResolvedValue();
  useCreds(creds([OPENAI_KEY, WORK_KEY]));
  await renderShell("ai");

  // 키가 없으면 바로 입력란 — 이름표 + 키.
  const anthropic = keys("Claude");
  expect(anthropic.queryByRole("button", { name: "삭제" })).toBeNull();
  await act(async () => fireEvent.change(anthropic.getByLabelText("Anthropic API 이름표"), { target: { value: "회사" } }));
  const key = anthropic.getByLabelText("Anthropic API 키") as HTMLInputElement;
  await act(async () => fireEvent.change(key, { target: { value: "sk-secret" } }));
  await act(async () => fireEvent.click(anthropic.getByRole("button", { name: "저장" })));
  expect(add).toHaveBeenCalledWith("anthropic", "회사", "sk-secret");
  // 저장이 끝나기 전에 이미 비어 있다 — 키를 상태에 남기지 않는다.
  expect(key.value).toBe("");
  await act(async () => finish());

  // 키가 있으면 [키 추가]로 연다.
  const openai = keys("OpenAI");
  expect(openai.queryByLabelText("OpenAI API 키")).toBeNull();
  await act(async () => fireEvent.click(openai.getByRole("button", { name: "키 추가" })));
  expect(openai.getByLabelText("OpenAI API 키")).toBeTruthy();

  await act(async () => fireEvent.click(row(openai, "회사").getByRole("button", { name: "기본으로" })));
  expect(setDefault).toHaveBeenCalledWith("openai", "0123abcd");

  await act(async () => fireEvent.click(row(openai, "회사").getByRole("button", { name: "삭제" })));
  expect(del).not.toHaveBeenCalled();
  await act(async () => fireEvent.click(row(openai, "회사").getByRole("button", { name: "정말 삭제" })));
  expect(del).toHaveBeenCalledWith("openai", "0123abcd");
});

const COMMAND = 'CLAUDE_CONFIG_DIR="/Users/me/Library/Application Support/ieum/accounts/k/claude/abcd1234" claude';

/** [계정 연결] → 이름표 → [다음]까지. */
const beginAccount = async () => {
  const begin = vi.spyOn(api, "beginCliAccount").mockResolvedValue({ id: "abcd1234", command: COMMAND });
  await renderShell("ai");
  const claude = sub("Claude");
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "계정 연결" })));
  await act(async () => fireEvent.change(claude.getByLabelText("새 계정 이름표"), { target: { value: "회사" } }));
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "다음" })));
  expect(begin).toHaveBeenCalledWith("claude", "회사");
  return claude;
};

test("add_claude_account_shows_quoted_command_and_confirms", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  const confirm = vi.spyOn(api, "confirmCliAccount").mockResolvedValue({ state: "ok", windows: [], checkedAt: "", accountEmail: "work@claude.dev" });
  const list = useCreds(creds([]));
  const claude = await beginAccount();

  expect(claude.getByText(COMMAND)).toBeTruthy();
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "명령 복사" })));
  expect(writeText).toHaveBeenCalledWith(COMMAND);

  list.mockResolvedValue(creds([], { claudeAccounts: [SYS, WORK] }));
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "확인" })));
  expect(confirm).toHaveBeenCalledWith("claude", "abcd1234");
  expect(row(claude, "회사").getByText("work@claude.dev")).toBeTruthy();
  expect(claude.queryByText(COMMAND)).toBeNull();
});

test("confirm_not_logged_in_keeps_pending_hint", async () => {
  vi.spyOn(api, "confirmCliAccount").mockResolvedValue({ state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null });
  const cancel = vi.spyOn(api, "cancelCliAccount").mockResolvedValue();
  const claude = await beginAccount();
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "확인" })));
  expect(claude.getByText("아직 로그인 안 됨")).toBeTruthy();
  expect(claude.getByText(COMMAND)).toBeTruthy();
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "취소" })));
  expect(cancel).toHaveBeenCalledWith("claude", "abcd1234");
  expect(claude.queryByText(COMMAND)).toBeNull();
});

test("connect_button_always_visible", async () => {
  vi.spyOn(api, "subscriptionUsage").mockImplementation((k) =>
    k === "codex" ? Promise.resolve({ state: "not_logged_in", windows: [], checkedAt: "", accountEmail: null }) : Promise.reject("no command"),
  );
  const unhide = vi.spyOn(api, "setSystemHidden").mockResolvedValue();
  // Claude는 이 Mac의 로그인을 숨겨서 계정이 하나도 없다.
  useCreds(creds([], { claudeAccounts: [] }));
  await renderShell("ai");
  expect(sub("OpenAI").getByRole("button", { name: "계정 연결" })).toBeTruthy();
  expect(sub("Google").queryByRole("button", { name: "계정 연결" })).toBeNull();
  const claude = sub("Claude");
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "계정 연결" })));
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "이 Mac의 로그인 다시 쓰기" })));
  expect(unhide).toHaveBeenCalledWith("claude", false);
});

test("disconnect_added_account_deletes_after_confirm", async () => {
  const del = vi.spyOn(api, "deleteCredential").mockClear().mockResolvedValue();
  useCreds(creds([], { claudeAccounts: [SYS, WORK] }));
  await renderShell("ai");
  const work = row(sub("Claude"), "회사");
  await act(async () => fireEvent.click(work.getByRole("button", { name: "연결 해제" })));
  expect(del).not.toHaveBeenCalled();
  expect(work.getByText("이 계정의 로그인 정보가 이 앱에서 지워져요")).toBeTruthy();
  await act(async () => fireEvent.click(work.getByRole("button", { name: "연결 해제" })));
  expect(del).toHaveBeenCalledWith("claude", "abcd1234");
});

test("next_and_remove_disabled_while_pending", async () => {
  // 응답 전 연타 — 보이지 않는 대기 폴더·두 번째 삭제가 생기지 않는다.
  const begin = vi.spyOn(api, "beginCliAccount").mockClear().mockReturnValue(new Promise(() => {}));
  const del = vi.spyOn(api, "deleteCredential").mockClear().mockReturnValue(new Promise(() => {}));
  useCreds(creds([], { claudeAccounts: [SYS, WORK] }));
  await renderShell("ai");
  const claude = sub("Claude");
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "계정 연결" })));
  await act(async () => fireEvent.change(claude.getByLabelText("새 계정 이름표"), { target: { value: "회사2" } }));
  await act(async () => fireEvent.click(claude.getByRole("button", { name: "다음" })));
  await act(async () => fireEvent.submit(claude.getByLabelText("새 계정 이름표")));
  expect(begin).toHaveBeenCalledTimes(1);
  const work = row(claude, "회사");
  await act(async () => fireEvent.click(work.getByRole("button", { name: "연결 해제" })));
  await act(async () => fireEvent.click(work.getByRole("button", { name: "연결 해제" })));
  await act(async () => fireEvent.click(work.getByRole("button", { name: "연결 해제" })));
  expect(del).toHaveBeenCalledTimes(1);
});

test("disconnect_system_hides_not_logout", async () => {
  const del = vi.spyOn(api, "deleteCredential").mockClear().mockResolvedValue();
  const hide = vi.spyOn(api, "setSystemHidden").mockClear().mockResolvedValue();
  const logout = vi.spyOn(api, "webLogout").mockClear().mockResolvedValue();
  await renderShell("ai");
  const sys = row(sub("Claude"), "이 Mac의 로그인");
  await act(async () => fireEvent.click(sys.getByRole("button", { name: "연결 해제" })));
  expect(hide).not.toHaveBeenCalled();
  await act(async () => fireEvent.click(sys.getByRole("button", { name: "연결 해제" })));
  expect(hide).toHaveBeenCalledWith("claude", true);
  expect(del).not.toHaveBeenCalled();
  expect(logout).not.toHaveBeenCalled();
});

test("provider_logos_render", async () => {
  await renderShell("ai");
  const ai = document.getElementById("settings-ai")!;
  const logos = [...ai.querySelectorAll("article [data-logo]")].map((e) => e.getAttribute("data-logo"));
  expect(logos).toEqual(["claude", "openai", "gemini"]);
  expect(ai.querySelectorAll("article [data-logo] svg path").length).toBe(3);
});

test("settings_search_matches_section_desc_and_button", async () => {
  await renderShell();
  const search = screen.getByLabelText("설정 검색");

  // 칸 이름 → 박스마다 그 칸만
  await act(async () => fireEvent.change(search, { target: { value: "API 키" } }));
  expect(screen.queryByText("맞는 설정이 없어요.")).toBeNull();
  expect(keys("OpenAI")).toBeTruthy();
  expect(keys("Google")).toBeTruthy();
  expect(screen.queryByRole("region", { name: "구독" })).toBeNull();
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
  await renderShell("permissions");
  expect(open).not.toHaveBeenCalled();
  const perms = document.getElementById("settings-permissions")!;
  expect(perms.textContent).toContain("폴더 권한은 워크플로우가 처음 그 폴더를 쓸 때 macOS가 물어봐요");
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "시스템 설정 열기" })[0]));
  expect(open).toHaveBeenCalledWith("files");
});

test("notify_toggles_save_settings", async () => {
  const set = vi.spyOn(api, "setSettings").mockResolvedValue();
  await renderShell("permissions");
  await act(async () => fireEvent.click(screen.getByRole("switch", { name: "폴더 감시 끝·실패" })));
  expect(set).toHaveBeenCalledWith(expect.objectContaining({ notifyWatchEnd: false, notifyManualStart: true }));
});

test("autostart_toggle_lives_only_here", async () => {
  await renderShell("general");
  const general = document.getElementById("settings-general")!;
  expect(general.textContent).not.toContain("로그인할 때 자동으로 시작");
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "권한·준비" })));
  const perms = document.getElementById("settings-permissions")!;
  expect(perms.querySelector('[role="switch"][aria-label="로그인할 때 자동으로 시작"]')).toBeTruthy();
});

test("replay_onboarding_resets_flag", async () => {
  const set = vi.spyOn(api, "setSettings").mockResolvedValue();
  await renderShell("permissions");
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

test("general_update_check_shows_result", async () => {
  vi.spyOn(api, "checkUpdateNow").mockResolvedValueOnce({ status: "latest" }).mockRejectedValueOnce("offline");
  await renderShell("general");
  expect(screen.getByText("업데이트는 시작할 때 자동으로 확인해요.")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "업데이트 확인" })));
  expect(screen.getByText("최신 버전이에요.")).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "업데이트 확인" })));
  expect(screen.getByText("업데이트를 확인하지 못했어요. 네트워크를 확인해 주세요.")).toBeTruthy();
});
