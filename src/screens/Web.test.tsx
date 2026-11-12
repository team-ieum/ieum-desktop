// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type WebExecPayload } from "../api";
import { setLang } from "../i18n";
import { Web } from "./Web";

const ex = (id: string, status: string) => ({ id, workflowId: "w1", status, startedAt: null });

beforeAll(() => {
  setLang("ko");
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  // @ts-expect-error jsdom에 없음
  globalThis.DOMMatrixReadOnly = class {
    m22 = 1;
  };
  vi.spyOn(api, "webListWorkflows").mockResolvedValue([{ id: "w1", name: "흐름", active: true }]);
  vi.spyOn(api, "webGetWorkflow").mockResolvedValue({ id: "w1", name: "흐름", nodes: [], edges: [] });
  vi.spyOn(api, "webExecutionLogs").mockResolvedValue([]);
});
afterEach(cleanup);

/** App처럼 목록↔워크플로우를 위에서 쥔다. */
function Harness() {
  const [id, setId] = useState<string | null>(null);
  return <Web workflow={id ?? undefined} onOpen={setId} />;
}

test("web_executions_more_appends_and_unwatch_on_leave", async () => {
  const list = vi
    .spyOn(api, "webListExecutions")
    .mockResolvedValueOnce({ content: [ex("e1", "RUNNING")], hasNext: true, nextCursor: "c1" })
    .mockRejectedValueOnce({ code: "NETWORK" })
    .mockResolvedValueOnce({ content: [ex("e2", "SUCCESS")], hasNext: false, nextCursor: null });
  vi.spyOn(api, "openWebEditor").mockRejectedValueOnce("브라우저를 열지 못했어요");
  let emit: (p: WebExecPayload) => void = () => {};
  vi.spyOn(api, "onWebExecEvent").mockImplementation(async (cb) => {
    emit = cb;
    return () => {};
  });
  vi.spyOn(api, "webWatchExecution").mockResolvedValue();
  const unwatch = vi.spyOn(api, "webUnwatchExecution").mockResolvedValue();

  await act(async () => {
    render(<Harness />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "흐름" })));

  // 웹 편집 열기 실패는 화면에.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "웹에서 수정하기" })));
  expect(screen.getByText("브라우저를 열지 못했어요")).toBeTruthy();

  // 더 보기: 실패 뒤 재시도가 성공하면 오류가 지워지고 이어 붙는다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "더 보기" })));
  expect(screen.getByText(/서버에 연결하지 못했어요/)).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "더 보기" })));
  expect(list).toHaveBeenLastCalledWith("w1", "c1");
  expect(screen.queryByText(/서버에 연결하지 못했어요/)).toBeNull();
  expect(screen.getAllByText("실행 중")).toHaveLength(1);
  expect(screen.getAllByText("성공")).toHaveLength(1);
  expect(screen.queryByRole("button", { name: "더 보기" })).toBeNull();

  // 실행 중인 e1을 열면 생중계.
  await act(async () => fireEvent.click(screen.getByText("실행 중")));
  expect(api.webWatchExecution).toHaveBeenCalledWith("w1", "e1");

  // 처음 로그에 없는 노드의 생중계 실패 문구도 보인다.
  await act(async () => emit({ executionId: "e1", event: { type: "NODE_FAILED", nodeId: "n9", errorMessage: "터짐" } }));
  expect(screen.getByText("터짐")).toBeTruthy();

  // 폴백 logs면 실행 목록을 다시 읽어 헤더 상태를 갱신한다.
  list.mockResolvedValueOnce({ content: [ex("e1", "FAILED")], hasNext: false, nextCursor: null });
  await act(async () => emit({ executionId: "e1", logs: [] }));
  expect(screen.getByText("실패")).toBeTruthy();

  // 떠나면 해제.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "뒤로" })));
  expect(unwatch).toHaveBeenCalledWith("e1");
});
