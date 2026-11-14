// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type CredentialsView, type Workflow, type WfNode } from "../api";
import { setLang } from "../i18n";
import { useCredentials } from "../stores/credentials";
import { ConfigPanel } from "./ConfigPanel";

beforeEach(() => {
  vi.spyOn(api, "listModels").mockImplementation(async (p) =>
    p === "api:anthropic"
      ? {
          models: [
            { id: "claude-haiku-5-5", label: "claude-haiku-5-5", efforts: ["low", "high"] },
            { id: "claude-opus-5-5", label: "claude-opus-5-5", efforts: ["low", "max"] },
          ],
          defaultEfforts: ["low", "high"],
        }
      : p === "cli:agy"
        ? { models: [{ id: "gemini-4-argon", label: "Gemini 4 Argon", efforts: ["low", "medium", "high"] }], defaultEfforts: [] }
        : { models: [], defaultEfforts: [] },
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test("config_panel_renders_choices_and_ref_fields", async () => {
  setLang("ko");
  const branch: WfNode = {
    id: "b",
    event: "branch.if",
    type: "CONDITION",
    config: {
      branches: [{ key: "b_0a1b2c3d", name: "청구서", match: "all", rules: [{ left: "", operator: "equals", right: "" }] }],
    },
  };
  const move: WfNode = { id: "m", event: "local.file_move", type: "ACTION", config: {} };
  const wf: Workflow = { nodes: [branch, move], edges: [] };

  // branch.if: 갈래 조건의 비교 방식 선택지 20개
  const r = render(<ConfigPanel node={branch} workflow={wf} onChange={() => {}} />);
  const op = screen.getByLabelText(/^비교 방식/) as HTMLSelectElement;
  expect([...op.options].filter((o) => o.value !== "").length).toBe(20);
  r.unmount();

  // local.file_move: 기준 폴더는 텍스트 입력 없이 폴더 고르기 버튼만
  const onChange = vi.fn();
  const pickFolder = vi.fn().mockResolvedValue("/Users/me/Documents");
  render(<ConfigPanel node={move} workflow={wf} onChange={onChange} pickFolder={pickFolder} />);
  const base = screen.getByTestId("field-baseFolder");
  expect(base.querySelector("input, textarea")).toBeNull();
  const button = base.querySelector("button")!;
  await act(async () => {
    fireEvent.click(button);
  });
  expect(pickFolder).toHaveBeenCalled();
  expect(onChange).toHaveBeenCalledWith(
    expect.objectContaining({ config: expect.objectContaining({ baseFolder: "/Users/me/Documents" }) }),
  );
  // 참조 가능한 칸(목적지)은 텍스트 입력
  expect(screen.getByTestId("field-destination").querySelector("input")).not.toBeNull();
});

test("config_panel_no_ref_picker_in_ref_false_list", () => {
  setLang("ko");
  const trig: WfNode = { id: "t", event: "local.folder_new_file", type: "TRIGGER", config: {} };
  const ai: WfNode = {
    id: "a",
    event: "local.ai",
    type: "AI",
    config: { outputFields: [{ key: "folder", type: "text", description: "" }] },
  };
  const wf: Workflow = { nodes: [trig, ai], edges: [{ source: "t", target: "a" }] };
  render(<ConfigPanel node={ai} workflow={wf} onChange={() => {}} />);
  // 참조 가능한 칸(추가 데이터)엔 뜨고, ref:false 목록(출력 칸)의 이름·설명엔 안 뜬다
  expect(screen.getByTestId("field-data").querySelector('select[aria-label="앞 단계 값 넣기"]')).not.toBeNull();
  expect(screen.getByTestId("field-outputFields").querySelector('select[aria-label="앞 단계 값 넣기"]')).toBeNull();
});

const VIEW: CredentialsView = {
  apiKeys: {
    anthropic: [
      { id: "default", label: "개인", isDefault: true, createdAt: "", last4: "1111" },
      { id: "0123abcd", label: "회사", isDefault: false, createdAt: "1", last4: "2222" },
    ],
    openai: [],
    gemini: [],
  },
  claudeAccounts: [{ id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: null }],
  codexAccounts: [],
  keychainError: null,
};
const aiNode = (config: Record<string, unknown>): WfNode => ({ id: "a", event: "local.ai", type: "AI", config: { provider: "api:anthropic", prompt: "p", ...config } });

test("ai_node_credential_dropdown_saves_id_or_omits_for_default", () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const onChange = vi.fn();
  const node = aiNode({});
  const r = render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  const sel = screen.getByLabelText("키·계정") as HTMLSelectElement;
  expect([...sel.options].map((o) => [o.value, o.text])).toEqual([
    ["", "기본 (개인)"],
    ["default", "개인"],
    ["0123abcd", "회사"],
  ]);
  // 공급원 옆에 로고
  expect(screen.getByTestId("field-provider").querySelector('[data-logo="anthropic"]')).not.toBeNull();
  fireEvent.change(sel, { target: { value: "0123abcd" } });
  expect(onChange.mock.lastCall![0].config.credential).toBe("0123abcd");
  r.unmount();

  const picked = aiNode({ credential: "0123abcd" });
  render(<ConfigPanel node={picked} workflow={{ nodes: [picked], edges: [] }} onChange={onChange} />);
  fireEvent.change(screen.getByLabelText("키·계정"), { target: { value: "" } });
  expect("credential" in onChange.mock.lastCall![0].config).toBe(false);
  // 공급원을 바꾸면 고른 키는 버린다(다른 공급원의 id).
  fireEvent.change(screen.getByLabelText(/^AI 공급원/), { target: { value: "cli:claude" } });
  expect(onChange.mock.lastCall![0].config).toEqual({ provider: "cli:claude", prompt: "p" });
  useCredentials.setState({ view: null });
});

test("codex_provider_lists_codex_accounts", () => {
  setLang("ko");
  useCredentials.setState({
    view: {
      ...VIEW,
      codexAccounts: [
        { id: "system", label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: null },
        { id: "c0dec0de", label: "회사", isDefault: false, createdAt: "1", email: null },
      ],
    },
  });
  const node = aiNode({ provider: "cli:codex" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={() => {}} />);
  expect(screen.getByRole("option", { name: "Codex (구독)" })).toBeTruthy();
  expect(screen.getByTestId("field-provider").querySelector('[data-logo="openai"]')).not.toBeNull();
  const cred = screen.getByLabelText(/키·계정/) as HTMLSelectElement;
  expect([...cred.options].map((o) => o.value)).toEqual(["", "system", "c0dec0de"]);
  useCredentials.setState({ view: null });
});

test("agy_provider_cannot_send_file_content", () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const onChange = vi.fn();
  const node = aiNode({ provider: "cli:claude", sendContent: true });
  const r = render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  fireEvent.change(screen.getByLabelText(/AI 공급원/), { target: { value: "cli:agy" } });
  expect(onChange.mock.lastCall![0].config).toEqual({ provider: "cli:agy", prompt: "p" });
  r.unmount();

  // 이미 cli:agy면 체크 칸이 잠기고 안내가 보인다(저장본에 sendContent가 남아 있어도 켜진 것으로 보이지 않는다).
  const agy = aiNode({ provider: "cli:agy", sendContent: true });
  render(<ConfigPanel node={agy} workflow={{ nodes: [agy], edges: [] }} onChange={onChange} />);
  expect(screen.getByRole("option", { name: "Antigravity (구독)" })).toBeTruthy();
  expect(screen.getByTestId("field-provider").querySelector('[data-logo="antigravity"]')).not.toBeNull();
  const box = screen.getByLabelText(/파일 내용도 보내기/) as HTMLInputElement;
  expect([box.disabled, box.checked]).toEqual([true, false]);
  expect(screen.getByText("Antigravity (구독)는 파일 내용을 보낼 수 없어요. 이름·확장자·크기만 가요.")).toBeTruthy();
  expect((screen.getByLabelText(/키·계정/) as HTMLSelectElement).options.length).toBe(1);
  useCredentials.setState({ view: null });
});

test("ai_node_model_is_a_dropdown_of_the_provider_models", async () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const onChange = vi.fn();
  const node = aiNode({});
  const r = render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  const model = await screen.findByRole("button", { name: "모델: 기본 모델" });
  expect(screen.queryByRole("textbox", { name: /모델/ })).toBeNull(); // 직접 입력 칸 없음
  fireEvent.click(model);
  const names = within(screen.getByRole("listbox", { name: "모델" }))
    .getAllByRole("option")
    .map((o) => o.textContent);
  expect(names).toEqual(["기본 모델", "claude-haiku-5-5", "claude-opus-5-5"]);
  fireEvent.click(screen.getByRole("option", { name: "claude-opus-5-5" }));
  expect(onChange.mock.lastCall![0].config.model).toBe("claude-opus-5-5");
  r.unmount();

  // 목록에 없는 저장값은 그대로 보이고 바뀌지 않는다. "기본 모델"이면 칸을 뺀다.
  const old = aiNode({ model: "claude-old-1" });
  onChange.mockClear();
  render(<ConfigPanel node={old} workflow={{ nodes: [old], edges: [] }} onChange={onChange} />);
  const kept = await screen.findByRole("button", { name: /^모델: claude-old-1 \(목록에 없음\)$/ }); // act 밖에서 찾는다
  await act(async () => fireEvent.click(kept));
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("option", { name: "기본 모델" }));
  expect("model" in onChange.mock.lastCall![0].config).toBe(false);
  useCredentials.setState({ view: null });
});

