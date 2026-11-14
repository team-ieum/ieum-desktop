// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api, type ChatTurn, type CredentialsView } from "../../api";
import { setLang } from "../../i18n";
import { useCredentials } from "../../stores/credentials";
import { aiChoices, AI_KEY } from "./chatAi";
import { ChatSlot } from "./ChatSlot";

const key = (id: string, isDefault = false) => ({ id, label: id, isDefault, createdAt: "", last4: "" });
const VIEW: CredentialsView = {
  apiKeys: { anthropic: [key("k1"), key("k2", true)], openai: [key("o1", true)], gemini: [] },
  claudeAccounts: [{ id: "system", label: "이 Mac", isDefault: true, createdAt: "", email: null }],
  codexAccounts: [],
  keychainError: null,
};
const EMPTY_CHAT = { messages: [], state: { outline: [], baseline: null, tests: {}, pendingFinish: null }, usage: [] };
let stage: (p: { id: string; stage: "designing" | "testing" }) => void = () => {};
let send: ReturnType<typeof vi.spyOn>;

// 이 환경의 localStorage는 온전하지 않다 — 메모리 저장소를 꽂는다(AssistantPanel.test.tsx와 같은 방식).
const memoryStorage = () => {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
};

beforeAll(() => setLang("ko"));
beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
  useCredentials.setState({ view: VIEW, load: async () => {} });
  vi.spyOn(api, "getChat").mockResolvedValue({
    ...EMPTY_CHAT,
    messages: [
      { role: "user", text: "안녕", at: "" },
      { role: "assistant", text: "네", at: "" },
    ],
  });
  vi.spyOn(api, "onChatStage").mockImplementation(async (cb) => ((stage = cb), () => {}));
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [], apiKeys: [] });
  send = vi.spyOn(api, "chatSend").mockResolvedValue({ say: "만들게요", cards: [], error: null });
  vi.spyOn(api, "listModels").mockImplementation(async (p) =>
    p === "api:openai"
      ? {
          models: [
            { id: "gpt-6-astra", label: "gpt-6-astra", efforts: ["low", "high"] },
            { id: "gpt-6-luna", label: "gpt-6-luna", efforts: ["low", "xhigh"] },
          ],
          defaultEfforts: [],
        }
      : p === "cli:agy"
        ? {
            models: [
              { id: "gemini-4-argon", label: "Gemini 4 Argon", efforts: ["low", "medium", "high"] },
              { id: "gemini-pro-orchestrator-g1-pro", label: "Orchestrator", efforts: [] },
            ],
            defaultEfforts: [],
          }
        : { models: [{ id: "claude-haiku-5-5", label: "claude-haiku-5-5", efforts: ["low", "max"] }], defaultEfforts: ["low", "max"] },
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const open = () => act(async () => render(<ChatSlot workflowId="w1" visible />));
const input = () => screen.getByLabelText("메시지 입력") as HTMLTextAreaElement;
/** 입력 카드의 고르기 칩(앱 드롭다운)을 열고 항목을 고른다. */
const chip = (label: string) => screen.getByRole("button", { name: new RegExp(`^${label}: `) });
const options = () => screen.getAllByRole("option").map((o) => o.textContent);
const choose = async (label: string, option: string) => {
  await act(async () => fireEvent.click(chip(label)));
  await act(async () => fireEvent.click(screen.getByRole("option", { name: option })));
};
const type = (v: string) => act(async () => fireEvent.change(input(), { target: { value: v } }));
const enter = (init: object = {}) => act(async () => fireEvent.keyDown(input(), { key: "Enter", ...init }));

test("ai_choices_order_and_default_first", () => {
  expect(aiChoices(VIEW).map((c) => c.value)).toEqual(["cli:claude|system", "api:anthropic|k2", "api:anthropic|k1", "api:openai|o1"]);
  expect(aiChoices(VIEW).map((c) => c.label)).toEqual(["Claude (구독) · 이 Mac", "Claude API · k2", "Claude API · k1", "OpenAI · o1"]);
  expect(aiChoices(null)).toEqual([]);
});

test("ai_choices_add_codex_and_agy_only_when_the_cli_is_enabled", async () => {
  const view = { ...VIEW, codexAccounts: [{ id: "c1", label: "회사", isDefault: true, createdAt: "", email: null }] };
  // 감지되지 않은(등록 안 됐거나 설치 안 된) CLI는 계정이 있어도 안 보인다.
  expect(aiChoices(view).map((c) => c.provider)).not.toContain("cli:codex");
  // AI 노드 공급원과 같은 순서: Claude → Codex → Antigravity → API 키.
  const all = aiChoices(view, ["cli:claude", "cli:codex", "cli:agy"]);
  expect(all.map((c) => c.value)).toEqual(["cli:claude|system", "cli:codex|c1", "cli:agy|", "api:anthropic|k2", "api:anthropic|k1", "api:openai|o1"]);
  expect(all.slice(1, 3).map((c) => c.label)).toEqual(["Codex (구독) · 회사", "Antigravity (구독)"]);
  // 화면: 감지 결과가 오면 선택지에 들고, 계정이 따로 없는 Antigravity는 credential 없이 보낸다.
  useCredentials.setState({ view });
  vi.mocked(api.detectProviders).mockResolvedValue({ clis: [{ id: "cli:agy", path: "", version: "" }], apiKeys: [] });
  await open();
  expect(screen.queryByRole("combobox")).toBeNull(); // 브라우저 기본 select 없음
  await act(async () => fireEvent.click(chip("채팅에 쓸 키·계정")));
  expect(options()).toEqual(["Claude (구독) · 이 Mac", "Antigravity (구독)", "Claude API · k2", "Claude API · k1", "OpenAI · o1"]);
  // 구독과 API 키를 머리말로 나눈다 — 머리말은 고를 수 없는 항목.
  const heads = within(screen.getByRole("listbox")).getAllByRole("presentation").map((h) => h.textContent);
  expect(heads).toEqual(["구독", "API 키"]);
  await act(async () => fireEvent.click(screen.getByRole("option", { name: "Antigravity (구독)" })));
  await type("안녕");
  await enter();
  expect(send).toHaveBeenCalledWith("w1", "안녕", { provider: "cli:agy", credential: null, model: null, effort: null });
});

test("chat_restores_messages_and_sends_on_enter_but_not_while_composing", async () => {
  const r = await open();
  expect(screen.getByText("안녕")).toBeTruthy(); // 저장된 대화 복원
  expect(r.container.querySelector('[data-slot="chat"]')).toBeTruthy();
  await type("정리해 줘");
  await enter({ isComposing: true }); // 한글 조합 중 Enter
  expect(send).not.toHaveBeenCalled();
  await enter();
  expect(send).toHaveBeenCalledWith("w1", "정리해 줘", { provider: "cli:claude", credential: "system", model: null, effort: null });
  expect(screen.getByText("정리해 줘").className).toContain("bg-main-blue");
  expect(screen.getByText("만들게요").className).toContain("rounded-[0_14px_14px_14px]");
  expect(input().value).toBe("");
  await enter(); // 빈 글은 보내지 않는다
  expect(send).toHaveBeenCalledTimes(1);
});

test("chat_shows_stage_while_waiting_and_blocks_second_send", async () => {
  let done: (t: ChatTurn) => void = () => {};
  send.mockReturnValue(new Promise<ChatTurn>((res) => (done = res)));
  await open();
  await type("하나");
  await enter();
  expect(input().disabled).toBe(true);
  await act(async () => stage({ id: "다른 워크플로우", stage: "testing" }));
  expect(screen.queryByText("노드를 테스트하고 있어요")).toBeNull();
  await act(async () => stage({ id: "w1", stage: "designing" }));
  expect(screen.getByText("워크플로우를 설계하고 있어요")).toBeTruthy();
  await act(async () => stage({ id: "w1", stage: "testing" }));
  expect(screen.getByText("노드를 테스트하고 있어요")).toBeTruthy();
  await act(async () => done({ say: "", cards: [], error: "키가 없어요" }));
  expect(screen.getByRole("alert").textContent).toContain("키가 없어요");
  expect(input().disabled).toBe(false);
});

test("chat_model_is_picked_from_the_provider_list_and_remembered", async () => {
  await open();
  // 기본 모델 + 그 공급원의 목록.
  await act(async () => fireEvent.click(chip("모델")));
  expect(options()).toEqual(["기본 모델", "claude-haiku-5-5"]);
  await act(async () => fireEvent.click(screen.getByRole("option", { name: "claude-haiku-5-5" })));
  // OpenAI는 기본 모델이 없다 — 목록 첫 모델을 골라 두고, "기본 모델" 항목이 없다.
  await choose("채팅에 쓸 키·계정", "OpenAI · o1");
  expect(chip("모델").getAttribute("aria-label")).toBe("모델: gpt-6-astra");
  await act(async () => fireEvent.click(chip("모델")));
  expect(options()).toEqual(["gpt-6-astra", "gpt-6-luna"]);
  await act(async () => fireEvent.click(screen.getByRole("option", { name: "gpt-6-luna" })));
  await type("안녕");
  await enter();
  expect(send).toHaveBeenCalledWith("w1", "안녕", { provider: "api:openai", credential: "o1", model: "gpt-6-luna", effort: null });
  expect(JSON.parse(localStorage.getItem(AI_KEY)!)).toEqual({ value: "api:openai|o1", model: "gpt-6-luna", effort: "" });
  cleanup();
  await open();
  expect(chip("채팅에 쓸 키·계정").getAttribute("aria-label")).toBe("채팅에 쓸 키·계정: OpenAI · o1");
  expect(chip("모델").getAttribute("aria-label")).toBe("모델: gpt-6-luna");
});

test("chat_effort_lists_only_what_the_model_takes_and_is_remembered", async () => {
  await open();
  // Claude 구독: 기본 모델이어도 --effort를 고를 수 있다.
  await act(async () => fireEvent.click(chip("추론 수준")));
  expect(options()).toEqual(["기본", "Low", "Max"]);
  await act(async () => fireEvent.click(screen.getByRole("option", { name: "Max" })));
  await type("안녕");
  await enter();
  expect(send).toHaveBeenLastCalledWith("w1", "안녕", { provider: "cli:claude", credential: "system", model: null, effort: "max" });
  // 모델을 바꾸면 수준은 기본으로, 목록은 그 모델의 것.
  await choose("채팅에 쓸 키·계정", "OpenAI · o1");
  await choose("모델", "gpt-6-luna");
  expect(chip("추론 수준").getAttribute("aria-label")).toBe("추론 수준: 기본");
  await act(async () => fireEvent.click(chip("추론 수준")));
  expect(options()).toEqual(["기본", "Low", "XHigh"]);
  await act(async () => fireEvent.click(screen.getByRole("option", { name: "XHigh" })));
  expect(JSON.parse(localStorage.getItem(AI_KEY)!)).toEqual({ value: "api:openai|o1", model: "gpt-6-luna", effort: "xhigh" });
  cleanup();
  await open();
  expect(chip("추론 수준").getAttribute("aria-label")).toBe("추론 수준: XHigh");
});

test("chat_effort_chip_is_off_with_a_reason_when_the_model_takes_none", async () => {
  const view = { ...VIEW };
  useCredentials.setState({ view });
  vi.mocked(api.detectProviders).mockResolvedValue({ clis: [{ id: "cli:agy", path: "", version: "" }], apiKeys: [] });
  localStorage.setItem(AI_KEY, JSON.stringify({ value: "cli:agy|", model: "" }));
  await open();
  // Antigravity 기본 모델: 수준이 모델 id에 묶여 못 고른다.
  expect((chip("추론 수준") as HTMLButtonElement).disabled).toBe(true);
  expect(chip("추론 수준").getAttribute("title")).toBe("Antigravity는 모델을 골라야 추론 수준을 고를 수 있어요.");
  await choose("모델", "Orchestrator");
  expect(chip("추론 수준").getAttribute("title")).toBe("이 모델은 추론 수준을 고를 수 없어요.");
  await choose("모델", "Gemini 4 Argon");
  expect((chip("추론 수준") as HTMLButtonElement).disabled).toBe(false);
});

test("chat_splits_an_old_antigravity_model_without_rewriting_it", async () => {
  vi.mocked(api.detectProviders).mockResolvedValue({ clis: [{ id: "cli:agy", path: "", version: "" }], apiKeys: [] });
  localStorage.setItem(AI_KEY, JSON.stringify({ value: "cli:agy|", model: "gemini-4-argon-high" }));
  await open();
  expect(chip("모델").getAttribute("aria-label")).toBe("모델: Gemini 4 Argon");
  expect(chip("추론 수준").getAttribute("aria-label")).toBe("추론 수준: High");
  // 같은 모델을 다시 골라도 저장본은 그대로.
  await choose("모델", "Gemini 4 Argon");
  expect(JSON.parse(localStorage.getItem(AI_KEY)!)).toEqual({ value: "cli:agy|", model: "gemini-4-argon-high" });
  await type("안녕");
  await enter();
  expect(send).toHaveBeenCalledWith("w1", "안녕", { provider: "cli:agy", credential: null, model: "gemini-4-argon-high", effort: null });
  // 수준을 고르면 모델은 수준을 뗀 이름으로.
  await choose("추론 수준", "Low");
  expect(JSON.parse(localStorage.getItem(AI_KEY)!)).toEqual({ value: "cli:agy|", model: "gemini-4-argon", effort: "low" });
});

test("chat_keeps_a_remembered_model_that_is_not_in_the_list", async () => {
  localStorage.setItem(AI_KEY, JSON.stringify({ value: "cli:claude|system", model: "claude-old-1" }));
  await open();
  expect(chip("모델").getAttribute("aria-label")).toBe("모델: claude-old-1 (목록에 없음)");
  await type("안녕");
  await enter();
  expect(send).toHaveBeenCalledWith("w1", "안녕", { provider: "cli:claude", credential: "system", model: "claude-old-1", effort: null });
});

test("openai_without_a_model_list_cannot_send_and_says_why", async () => {
  vi.mocked(api.listModels).mockRejectedValue("네트워크");
  localStorage.setItem(AI_KEY, JSON.stringify({ value: "api:openai|o1", model: "" }));
  await open();
  await type("안녕");
  await enter();
  expect(send).not.toHaveBeenCalled();
  expect(screen.getByText("OpenAI는 모델을 골라야 해요.")).toBeTruthy();
  expect(screen.getByRole("status").textContent).toContain("모델 목록을 읽지 못했어요");
});

test("empty_chat_greets_with_example_chips_that_send_right_away", async () => {
  vi.mocked(api.getChat).mockResolvedValue(EMPTY_CHAT);
  await open();
  expect(screen.getByText("어떤 자동화를 만들어 볼까요?")).toBeTruthy();
  const examples = within(screen.getByRole("list", { name: "예시" })).getAllByRole("button");
  expect(examples).toHaveLength(3);
  await act(async () => fireEvent.click(examples[0]));
  expect(send).toHaveBeenCalledWith("w1", "다운로드 폴더에 들어온 PDF를 '문서' 폴더로 옮겨 줘", expect.anything());
  expect(screen.queryByText("어떤 자동화를 만들어 볼까요?")).toBeNull();
});

test("korean_ime_enter_reported_as_keycode_229_does_not_send", async () => {
  await open();
  await type("정리해 줘");
  // WKWebView는 조합 확정 Enter를 isComposing 없이 keyCode 229로 보낸다.
  await enter({ keyCode: 229 });
  expect(send).not.toHaveBeenCalled();
  await enter();
  expect(send).toHaveBeenCalledTimes(1);
});

test("shift_enter_makes_a_new_line_instead_of_sending", async () => {
  await open();
  await type("첫 줄");
  await enter({ shiftKey: true });
  expect(send).not.toHaveBeenCalled();
});

test("chat_without_any_key_explains_and_disables", async () => {
  useCredentials.setState({ view: { ...VIEW, apiKeys: { anthropic: [], openai: [], gemini: [] }, claudeAccounts: [] } });
  await open();
  expect(screen.getByRole("note").textContent).toContain("설정에서 API 키나 Claude 구독을 연결하면");
  expect(input().disabled).toBe(true);
});

test("chat_survives_failed_loads_and_reports_busy", async () => {
  vi.spyOn(api, "getChat").mockRejectedValue("없음");
  vi.spyOn(api, "onChatStage").mockRejectedValue("없음");
  useCredentials.setState({ load: () => Promise.reject("없음") });
  send.mockRejectedValueOnce("CHAT_BUSY").mockRejectedValueOnce("다른 오류");
  await open(); // 빈 대화로 그린다
  await type("하나");
  await enter();
  expect(screen.getByRole("alert").textContent).toBe("앞 요청을 처리하고 있어요.");
  await type("둘");
  await enter();
  expect(screen.getAllByRole("alert")[1].textContent).toBe("응답을 받지 못했어요.");
});

test("pick_folder_card_opens_dialog_only_on_click_then_continues", async () => {
  send.mockResolvedValueOnce({ say: "폴더를 골라 주세요", cards: [{ kind: "pickFolder", nodeId: "t", field: "folder" }], error: null });
  const pick = vi
    .spyOn(api, "chatPickFolder")
    .mockResolvedValueOnce({ ok: false, code: "PICK_CANCELLED", message: "사용자가 폴더를 고르지 않음" })
    .mockResolvedValueOnce({ ok: true, data: { path: "/Users/me/in" } });
  await open();
  await type("시작");
  await enter();
  expect(pick).not.toHaveBeenCalled(); // 카드가 떴다고 창이 열리지 않는다
  const button = () => screen.getByRole("button", { name: "폴더 고르기" });
  await act(async () => fireEvent.click(button()));
  expect(pick).toHaveBeenCalledWith("w1", "t", "folder");
  expect(send).toHaveBeenCalledTimes(1); // 취소면 그대로
  await act(async () => fireEvent.click(button()));
  expect(screen.getByText("/Users/me/in")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "폴더 고르기" })).toBeNull();
  expect(send).toHaveBeenLastCalledWith("w1", "폴더를 골랐어요.", expect.anything());
});

