// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api, type HttpLine, type HttpTestConfirm, type StoredRun, type WfNode } from "../../api";
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

const line = (o: Partial<HttpLine> = {}): HttpLine => ({
  nodeId: "h",
  label: "주문 넣기",
  method: "POST",
  url: "https://x.test/o/{{nodes.a.output.id}}",
  headerKeys: [],
  sends: [],
  hasBody: false,
  ...o,
});
const LINES = [line()];

const requests = (card: HTMLElement) => within(card).getAllByTestId("http-request").map((e) => e.textContent);

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
  expect(requests(card)).toEqual(["POST https://x.test/o/〈앞 노드 값〉"]);
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
  await emit({ id: "w1", lines: [line({ nodeId: "g", label: "", method: "DELETE", url: "https://y.test" })] });
  const card = screen.getByRole("dialog");
  expect(requests(card)).toEqual(["DELETE https://y.test"]);
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

const bold = (card: HTMLElement) => Array.from(card.querySelectorAll("strong"), (e) => e.textContent);
const confirmCard = (url: string) => {
  render(<HttpConfirm lines={[line({ url })]} skipLabel="취소" onAnswer={() => {}} />);
  return screen.getByRole("dialog");
};

test("http_card_bolds_only_the_real_host", () => {
  const card = confirmCard("https://good.com@evil.com/");
  expect(bold(card)).toEqual(["evil.com"]);
  expect(requests(card)).toEqual(["POST https://…@evil.com/"]);
  expect(card.textContent).not.toContain("good.com");
});

test("http_card_warns_when_host_is_a_reference", () => {
  for (const url of ["{{nodes.a.output.url}}/x", "https://{{nodes.a.output.host}}/x", "not a url"]) {
    const card = confirmCard(url);
    expect(bold(card)).toEqual([]);
    expect(within(card).getByText("목적지가 앞 노드 값으로 정해져요")).toBeTruthy();
    cleanup();
  }
});

test("http_card_shows_punycode_host_with_original", () => {
  const card = confirmCard("https://한글.kr/a");
  const host = new URL("https://한글.kr").host;
  expect(host).toContain("xn--");
  expect(bold(card)).toEqual([host]);
  expect(card.textContent).toContain("한글.kr");
});

test("http_card_plain_url_unchanged", () => {
  const card = confirmCard("https://x.test:8080/o/{{nodes.a.output.id}}?q=1");
  expect(bold(card)).toEqual(["x.test:8080"]);
  expect(requests(card)).toEqual(["POST https://x.test:8080/o/〈앞 노드 값〉?q=1"]);
  expect(within(card).queryByText("목적지가 앞 노드 값으로 정해져요")).toBeNull();
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
  expect(requests(card)).toEqual(["GET https://x.test", "POST https://x.test"]);
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

const SENDING = line({
  headerKeys: ["Authorization", "X-Data"],
  hasBody: true,
  sends: [
    { nodeId: "r", nodeLabel: "파일 읽기", output: "content" },
    { nodeId: "ai", nodeLabel: "", output: "summary" },
  ],
});

test("http_card_shows_headers_body_and_sent_values", async () => {
  const emit = captureEvent();
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  await emit({ id: "w1", lines: [SENDING, line({ nodeId: "g", method: "GET", url: "https://y.test" })] });
  const card = screen.getByRole("dialog");
  expect(within(card).getByText("머리말: Authorization, X-Data")).toBeTruthy();
  expect(within(card).getByText("본문 있음")).toBeTruthy();
  // 이름이 없는 노드는 id로.
  const sends = within(card).getByText("함께 보내는 값: 파일 읽기 의 content, ai 의 summary");
  expect(sends.className).toContain("text-danger-700");
  // 머리말·본문·보내는 값이 없는 줄은 그 글이 없다.
  expect(within(card).getAllByText(/머리말|본문 있음|함께 보내는 값/)).toHaveLength(3);
});

test("http_card_keeps_host_and_buttons_visible_for_long_url", async () => {
  const emit = captureEvent();
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  await emit({ id: "w1", lines: [line({ url: `https://evil.example.com/${"a".repeat(2000)}` })] });
  const card = screen.getByRole("dialog");
  expect(within(card).getByText("evil.example.com").tagName).toBe("STRONG");
  expect(within(card).getByRole("button", { name: "보내기" })).toBeTruthy();
  expect(within(card).getByRole("button", { name: "테스트 없이 넘어가기" })).toBeTruthy();
  expect(card.className).toContain("max-h-");
  expect(card.querySelector(".overflow-y-auto")).toBeTruthy();
  // 버튼은 스크롤 밖.
  expect(card.querySelector(".overflow-y-auto button")).toBeNull();
});

test("http_card_marks_changed_lines_and_blocks_send_for_a_second", async () => {
  const emit = captureEvent();
  const answer = vi.spyOn(api, "answerHttpConfirm").mockResolvedValue(true);
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  vi.useFakeTimers();
  await emit({ id: "w1", lines: LINES });
  expect(screen.queryByText("요청 내용이 바뀌었어요")).toBeNull();
  await emit({ id: "w1", lines: [SENDING] });
  expect(screen.getByText("요청 내용이 바뀌었어요")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "보내기" }));
  expect(answer).not.toHaveBeenCalled();
  act(() => vi.advanceTimersByTime(1000));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "보내기" })));
  expect(answer).toHaveBeenCalledWith("w1", true);
});

