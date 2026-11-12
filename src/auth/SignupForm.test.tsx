// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api } from "../api";
import { setLang } from "../i18n";
import { useWeb } from "../stores/web";
import { SignupForm } from "./SignupForm";

const EMAIL = "new@b.c";
const PW = "Secret1!pw";
const ACCOUNT = { email: EMAIL, serverUrl: "https://green.kiacms.co.kr" };

beforeAll(() => setLang("ko"));
beforeEach(() => useWeb.setState({ account: null }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const click = (name: string | RegExp) => act(async () => fireEvent.click(screen.getByRole("button", { name })));
const type = (label: string, value: string) => act(async () => fireEvent.change(screen.getByLabelText(label), { target: { value } }));
const digits = () => [1, 2, 3, 4, 5, 6].map((i) => screen.getByLabelText(`인증번호 ${i}`) as HTMLInputElement);

async function toStep2() {
  await act(async () => {
    render(<SignupForm onBack={() => {}} />);
  });
  await type("이메일", EMAIL);
  await click("인증번호 받기");
}

async function toStep3() {
  vi.spyOn(api, "sendCode").mockResolvedValue();
  vi.spyOn(api, "verifyCode").mockResolvedValue();
  await toStep2();
  await act(async () => fireEvent.paste(digits()[0], { clipboardData: { getData: () => "123456" } }));
  await click("확인");
}

async function fillInfo(pw = PW) {
  await type("이름", "홍길동");
  await type("비밀번호", pw);
  await type("비밀번호 확인", pw);
  await click("가입하기");
}

test("signup_happy_path_calls_in_order", async () => {
  const order: string[] = [];
  const send = vi.spyOn(api, "sendCode").mockImplementation(async () => void order.push("send"));
  const verify = vi.spyOn(api, "verifyCode").mockImplementation(async () => void order.push("verify"));
  const register = vi.spyOn(api, "register").mockImplementation(async () => (order.push("register"), { ...ACCOUNT, name: "" }) as never);
  const login = vi.spyOn(api, "webLogin").mockImplementation(async () => (order.push("login"), ACCOUNT) as never);
  await toStep2();
  await act(async () => fireEvent.paste(digits()[0], { clipboardData: { getData: () => "123456" } }));
  await click("확인");
  await fillInfo();
  expect(send).toHaveBeenCalledWith(EMAIL);
  expect(verify).toHaveBeenCalledWith(EMAIL, "123456");
  expect(register).toHaveBeenCalledWith(EMAIL, PW, "홍길동");
  expect(login).toHaveBeenCalledWith(EMAIL, PW);
  expect(order).toEqual(["send", "verify", "register", "login"]);
  // 가입 성공 = 로그인 상태 → 관문이 닫힌다.
  expect(useWeb.getState().account).toEqual(ACCOUNT);
});

test("signup_shows_distinct_errors", async () => {
  const texts: string[] = [];
  for (const code of ["EMAIL_TAKEN", "CODE_INVALID", "NETWORK", "BAD_INPUT", "AUTH_LOCKED"]) {
    vi.spyOn(api, "register").mockRejectedValue({ code });
    await toStep3();
    await fillInfo();
    const alert = screen.getByRole("alert").textContent ?? "";
    expect(alert).not.toContain(PW);
    expect(document.body.textContent).not.toContain(PW); // 칸 값 말고 화면 문구
    texts.push(alert);
    if (code === "EMAIL_TAKEN") expect((screen.getByLabelText("이메일") as HTMLInputElement).readOnly).toBe(false); // ①로 돌아감
    if (code === "CODE_INVALID") expect(digits()[0].value).toBe(""); // ②로 돌아감
    cleanup();
    vi.restoreAllMocks();
  }
  expect(texts[0]).toBe("이미 가입된 이메일이에요.");
  expect(texts[1]).toBe("인증번호가 맞지 않거나 만료됐어요.");
  expect(texts[2]).toBe("서버에 연결하지 못했어요");
  expect(new Set(texts).size).toBe(texts.length);
});

test("signup_blocks_weak_password_before_request", async () => {
  const register = vi.spyOn(api, "register").mockResolvedValue({} as never);
  await toStep3();
  await fillInfo("abcdefgh");
  expect(register).not.toHaveBeenCalled();
  // 항목별 표시
  expect(screen.getByText("영문 대문자 1자 이상").closest("li")?.dataset.ok).toBe("false");
  expect(screen.getByText("8~72자").closest("li")?.dataset.ok).toBe("true");
  // 확인이 다르면 막는다.
  await type("비밀번호", PW);
  await type("비밀번호 확인", PW + "x");
  await click("가입하기");
  expect(register).not.toHaveBeenCalled();
  expect(screen.getByText("비밀번호가 서로 달라요.")).toBeTruthy();
});

test("code_paste_fills_six_and_ignores_non_digits", async () => {
  vi.spyOn(api, "sendCode").mockResolvedValue();
  await toStep2();
  const d = digits();
  await act(async () => fireEvent.change(d[0], { target: { value: "a" } }));
  expect(d[0].value).toBe("");
  await act(async () => fireEvent.change(d[0], { target: { value: "7" } }));
  expect(d[0].value).toBe("7");
  await act(async () => fireEvent.paste(d[2], { clipboardData: { getData: () => "12-34 56" } }));
  expect(digits().map((x) => x.value).join("")).toBe("123456");
});

test("resend_countdown_starts_at_three_minutes", async () => {
  vi.useFakeTimers();
  const send = vi.spyOn(api, "sendCode").mockResolvedValue();
  await toStep2();
  const resend = () => screen.getByRole("button", { name: /인증번호 다시 받기/ }) as HTMLButtonElement;
  expect(resend().textContent).toContain("3:00");
  expect(resend().disabled).toBe(true);
  await act(async () => vi.advanceTimersByTime(19_000));
  expect(resend().textContent).toContain("2:41");
  await act(async () => vi.advanceTimersByTime(161_000));
  expect(resend().disabled).toBe(false);
  await act(async () => fireEvent.click(resend()));
  expect(send).toHaveBeenCalledTimes(2);
  expect(resend().textContent).toContain("3:00");
});