test("test_card_shows_values_from_run_record_not_from_turn", async () => {
  send.mockResolvedValueOnce({ say: "테스트했어요", cards: [{ kind: "test", nodeId: "a", runId: "r1", status: "failed" }, { kind: "finish" }], error: null });
  vi.spyOn(api, "getRun").mockResolvedValue({
    run: {
      runId: "r1",
      status: "failed",
      validationErrors: [],
      nodes: {
        a: { status: "succeeded", output: { category: "영수증" }, error: null },
        m: { status: "failed", output: null, error: "옮길 파일이 없어요: /in/a.pdf" },
      },
    },
    workflowId: "w1",
    startedAt: "",
    finishedAt: "",
    fileOps: [],
    allowedRoots: [],
    undo: { restored: [], skipped: [[{ kind: "move", from: "/in/a.pdf", to: "/out/a.pdf" }, { reason: "missingNow" }]] },
  } as never);
  // 노드 이름은 편집기가 넘긴 맵에서 — 맵에 없는 노드는 상태만 보인다.
  await act(async () => render(<ChatSlot workflowId="w1" visible nodeNames={{ a: "분류" }} />));
  await type("테스트");
  await enter();
  const card = screen.getByRole("group", { name: "테스트 결과" });
  expect(within(card).getByText("분류 · 성공")).toBeTruthy();
  expect(within(card).getByText("실패")).toBeTruthy();
  expect(within(card).getByText(/category: 영수증/)).toBeTruthy();
  expect(within(card).getByText(/옮길 파일이 없어요/)).toBeTruthy();
  expect(within(card).getByText("되돌리지 못한 파일")).toBeTruthy();
  expect(within(card).getByText("/in/a.pdf")).toBeTruthy();
  expect(send).toHaveBeenCalledTimes(1); // 카드의 값은 다시 보내지 않는다
});