test("test_run_confirm_shows_headers_body_and_sent_values", async () => {
  const nodes: WfNode[] = [
    { id: "r", event: "local.file_read", label: "파일 읽기", config: {} },
    { id: "ai", event: "local.ai", config: {} },
    {
      id: "p",
      event: "http.request",
      config: {
        method: "post",
        url: "https://x.test/{{nodes.ai.output.summary}}",
        headers: { "X-Data": "{{nodes.ai.output.summary}}", Authorization: "Bearer t" },
        body: { text: "{{nodes.r.output.content}}", list: ["{{nodes.r.output.items.0.name}}"] },
      },
    },
  ];
  render(<Harness nodes={nodes} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  const card = screen.getByRole("dialog");
  expect(requests(card)).toEqual(["POST https://x.test/〈앞 노드 값〉"]);
  expect(within(card).getByText("머리말: Authorization, X-Data")).toBeTruthy();
  expect(within(card).getByText("본문 있음")).toBeTruthy();
  expect(within(card).getByText("함께 보내는 값: ai 의 summary, 파일 읽기 의 content, 파일 읽기 의 items.0.name")).toBeTruthy();
});

test("renders_github_line", async () => {
  const emit = captureEvent();
  await act(async () => render(<HttpTestCard workflowId="w1" />));
  const gh = line({
    label: "",
    method: "",
    url: "",
    github: { event: "github.add_labels", repo: "a/b", summary: "#{{nodes.t.output.n}}" },
    headerKeys: ["X"],
    sends: [{ nodeId: "t", nodeLabel: "", output: "n" }],
  });
  await emit({ id: "w1", lines: [gh] });
  const card = screen.getByRole("dialog");
  // 줄 머리는 노드 제목 — 만들기·라벨·댓글을 가린다. label이 없으면 제목을 두 번 쓰지 않는다.
  expect(requests(card)).toEqual(["GitHub 이슈에 라벨 추가 · a/b · #〈앞 노드 값〉"]);
  expect(within(card).getAllByText(/GitHub 이슈에 라벨 추가/)).toHaveLength(1);
  expect(within(card).getByText("함께 보내는 값: t 의 n")).toBeTruthy();
  expect(within(card).queryByText(/머리말/)).toBeNull();
});

test("test_run_asks_for_github_write_only", async () => {
  vi.spyOn(api, "pickPath").mockResolvedValue("/tmp/a.txt");
  const runNow = vi.spyOn(api, "runNow").mockResolvedValue("r1");
  const gh = (event: string): WfNode => ({ id: "g", event, config: { owner: "a", repo: "b", issue_number: "{{nodes.t.output.n}}" } });
  const r = render(<Harness nodes={[gh("github.add_labels")]} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  const card = screen.getByRole("dialog", { name: "테스트로 실제 요청을 보낼까요?" });
  expect(requests(card)).toEqual(["GitHub 이슈에 라벨 추가 · a/b · #〈앞 노드 값〉"]);
  expect(runNow).not.toHaveBeenCalled();

  r.rerender(<Harness nodes={[gh("github.find_issue")]} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  expect(runNow).toHaveBeenCalledTimes(1);
});

test("test_run_github_line_shows_numeric_issue_number", async () => {
  vi.spyOn(api, "pickPath").mockResolvedValue("/tmp/a.txt");
  vi.spyOn(api, "runNow").mockResolvedValue("r1");
  const n: WfNode = { id: "u", event: "github.update_issue", config: { owner: "a", repo: "b", issue_number: 12, state: "closed" } };
  render(<Harness nodes={[n]} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  expect(requests(screen.getByRole("dialog"))).toEqual(["GitHub 이슈 수정 · a/b · #12"]);
});

test("test_run_github_pr_lines_use_title_or_pull_number", async () => {
  vi.spyOn(api, "pickPath").mockResolvedValue("/tmp/a.txt");
  const runNow = vi.spyOn(api, "runNow").mockResolvedValue("r1");
  const base = { owner: "a", repo: "b" };
  const nodes: WfNode[] = [
    { id: "p", event: "github.create_pull_request", config: { ...base, title: "T" } },
    { id: "s", event: "github.submit_pr_review", config: { ...base, pull_number: 7 } },
  ];
  const r = render(<Harness nodes={nodes} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  const lines = requests(screen.getByRole("dialog"));
  expect(lines).toHaveLength(2);
  expect(lines[0]).toMatch(/ · a\/b · T$/);
  expect(lines[1]).toMatch(/ · a\/b · #7$/);
  expect(runNow).not.toHaveBeenCalled();

  r.rerender(<Harness nodes={[{ id: "f", event: "github.find_pull_request", config: base }]} />);
  await act(async () => fireEvent.click(screen.getByText("run")));
  expect(runNow).toHaveBeenCalledTimes(1);
});
