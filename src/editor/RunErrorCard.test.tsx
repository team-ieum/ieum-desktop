// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import type { WfNode } from "../api";
import { setLang } from "../i18n";
import { emptyLive, type Live } from "../stores/runs";
import { RunErrorCard } from "./RunErrorCard";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

const nodes = [
  { id: "t", event: "manual_trigger", type: "TRIGGER", config: {} },
  { id: "m", event: "http_request", type: "ACTION", config: {}, label: "m 제목" },
] as unknown as WfNode[];
const failed = (runId = "r1", codes: Record<string, string> = {}): Live => ({
  ...emptyLive("w"),
  runId,
  status: "failed",
  nodes: { t: "succeeded", m: "failed" },
  errors: { m: "x" },
  codes,
});

test("shows_first_failed_node", () => {
  render(<RunErrorCard live={failed()} nodes={nodes} right={0} onShow={() => {}} />);
  expect(screen.getByText("2")).toBeTruthy();
  expect(screen.getByText("m 제목에서 멈췄어요")).toBeTruthy();
  expect(screen.getByRole("alert").className).toContain("ieum-grow-br");
});

test("buttons_call_back", () => {
  const onShow = vi.fn();
  const onOpenIntegrations = vi.fn();
  const live = failed("r1", { m: "GITHUB_AUTH" });
  const { rerender } = render(<RunErrorCard live={live} nodes={nodes} right={0} onShow={onShow} onOpenIntegrations={onOpenIntegrations} />);
  fireEvent.click(screen.getByText("설정 > 연동 열기"));
  fireEvent.click(screen.getByText("노드 보기"));
  expect(onOpenIntegrations).toHaveBeenCalledTimes(1);
  expect(onShow).toHaveBeenCalledWith("m");
  rerender(<RunErrorCard live={live} nodes={nodes} right={0} onShow={onShow} />);
  expect(screen.queryByText("설정 > 연동 열기")).toBeNull();
});

test("ai_limit_opens_ai_settings", () => {
  const onOpenAiSettings = vi.fn();
  render(<RunErrorCard live={failed("r1", { m: "AI_KEY_LIMIT" })} nodes={nodes} right={0} onShow={() => {}} onOpenIntegrations={() => {}} onOpenAiSettings={onOpenAiSettings} />);
  fireEvent.click(screen.getByText("설정 > AI 열기"));
  expect(onOpenAiSettings).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("설정 > 연동 열기")).toBeNull();
});

test("narrow_canvas_shrinks_card", () => {
  render(<RunErrorCard live={failed("r1", { m: "AI_KEY_LIMIT" })} nodes={nodes} right={980} onShow={() => {}} onOpenAiSettings={() => {}} />);
  const card = screen.getByRole("alert");
  // 채팅창·노드 패널이 넓게 덮어도 남은 폭 안에 들어가 왼쪽이 잘리지 않는다.
  expect(card.style.maxWidth).toBe("calc(100% - 996px)");
  expect(screen.getByText("설정 > AI 열기").parentElement?.className).toContain("flex-wrap");
});

// Editor처럼 실행마다 key를 준다.
const card = (live: Live) => <RunErrorCard key={live.runId ?? ""} live={live} nodes={nodes} right={0} onShow={() => {}} />;

test("dismiss_is_per_run", () => {
  const live = failed();
  const { rerender } = render(card(live));
  fireEvent.click(screen.getByLabelText("오류 알림 닫기"));
  expect(screen.queryByRole("alert")).toBeNull();
  rerender(card(live));
  expect(screen.queryByRole("alert")).toBeNull();
  rerender(card(failed("r2")));
  expect(screen.getByRole("alert")).toBeTruthy();
});

test("dismiss_with_motion_then_next_run_grows_again", () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  vi.useFakeTimers();
  try {
    const { rerender } = render(card(failed()));
    fireEvent.click(screen.getByLabelText("오류 알림 닫기"));
    expect(screen.getByRole("alert").className).toContain("ieum-slide-out");
    act(() => void vi.advanceTimersByTime(160));
    expect(screen.queryByRole("alert")).toBeNull();
    rerender(card(failed("r2")));
    expect(screen.getByRole("alert").className).toContain("ieum-grow-br");
    expect(screen.getByRole("alert").className).not.toContain("ieum-slide-out");
  } finally {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});

test("new_run_during_close_is_not_dismissed", () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  vi.useFakeTimers();
  try {
    const { rerender } = render(card(failed()));
    fireEvent.click(screen.getByLabelText("오류 알림 닫기"));
    rerender(card(failed("r2"))); // 빠지는 0.16초 안에 다음 실행이 실패
    act(() => void vi.advanceTimersByTime(160));
    expect(screen.getByRole("alert").className).toContain("ieum-grow-br");
  } finally {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});

test("cancelled_run_shows_nothing", () => {
  render(<RunErrorCard live={{ ...failed(), status: "cancelled" }} nodes={nodes} right={0} onShow={() => {}} />);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("right_offset_applied", () => {
  render(<RunErrorCard live={failed()} nodes={nodes} right={592} onShow={() => {}} />);
  expect(screen.getByRole("alert").style.right).toBe("592px");
});
