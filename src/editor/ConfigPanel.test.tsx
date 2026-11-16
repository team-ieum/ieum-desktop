// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
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
  const onBranch = vi.fn();
  const r = render(<ConfigPanel node={branch} workflow={wf} onChange={onBranch} />);
  expect(screen.queryByRole("combobox")).toBeNull(); // 브라우저 기본 select 없음
  expect(optionsOf(/^비교 방식/).length).toBe(20);
  // 비어 있으면 자리표시 — 첫 선택지를 고른 것처럼 보이지 않게
  expect(screen.getByRole("button", { name: /^여러 갈래가 맞으면/ }).textContent).toBe("고르세요");
  choose(/^여러 갈래가 맞으면/, "맞는 갈래 전부");
  expect(onBranch.mock.lastCall![0].config.pick).toBe("all");
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
  const onChange = vi.fn();
  render(<ConfigPanel node={ai} workflow={wf} onChange={onChange} />);
  // 참조 가능한 칸(추가 데이터)엔 뜨고, ref:false 목록(출력 칸)의 이름·설명엔 안 뜬다
  const data = within(screen.getByTestId("field-data"));
  expect(within(screen.getByTestId("field-outputFields")).queryByRole("button", { name: /^앞 단계 값 넣기/ })).toBeNull();
  // 고르면 글 끝에 참조식을 붙이고, 단추는 다시 "앞 단계 값 넣기"
  fireEvent.click(data.getByRole("button", { name: /^앞 단계 값 넣기/ }));
  fireEvent.click(within(screen.getByRole("listbox")).getAllByRole("option")[1]);
  expect(onChange.mock.lastCall![0].config.data).toMatch(/^\{\{nodes\.t\.output\.\w+\}\}$/);
  expect(data.getByRole("button", { name: /^앞 단계 값 넣기/ }).textContent).toBe("앞 단계 값 넣기");
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
/** 앱 드롭다운: 단추를 열고 항목을 고른다. */
const choose = (label: RegExp, option: string) => {
  fireEvent.click(screen.getByRole("button", { name: label }));
  fireEvent.click(within(screen.getByRole("listbox")).getByRole("option", { name: option }));
};
const optionsOf = (label: RegExp) => {
  fireEvent.click(screen.getByRole("button", { name: label }));
  const names = within(screen.getByRole("listbox")).getAllByRole("option").map((o) => o.textContent);
  fireEvent.click(screen.getByRole("button", { name: label })); // 닫기
  return names;
};
const aiNode = (config: Record<string, unknown>): WfNode => ({ id: "a", event: "local.ai", type: "AI", config: { provider: "api:anthropic", prompt: "p", ...config } });

test("ai_node_credential_dropdown_saves_id_or_omits_for_default", () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const onChange = vi.fn();
  const node = aiNode({});
  const r = render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  expect(screen.queryByRole("combobox")).toBeNull(); // 브라우저 기본 select 없음
  expect(optionsOf(/^키·계정/)).toEqual(["기본 (개인)", "개인", "회사"]);
  // 공급원 목록은 구독과 API 키를 머리말로 나눈다.
  fireEvent.click(screen.getByRole("button", { name: /^AI 공급원/ }));
  expect(within(screen.getByRole("listbox")).getAllByRole("presentation").map((h) => h.textContent)).toEqual(["구독", "API 키"]);
  fireEvent.click(screen.getByRole("button", { name: /^AI 공급원/ }));
  // 공급원 옆에 로고
  expect(screen.getByTestId("field-provider").querySelector('[data-logo="claude"]')).not.toBeNull();
  choose(/^키·계정/, "회사");
  expect(onChange.mock.lastCall![0].config.credential).toBe("0123abcd");
  r.unmount();

  const picked = aiNode({ credential: "0123abcd" });
  render(<ConfigPanel node={picked} workflow={{ nodes: [picked], edges: [] }} onChange={onChange} />);
  choose(/^키·계정/, "기본 (개인)");
  expect("credential" in onChange.mock.lastCall![0].config).toBe(false);
  // 공급원을 바꾸면 고른 키는 버린다(다른 공급원의 id).
  choose(/^AI 공급원/, "Claude (구독)");
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
  expect(screen.getByRole("button", { name: "AI 공급원: Codex (구독)" })).toBeTruthy();
  expect(screen.getByTestId("field-provider").querySelector('[data-logo="openai"]')).not.toBeNull();
  expect(optionsOf(/^키·계정/)).toEqual(["기본 (이 Mac의 로그인)", "이 Mac의 로그인", "회사"]);
  useCredentials.setState({ view: null });
});

