// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type HttpTestConfirm, type StoredRun, type WfNode } from "../../api";
import { setLang } from "../../i18n";
import { useTestRun } from "../useTestRun";
import { TestCard } from "./ChatCards";
import { HttpConfirm, HttpTestCard, showRefs } from "./HttpTestCard";

beforeAll(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const LINES = [{ nodeId: "h", label: "주문 넣기", method: "POST", url: "https://x.test/o/{{nodes.a.output.id}}" }];

/** 이벤트 구독을 가로채 테스트가 직접 쏜다. */
function captureEvent() {
  let emit: (p: HttpTestConfirm) => void = () => {};
  vi.spyOn(api, "onHttpTestConfirm").mockImplementation(async (cb) => ((emit = cb), () => {}));
  return (p: HttpTestConfirm) => act(() => emit(p));
}

test("show_refs_masks_reference_expressions", () => {
  expect(showRefs("https://x/{{nodes.a.output.id}}/{{ nodes.b.output.name }}?q=1")).toBe("https://x/〈앞 노드 값〉/〈앞 노드 값〉?q=1");
});

test("http_card_shows_method_and_masked_url_and_answers", async () => {
  const emit = captureEvent();
  const answer = vi.spyOn(api, "answerHttpConfirm").mockResolvedValue(true);
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  expect(screen.queryByRole("dialog")).toBeNull();
  await emit({ id: "w1", lines: LINES });
  const card = screen.getByRole("dialog", { name: "테스트로 실제 요청을 보낼까요?" });
  expect(within(card).getByText("POST https://x.test/o/〈앞 노드 값〉")).toBeTruthy();
  expect(within(card).getByText("주문 넣기")).toBeTruthy();
  expect(within(card).getByText("요청은 되돌릴 수 없어요.")).toBeTruthy();
  // 실수로 Enter를 눌러도 요청이 나가지 않게.
  expect(document.activeElement).toBe(within(card).getByRole("button", { name: "테스트 없이 넘어가기" }));
  const send = within(card).getByRole("button", { name: "보내기" });
  await act(async () => {
    fireEvent.click(send);
    fireEvent.click(send);
  });
  expect(answer).toHaveBeenCalledTimes(1);
  expect(answer).toHaveBeenCalledWith("w1", true);
  expect(screen.queryByRole("dialog")).toBeNull();

  await emit({ id: "w1", lines: LINES });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "테스트 없이 넘어가기" })));
  expect(answer).toHaveBeenLastCalledWith("w1", false);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("http_card_ignores_other_workflow_and_replaces_with_new_event", async () => {
  const emit = captureEvent();
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  await emit({ id: "w2", lines: LINES });
  expect(screen.queryByRole("dialog")).toBeNull();
  await emit({ id: "w1", lines: LINES });
  await emit({ id: "w1", lines: [{ nodeId: "g", label: "", method: "DELETE", url: "https://y.test" }] });
  const card = screen.getByRole("dialog");
  expect(within(card).queryByText("POST https://x.test/o/〈앞 노드 값〉")).toBeNull();
  expect(within(card).getByText("DELETE https://y.test")).toBeTruthy();
  // 이름이 없으면 노드 종류 이름.
  expect(within(card).getByText("HTTP 요청")).toBeTruthy();
});

test("http_card_closes_quietly_when_no_longer_waiting_and_after_5_minutes", async () => {
  const emit = captureEvent();
  vi.spyOn(api, "answerHttpConfirm").mockResolvedValue(false);
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  await emit({ id: "w1", lines: LINES });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "보내기" })));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();

  vi.useFakeTimers();
  await emit({ id: "w1", lines: LINES });
  act(() => vi.advanceTimersByTime(5 * 60_000 - 1));
  expect(screen.getByRole("dialog")).toBeTruthy();
  act(() => vi.advanceTimersByTime(1));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("http_card_does_not_close_on_escape", async () => {
  const emit = captureEvent();
  const answer = vi.spyOn(api, "answerHttpConfirm").mockResolvedValue(true);
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  await emit({ id: "w1", lines: LINES });
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  fireEvent.mouseDown(document.body);
  expect(screen.getByRole("dialog")).toBeTruthy();
  expect(answer).not.toHaveBeenCalled();
});

test("test_card_notes_real_request", async () => {
  vi.spyOn(api, "getRun").mockResolvedValue({ run: { nodes: {} } } as unknown as StoredRun);
  const r = await act(async () => render(<TestCard runId="r1" status="succeeded" httpSent />));
  expect(screen.getByText("실제로 요청을 보냈어요")).toBeTruthy();
  await act(async () => r.rerender(<TestCard runId="r1" status="succeeded" />));
  expect(screen.queryByText("실제로 요청을 보냈어요")).toBeNull();
});

function Harness({ nodes }: { nodes: WfNode[] }) {
  const run = useTestRun("w1", nodes);
  return (
    <>
      <button type="button" onClick={() => void run.run()}>
        run
      </button>
      {run.ask && <HttpConfirm lines={run.ask} skipLabel="취소" onAnswer={run.answer} />}
    </>
  );
}

const http = (id: string, method?: string): WfNode => ({ id, event: "http.request", config: { url: "https://x.test", ...(method ? { method } : {}) } });

test("test_run_asks_before_non_get_http", async () => {
  vi.spyOn(api, "pickPath").mockResolvedValue("/tmp/a.txt");
  const runNow = vi.spyOn(api, "runNow").mockResolvedValue("r1");
  const r = render(<Harness nodes={[http("g"), http("p", "post")]} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  const card = screen.getByRole("dialog", { name: "테스트로 실제 요청을 보낼까요?" });
  expect(within(card).getByText("POST https://x.test")).toBeTruthy();
  await act(async () => fireEvent.click(within(card).getByRole("button", { name: "취소" })));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(runNow).not.toHaveBeenCalled();

  await act(async () => fireEvent.click(screen.getByText("run")));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "보내기" })));
  expect(runNow).toHaveBeenCalledTimes(1);
  expect(runNow).toHaveBeenCalledWith("w1", "/tmp/a.txt");

  // GET만 있으면 묻지 않는다(대소문자·빈 method는 GET).
  r.rerender(<Harness nodes={[http("g"), http("h", "get")]} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(runNow).toHaveBeenCalledTimes(2);
});
