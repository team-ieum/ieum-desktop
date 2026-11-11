// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test } from "vitest";
import { setLang } from "../i18n";
import { RunPill } from "./History";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

// 재리뷰 N-2: 끝 시각 없는 기록(실행 도중 저장)은 "중단됨"이 아니라 "실행 중"
test("run_pill_unfinished_shows_running", () => {
  render(<RunPill status="cancelled" finishedAt="" />);
  expect(screen.getByText("실행 중")).toBeTruthy();
  cleanup();
  render(<RunPill status="cancelled" finishedAt="2026-11-11T00:00:00Z" />);
  expect(screen.getByText("중단됨")).toBeTruthy();
});
