// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { CredentialsView, Workflow, WfNode } from "../api";
import { setLang } from "../i18n";
import { useCredentials } from "../stores/credentials";
import { ConfigPanel } from "./ConfigPanel";

afterEach(cleanup);

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

test("ai_node_shows_missing_credential_warning", () => {
  setLang("ko");
  useCredentials.setState({ view: VIEW });
  const node = aiNode({ credential: "deadbeef" });
  render(<ConfigPanel node={node} workflow={{ nodes: [node], edges: [] }} onChange={() => {}} />);
  expect(screen.getByRole("alert").textContent).toContain("찾을 수 없는 키");
  expect((screen.getByLabelText("키·계정") as HTMLSelectElement).value).toBe("deadbeef");
  useCredentials.setState({ view: null });
});
