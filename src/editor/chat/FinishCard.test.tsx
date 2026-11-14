// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type PendingFinish } from "../../api";
import { setLang } from "../../i18n";
import { usePendingFinish } from "../../stores/pendingFinish";
import { FinishCard } from "./FinishCard";

const P: PendingFinish = {
  hash: "h",
  folder: "/h/in",
  actions: [
    { nodeId: "m", event: "local.file_move", label: "분류해 옮기기", target: "/h/out/{{nodes.a.output.folder}}" },
    { nodeId: "x", event: "local.file_trash", label: "버리기", target: null },
  ],
};
beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  usePendingFinish.setState({ pending: {} });
  vi.restoreAllMocks();
});

test("card_shows_folder_and_actions_and_only_a_click_turns_on", async () => {
  const pend = vi.spyOn(api, "pendingFinish").mockResolvedValue(P);
  const confirm = vi.spyOn(api, "confirmFinish").mockResolvedValue({ ok: true });
  await act(async () => render(<FinishCard workflowId="w1" version="v1" />));
  const card = screen.getByRole("dialog", { name: "워크플로우를 켤까요?" });
  expect(within(card).getByText("/h/in")).toBeTruthy();
  expect(within(card).getByText("옮기기 → /h/out/{{nodes.a.output.folder}}")).toBeTruthy();
  expect(within(card).getByText("휴지통으로")).toBeTruthy();
  expect(confirm).not.toHaveBeenCalled(); // 띄운 것만으로는 켜지지 않는다
  pend.mockResolvedValue(null);
  await act(async () => fireEvent.click(within(card).getByRole("button", { name: "켜기" })));
  expect(confirm).toHaveBeenCalledWith("w1", true);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(usePendingFinish.getState().pending.w1).toBeUndefined();
});

test("card_lists_ai_nodes_and_marks_file_content_sending", async () => {
  vi.spyOn(api, "pendingFinish").mockResolvedValue({
    ...P,
    ai: [
      { nodeId: "a", label: "요약하기", provider: "cli:claude", sendContent: true },
      { nodeId: "b", label: "", provider: "api:openai", sendContent: false },
    ],
  });
  await act(async () => render(<FinishCard workflowId="w1" version="v1" />));
  const card = screen.getByRole("dialog");
  const rows = within(card).getAllByTestId("finish-ai");
  expect(rows.map((r) => r.textContent)).toEqual(["요약하기Claude (구독)파일 내용을 AI에 보냄", "AIOpenAI API 키파일 이름·크기만 보냄"]);
  // 파일 내용을 보내는 노드만 눈에 띄게.
  expect(within(rows[0]).getByText("파일 내용을 AI에 보냄").className).toContain("text-danger-700");
  expect(within(rows[1]).getByText("파일 이름·크기만 보냄").className).not.toContain("text-danger-700");
});

test("card_reads_old_pending_without_ai_list", async () => {
  // 이 칸이 생기기 전에 저장된 확인 대기.
  vi.spyOn(api, "pendingFinish").mockResolvedValue({ hash: "h", folder: "/h/in", actions: [] } as unknown as PendingFinish);
  await act(async () => render(<FinishCard workflowId="w1" version="v1" />));
  expect(within(screen.getByRole("dialog")).queryByTestId("finish-ai")).toBeNull();
});

test("card_disappears_when_workflow_changes_and_decline_clears", async () => {
  const pend = vi.spyOn(api, "pendingFinish").mockResolvedValue(P);
  const confirm = vi.spyOn(api, "confirmFinish").mockResolvedValue({ ok: true });
  const r = await act(async () => render(<FinishCard workflowId="w1" version="v1" />));
  pend.mockResolvedValue(null); // 저장본이 바뀌어 대기가 풀림
  await act(async () => r.rerender(<FinishCard workflowId="w1" version="v2" />));
  expect(screen.queryByRole("dialog")).toBeNull();
  pend.mockResolvedValue(P);
  await act(async () => r.rerender(<FinishCard workflowId="w1" version="v3" />));
  pend.mockResolvedValue(null);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "아직" })));
  expect(confirm).toHaveBeenCalledWith("w1", false);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("card_keeps_showing_reason_when_turn_on_is_refused", async () => {
  vi.spyOn(api, "pendingFinish").mockResolvedValue(P);
  vi.spyOn(api, "confirmFinish").mockResolvedValue({ ok: false, code: "STALE", message: "워크플로우가 방금 바뀌었음" });
  await act(async () => render(<FinishCard workflowId="w1" version="v1" />));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "켜기" })));
  expect(screen.getByRole("alert").textContent).toContain("워크플로우가 방금 바뀌었음");
  expect(render(<FinishCard workflowId="" version="" />).container.querySelector("[role=dialog]")).toBeNull();
});