test("agy_provider_cannot_send_file_content", () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const onChange = vi.fn();
  const node = aiNode({ provider: "cli:claude", sendContent: true });
  const r = render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  choose(/^AI 공급원/, "Antigravity (구독)");
  expect(onChange.mock.lastCall![0].config).toEqual({ provider: "cli:agy", prompt: "p" });
  r.unmount();

  // 이미 cli:agy면 체크 칸이 잠기고 안내가 보인다(저장본에 sendContent가 남아 있어도 켜진 것으로 보이지 않는다).
  const agy = aiNode({ provider: "cli:agy", sendContent: true });
  render(<ConfigPanel node={agy} workflow={{ nodes: [agy], edges: [] }} onChange={onChange} />);
  expect(screen.getByRole("button", { name: "AI 공급원: Antigravity (구독)" })).toBeTruthy();
  expect(screen.getByTestId("field-provider").querySelector('[data-logo="antigravity"]')).not.toBeNull();
  const box = screen.getByLabelText(/파일 내용도 보내기/) as HTMLInputElement;
  expect([box.disabled, box.checked]).toEqual([true, false]);
  expect(screen.getByText("Antigravity (구독)는 파일 내용을 보낼 수 없어요. 이름·확장자·크기만 가요.")).toBeTruthy();
  expect(optionsOf(/^키·계정/)).toHaveLength(1);
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
  choose(/^AI 공급원/, "Claude (구독)");
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
  // 저장된 id는 그대로 두고(다른 키로 대신하지 않는다) 단추에도 없다고 보인다.
  expect(screen.getByRole("button", { name: /^키·계정: 찾을 수 없는 키/ })).toBeTruthy();
  useCredentials.setState({ view: null });
});

/** 저장값을 다시 받아 그리는 패널 — 실제 편집기처럼. */
function Live({ start, onChange }: { start: WfNode; onChange: (n: WfNode) => void }) {
  const [node, setNode] = useState(start);
  return (
    <ConfigPanel
      node={node}
      workflow={{ nodes: [node], edges: [] }}
      onChange={(n) => {
        setNode(n);
        onChange(n);
      }}
    />
  );
}
const typeIn = (el: HTMLElement, v: string) => fireEvent.change(el, { target: { value: v } });

test("dict_field_saves_object", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "t", event: "transform.map", type: "TRANSFORM", config: { mappings: {} } };
  render(<Live start={node} onChange={onChange} />);
  const field = within(screen.getByTestId("field-mappings"));
  fireEvent.click(field.getByRole("button", { name: "줄 추가" }));
  const key = field.getByRole("textbox", { name: "매핑 1번째 줄 키" });
  expect(document.activeElement).toBe(key); // 키보드로 추가하면 바로 키를 친다
  typeIn(key, "a");
  // 키를 치는 도중 줄이 사라지거나 바뀌지 않는다
  expect(field.getByRole("textbox", { name: "매핑 1번째 줄 키" })).toBe(key);
  typeIn(field.getByRole("textbox", { name: "매핑 1번째 줄 값" }), "1");
  expect(onChange.mock.lastCall![0].config.mappings).toEqual({ a: "1" });
  // 빈 키 줄은 저장하지 않는다
  fireEvent.click(field.getByRole("button", { name: "줄 추가" }));
  typeIn(field.getByRole("textbox", { name: "매핑 2번째 줄 값" }), "x");
  expect(onChange.mock.lastCall![0].config.mappings).toEqual({ a: "1" });
  // 같은 키 줄은 표시하고 먼저 나온 줄만 저장
  typeIn(field.getByRole("textbox", { name: "매핑 2번째 줄 키" }), "a");
  expect(onChange.mock.lastCall![0].config.mappings).toEqual({ a: "1" });
  expect(field.getByRole("textbox", { name: "매핑 2번째 줄 키" }).getAttribute("aria-invalid")).toBe("true");
  expect(field.getByText("위에 같은 키가 있어 이 줄은 저장하지 않아요.")).toBeTruthy();
  fireEvent.click(field.getByRole("button", { name: "매핑 2번째 줄 빼기" }));
  fireEvent.click(field.getByRole("button", { name: "매핑 1번째 줄 빼기" }));
  expect(onChange.mock.lastCall![0].config.mappings).toEqual({});
});

