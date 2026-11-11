// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Workflow, WfNode } from "../api";
import { setLang } from "../i18n";
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