test("openai_node_without_model_asks_to_pick_one_instead_of_showing_the_first", async () => {
  setLang("ko");
  vi.mocked(api.listModels).mockResolvedValue({
    models: [
      { id: "gpt-6-astra", label: "GPT-6 Astra", efforts: [] },
      { id: "gpt-6-luna", label: "GPT-6 Luna", efforts: [] },
    ],
    defaultEfforts: [],
  });
  const onChange = vi.fn();
  const node = aiNode({ provider: "api:openai" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  // 저장본에 모델이 없다 — 첫 모델을 고른 것처럼 보이면 안 된다.
  const model = await screen.findByRole("button", { name: "모델: 모델을 고르세요" });
  fireEvent.click(model);
  const names = within(screen.getByRole("listbox", { name: "모델" }))
    .getAllByRole("option")
    .map((o) => o.textContent);
  expect(names).toEqual(["모델을 고르세요", "GPT-6 Astra", "GPT-6 Luna"]); // "기본 모델"은 없다
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("option", { name: "GPT-6 Luna" }));
  expect(onChange.mock.lastCall![0].config.model).toBe("gpt-6-luna"); // 저장은 원래 id
});

test("ai_node_effort_follows_the_model_and_resets_when_it_changes", async () => {
  setLang("ko");
  const onChange = vi.fn();
  const node = aiNode({ model: "claude-opus-5-5", effort: "max" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  const effort = await screen.findByRole("button", { name: "추론 수준: Max" });
  await act(async () => fireEvent.click(effort));
  expect(within(screen.getByRole("listbox", { name: "추론 수준" })).getAllByRole("option").map((o) => o.textContent)).toEqual([
    "기본",
    "Low",
    "Max",
  ]);
  fireEvent.click(within(screen.getByRole("listbox", { name: "추론 수준" })).getByRole("option", { name: "기본" }));
  expect("effort" in onChange.mock.lastCall![0].config).toBe(false); // 기본이면 칸을 뺀다
  // 모델을 바꾸면 수준은 기본으로.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /^모델: / })));
  fireEvent.click(screen.getByRole("option", { name: "claude-haiku-5-5" }));
  expect(onChange.mock.lastCall![0].config).toEqual({ provider: "api:anthropic", prompt: "p", model: "claude-haiku-5-5" });
  // 공급원을 바꿔도.
  fireEvent.change(screen.getByLabelText(/AI 공급원/), { target: { value: "cli:claude" } });
  expect(onChange.mock.lastCall![0].config).toEqual({ provider: "cli:claude", prompt: "p" });
});

test("ai_node_splits_an_old_antigravity_model_and_rewrites_only_on_change", async () => {
  setLang("ko");
  const onChange = vi.fn();
  const node = aiNode({ provider: "cli:agy", model: "gemini-4-argon-high" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  expect(await screen.findByRole("button", { name: "모델: Gemini 4 Argon" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "추론 수준: High" })).toBeTruthy();
  // 같은 모델·수준을 다시 골라도 저장본을 고치지 않는다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "모델: Gemini 4 Argon" })));
  fireEvent.click(screen.getByRole("option", { name: "Gemini 4 Argon" }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "추론 수준: High" })));
  fireEvent.click(screen.getByRole("option", { name: "High" }));
  expect(onChange).not.toHaveBeenCalled();
  // 수준을 바꾸면 모델은 수준을 뗀 이름으로 함께 저장.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "추론 수준: High" })));
  fireEvent.click(screen.getByRole("option", { name: "Low" }));
  expect(onChange.mock.lastCall![0].config).toEqual({ provider: "cli:agy", prompt: "p", model: "gemini-4-argon", effort: "low" });
});

test("ai_node_effort_is_off_with_a_reason_for_agy_default_model", async () => {
  setLang("ko");
  const node = aiNode({ provider: "cli:agy" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={() => {}} />);
  await screen.findByRole("button", { name: "모델: 기본 모델" });
  const effort = screen.getByRole("button", { name: "추론 수준: 기본" }) as HTMLButtonElement;
  expect(effort.disabled).toBe(true);
  expect(effort.title).toBe("Antigravity는 모델을 골라야 추론 수준을 고를 수 있어요.");
});

test("ai_node_model_list_failure_is_shown_not_swallowed", async () => {
  setLang("ko");
  vi.mocked(api.listModels).mockRejectedValue("codex 없음");
  const node = aiNode({ provider: "cli:codex" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={() => {}} />);
  expect((await screen.findByRole("alert")).textContent).toContain("모델 목록을 읽지 못했어요");
  expect(screen.getByRole("button", { name: "모델: 기본 모델" })).toBeTruthy();
});

test("ai_node_shows_missing_credential_warning", () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const node = aiNode({ credential: "deadbeef" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={() => {}} />);
  expect(screen.getByRole("alert").textContent).toContain("찾을 수 없는 키");
  expect((screen.getByLabelText("키·계정") as HTMLSelectElement).value).toBe("deadbeef");
  useCredentials.setState({ view: null });
});