test("dict_field_keeps_non_string_values_and_text_values", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "t", event: "transform.map", type: "TRANSFORM", config: { mappings: { n: 3, o: { x: 1 }, s: "v" } } };
  const r = render(<Live start={node} onChange={onChange} />);
  const field = within(screen.getByTestId("field-mappings"));
  expect((field.getByRole("textbox", { name: "매핑 1번째 줄 값" }) as HTMLInputElement).value).toBe("3");
  typeIn(field.getByRole("textbox", { name: "매핑 3번째 줄 값" }), "w");
  expect(onChange.mock.lastCall![0].config.mappings).toEqual({ n: 3, o: { x: 1 }, s: "w" });
  r.unmount();

  // 참조식 글로 들어온 대응표는 지우지 않고 글 칸으로
  const text: WfNode = { id: "l", event: "formatter.lookup_table", type: "TRANSFORM", config: { input: "", table: "{{nodes.a.output.t}}" } };
  render(<Live start={text} onChange={onChange} />);
  const table = within(screen.getByTestId("field-table"));
  expect((table.getByRole("textbox") as HTMLInputElement).value).toBe("{{nodes.a.output.t}}");
  expect(table.queryByRole("button", { name: "줄 추가" })).toBeNull();
});

test("same_content_with_new_reference_keeps_draft", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "t", event: "transform.map", type: "TRANSFORM", config: { mappings: { a: "1" } } };
  const panel = (n: WfNode) => <ConfigPanel node={n} workflow={{ nodes: [n], edges: [] }} onChange={onChange} />;
  const r = render(panel(node));
  fireEvent.click(screen.getByRole("button", { name: "줄 추가" }));
  // 저장 뒤 백엔드에서 받은 것처럼 — 내용은 같고 참조만 새것(키 순서도 다를 수 있다)
  r.rerender(panel(structuredClone(node)));
  expect(screen.getByRole("textbox", { name: "매핑 2번째 줄 키" })).toBeTruthy();
  // 내용이 달라지면(되돌리기 등) 다시 읽는다
  r.rerender(panel({ ...node, config: { mappings: { b: "2" } } }));
  expect(screen.queryByRole("textbox", { name: "매핑 2번째 줄 키" })).toBeNull();
  expect((screen.getByRole("textbox", { name: "매핑 1번째 줄 키" }) as HTMLInputElement).value).toBe("b");
  r.unmount();

  // 본문은 저장값을 받는 부모 아래서(실제 편집기처럼) 친 뒤, 같은 내용의 새 객체를 내려 준다.
  let push: (n: WfNode) => void = () => {};
  function Parent({ start }: { start: WfNode }) {
    const [n, setN] = useState(start);
    push = setN;
    return <ConfigPanel node={n} workflow={{ nodes: [n], edges: [] }} onChange={setN} />;
  }
  const http: WfNode = { id: "h", event: "http.request", type: "HTTP", config: { method: "POST", url: "https://x" } };
  render(<Parent start={http} />);
  typeIn(screen.getByTestId("field-body").querySelector("textarea")!, '{"b":2,"a":1}');
  act(() => push({ ...http, config: { ...http.config, body: { a: 1, b: 2 } } }));
  expect(screen.getByTestId("field-body").querySelector("textarea")!.value).toBe('{"b":2,"a":1}');
});

test("dict_text_value_rules", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "l", event: "formatter.lookup_table", type: "TRANSFORM", config: { input: "", table: "{{nodes.a.output.t}}" } };
  render(<Live start={node} onChange={onChange} />);
  const table = () => within(screen.getByTestId("field-table"));
  const box = table().getByRole("textbox");
  // 참조식은 글 그대로
  typeIn(box, "{{nodes.a.output.u}}");
  expect(onChange.mock.lastCall![0].config.table).toBe("{{nodes.a.output.u}}");
  // 일반 글은 저장하지 않고 알린다
  onChange.mockClear();
  typeIn(box, "hello");
  expect(onChange).not.toHaveBeenCalled();
  expect(box.getAttribute("aria-invalid")).toBe("true");
  expect((box as HTMLInputElement).value).toBe("hello");
  // 다 지우면 키를 빼고 줄 화면으로
  typeIn(box, "");
  expect("table" in onChange.mock.lastCall![0].config).toBe(false);
  expect(table().getByRole("button", { name: "줄 추가" })).toBeTruthy();
  cleanup();

  // JSON 객체 글은 객체로
  render(<Live start={node} onChange={onChange} />);
  typeIn(table().getByRole("textbox"), '{"k":"v"}');
  expect(onChange.mock.lastCall![0].config.table).toEqual({ k: "v" });
});

test("integer_field_matches_builder", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "x", event: "formatter.text_truncate", type: "TRANSFORM", config: { input: "abc" } };
  render(<Live start={node} onChange={onChange} />);
  const box = screen.getByRole("textbox", { name: /^최대 글자 수/ });
  typeIn(box, "+5");
  expect(onChange.mock.lastCall![0].config.maxLength).toBe(5);
  typeIn(box, " 7 ");
  expect(onChange.mock.lastCall![0].config.maxLength).toBe(7);
  onChange.mockClear();
  typeIn(box, "3.0");
  expect(onChange).not.toHaveBeenCalled();
  expect(box.getAttribute("aria-invalid")).toBe("true");
});

