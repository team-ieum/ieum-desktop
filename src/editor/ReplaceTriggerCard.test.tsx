// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { setLang } from "../i18n";
import { ReplaceTriggerCard } from "./ReplaceTriggerCard";

beforeAll(() => setLang("ko"));
afterEach(cleanup);

const REFS = "뒤 노드 2개가 이 시작 조건의 결과를 쓰고 있어요. 바꾸면 그 참조가 끊길 수 있어요.";
const FOLDER = "파일 노드가 쓸 수 있는 폴더도 함께 바뀌어요.";
const OFF = "바꾸면 워크플로우가 꺼져요. 설정을 확인한 뒤 다시 켜 주세요.";
const card = (o: { refs?: number; folder?: boolean; turnsOff?: boolean; onAnswer?: (ok: boolean) => void } = {}) =>
  render(<ReplaceTriggerCard oldName="매일" newName="수동 실행" refs={o.refs ?? 0} folder={!!o.folder} turnsOff={!!o.turnsOff} onAnswer={o.onAnswer ?? (() => {})} />);

test("card_lines_follow_flags", () => {
  card();
  for (const s of [REFS, FOLDER, OFF]) expect(screen.queryByText(s)).toBeNull();
  cleanup();
  card({ refs: 2, folder: true, turnsOff: true });
  for (const s of [REFS, FOLDER, OFF]) expect(screen.getByText(s)).toBeTruthy();
  // 처음 포커스는 [취소] — Enter 한 번으로 바뀌지 않게.
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "취소" }));
});

test("card_escape_cancels_without_bubbling", () => {
  const onAnswer = vi.fn();
  const outside = vi.fn();
  document.addEventListener("keydown", outside);
  try {
    card({ onAnswer });
    fireEvent.keyDown(screen.getByRole("button", { name: "취소" }), { key: "Escape" });
  } finally {
    document.removeEventListener("keydown", outside);
  }
  expect(onAnswer).toHaveBeenCalledWith(false);
  expect(outside).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "바꾸기" }));
  expect(onAnswer).toHaveBeenLastCalledWith(true);
});

test("card_shows_custom_old_label", () => {
  render(<ReplaceTriggerCard oldName="매일 아침 9시" newName="수동 실행" refs={0} folder={false} turnsOff={false} onAnswer={() => {}} />);
  const title = "시작 조건 '매일 아침 9시'을(를) '수동 실행'(으)로 바꿀까요?";
  expect(screen.getByRole("dialog", { name: title })).toBeTruthy();
  expect(screen.getByRole("heading", { name: title })).toBeTruthy();
});

test("card_escape_from_body_cancels", () => {
  const onAnswer = vi.fn();
  const outside = vi.fn();
  document.addEventListener("keydown", outside);
  try {
    card({ onAnswer });
    // 카드 여백·캔버스를 눌러 포커스가 body로 빠져도 Esc는 카드가 받는다.
    (document.activeElement as HTMLElement).blur();
    fireEvent.keyDown(document.body, { key: "Escape" });
  } finally {
    document.removeEventListener("keydown", outside);
  }
  expect(onAnswer).toHaveBeenCalledWith(false);
  expect(outside).not.toHaveBeenCalled();
});

test("card_title_keeps_special_patterns_in_old_label", () => {
  render(<ReplaceTriggerCard oldName="a$&{new}$1b" newName="수동 실행" refs={0} folder={false} turnsOff={false} onAnswer={() => {}} />);
  expect(screen.getByRole("heading", { name: "시작 조건 'a$&{new}$1b'을(를) '수동 실행'(으)로 바꿀까요?" })).toBeTruthy();
});
