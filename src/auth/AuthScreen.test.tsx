// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api } from "../api";
import { setLang } from "../i18n";
import { useSettings } from "../stores/settings";
import { AuthScreen } from "./AuthScreen";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

const S = { language: "ko" as const, autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr" };

beforeAll(() => {
  setLang("ko");
  useSettings.setState({ settings: S });
});
afterEach(cleanup);

test("login_failure_keeps_password_out_of_dom", async () => {
  const login = vi.spyOn(api, "webLogin").mockRejectedValue({ code: "AUTH_FAILED" });
  await act(async () => {
    render(<AuthScreen />);
  });
  expect(await screen.findByText(/v0\.2\.0/)).toBeTruthy();
  await act(async () => fireEvent.change(screen.getByLabelText("이메일"), { target: { value: "a@b.c" } }));
  const pw = screen.getByLabelText("비밀번호") as HTMLInputElement;
  await act(async () => fireEvent.change(pw, { target: { value: "pw-secret-9" } }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "로그인" })));
  expect(login).toHaveBeenCalledWith("a@b.c", "pw-secret-9");
  expect(screen.getByText("이메일 또는 비밀번호가 맞지 않아요.")).toBeTruthy();
  expect(pw.value).toBe("");
  expect(document.body.innerHTML).not.toContain("pw-secret-9");
});

test("signup_link_sends_code_and_shows_error", async () => {
  const send = vi.spyOn(api, "sendCode").mockRejectedValue({ code: "NETWORK" });
  await act(async () => {
    render(<AuthScreen />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "회원가입" })));
  await act(async () => fireEvent.change(screen.getByLabelText("이메일"), { target: { value: "new@b.c" } }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "인증번호 받기" })));
  expect(send).toHaveBeenCalledWith("new@b.c");
  expect(screen.getByText("서버에 연결하지 못했어요")).toBeTruthy();
  // 로그인으로 돌아갈 수 있다.
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "로그인" })));
  expect(screen.getByLabelText("비밀번호")).toBeTruthy();
});

test("server_link_saves_address", async () => {
  const set = vi.spyOn(api, "setSettings").mockRejectedValue("허용되지 않은 서버 주소예요");
  vi.spyOn(api, "getSettings").mockResolvedValue(S);
  await act(async () => {
    render(<AuthScreen />);
  });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: /서버: green\.kiacms\.co\.kr/ })));
  const url = screen.getByLabelText("서버 주소") as HTMLInputElement;
  expect(url.value).toBe(S.serverUrl);
  await act(async () => fireEvent.change(url, { target: { value: "https://evil.example" } }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "저장" })));
  expect(set).toHaveBeenCalledWith({ ...S, serverUrl: "https://evil.example" });
  expect(screen.getByText("허용되지 않은 서버 주소예요")).toBeTruthy();
});