test("integer_field_saves_number", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "x", event: "formatter.text_truncate", type: "TRANSFORM", config: { input: "abc" } };
  render(<Live start={node} onChange={onChange} />);
  const box = screen.getByRole("textbox", { name: /^최대 글자 수/ });
  typeIn(box, "5");
  expect(onChange.mock.lastCall![0].config.maxLength).toBe(5);
  // 숫자가 아니면 저장하지 않고 알린다
  onChange.mockClear();
  typeIn(box, "5a");
  expect(onChange).not.toHaveBeenCalled();
  expect(box.getAttribute("aria-invalid")).toBe("true");
  // 참조식은 글로
  typeIn(box, "{{nodes.a.output.n}}");
  expect(onChange.mock.lastCall![0].config.maxLength).toBe("{{nodes.a.output.n}}");
  // 비우면 키를 뺀다
  typeIn(box, "");
  expect("maxLength" in onChange.mock.lastCall![0].config).toBe(false);
});

test("http_body_json_is_saved_as_object", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "h", event: "http.request", type: "HTTP", config: { method: "POST", url: "https://x" } };
  const r = render(<Live start={node} onChange={onChange} />);
  const body = screen.getByTestId("field-body").querySelector("textarea")!;
  typeIn(body, '{"a":1}');
  expect(onChange.mock.lastCall![0].config.body).toEqual({ a: 1 });
  expect(body.value).toBe('{"a":1}'); // 치는 대로 — 다시 포맷하지 않는다
  typeIn(body, "hi");
  expect(onChange.mock.lastCall![0].config.body).toBe("hi");
  typeIn(body, '{"a": {{nodes.b.output.x}}}');
  expect(onChange.mock.lastCall![0].config.body).toBe('{"a": {{nodes.b.output.x}}}');
  // 비우면 키를 뺀다 — 빈 글을 본문으로 보내지 않게.
  typeIn(body, "");
  expect("body" in onChange.mock.lastCall![0].config).toBe(false);
  r.unmount();

  render(<Live start={{ ...node, config: { ...node.config, body: { a: [1] } } }} onChange={onChange} />);
  expect(screen.getByTestId("field-body").querySelector("textarea")!.value).toBe(JSON.stringify({ a: [1] }, null, 2));
});

test("http_webhook_field_is_hidden", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "h", event: "http.request", type: "HTTP", config: { url: "https://x", webhookCredentialId: "w1" } };
  render(<Live start={node} onChange={onChange} />);
  expect(screen.queryByTestId("field-webhookCredentialId")).toBeNull();
  typeIn(screen.getByTestId("field-url").querySelector("input")!, "https://y");
  expect(onChange.mock.lastCall![0].config.webhookCredentialId).toBe("w1");
});

test("lookup_table_and_headers_use_dict_field", () => {
  setLang("ko");
  const onChange = vi.fn();
  const lookup: WfNode = { id: "l", event: "formatter.lookup_table", type: "TRANSFORM", config: { input: "", table: { k: "v" } } };
  const r = render(<Live start={lookup} onChange={onChange} />);
  expect((screen.getByRole("textbox", { name: "대응표 1번째 줄 키" }) as HTMLInputElement).value).toBe("k");
  r.unmount();
  const http: WfNode = { id: "h", event: "http.request", type: "HTTP", config: { url: "https://x" } };
  render(<Live start={http} onChange={onChange} />);
  const headers = within(screen.getByTestId("field-headers"));
  fireEvent.click(headers.getByRole("button", { name: "줄 추가" }));
  typeIn(headers.getByRole("textbox", { name: "헤더 1번째 줄 키" }), "X-A");
  expect(onChange.mock.lastCall![0].config.headers).toEqual({ "X-A": "" });
});

test("ai_node_without_provider_does_not_look_like_claude_is_picked", () => {
  setLang("ko");
  const onChange = vi.fn();
  const node: WfNode = { id: "a", event: "local.ai", type: "AI", config: { prompt: "p" } };
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={onChange} />);
  expect(screen.getByRole("button", { name: "AI 공급원: 공급원을 고르세요" })).toBeTruthy();
  choose(/^AI 공급원/, "Codex (구독)");
  expect(onChange.mock.lastCall![0].config).toEqual({ prompt: "p", provider: "cli:codex" });
});
