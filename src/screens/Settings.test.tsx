// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { api } from "../api";
import { setLang } from "../i18n";
import { useSettings } from "../stores/settings";
import { useWeb } from "../stores/web";
import { Settings } from "./Settings";

beforeAll(() => {
  setLang("ko");
  useSettings.setState({ settings: { language: "ko", autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr" } });
  vi.spyOn(api, "detectProviders").mockResolvedValue({ clis: [], apiKeys: [] });
  vi.spyOn(api, "keychainStatus").mockResolvedValue({ anthropic: "missing", openai: "missing", gemini: "missing", "ieum-session": "missing" });
  useWeb.setState({ account: null });
});
afterEach(cleanup);

test("settings_clears_password_and_key_inputs", async () => {
  const setKey = vi.spyOn(api, "setApiKey").mockResolvedValue();
  const login = vi.spyOn(api, "webLogin").mockRejectedValue({ code: "AUTH_FAILED" });
  await act(async () => {
    render(<Settings />);
  });

  const key = screen.getByLabelText("Anthropic API 키") as HTMLInputElement;
  await act(async () => fireEvent.change(key, { target: { value: "sk-secret" } }));
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "저장" })[0]));
  expect(setKey).toHaveBeenCalledWith("anthropic", "sk-secret");
  expect(key.value).toBe("");

  await act(async () => fireEvent.change(screen.getByLabelText("이메일"), { target: { value: "a@b.c" } }));
  const pw = screen.getByLabelText("비밀번호") as HTMLInputElement;
  await act(async () => fireEvent.change(pw, { target: { value: "pw123" } }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "로그인" })));
  expect(login).toHaveBeenCalledWith("a@b.c", "pw123");
  expect(pw.value).toBe("");
  expect(screen.getByText(/이메일 또는 비밀번호/)).toBeTruthy();
});

test("settings_server_url_fills_late_and_clears_expired", async () => {
  const S = { language: "ko" as const, autostart: false, firstRunDone: true, serverUrl: "https://green.kiacms.co.kr" };
  useSettings.setState({ settings: null });
  useWeb.setState({ account: null, expired: true });
  vi.spyOn(api, "setSettings").mockResolvedValue();
  vi.spyOn(api, "getSettings").mockResolvedValue({ ...S, serverUrl: "http://localhost:8080" });
  vi.spyOn(api, "webAccount").mockResolvedValue(null);
  await act(async () => {
    render(<Settings />);
  });
  const url = screen.getByLabelText("서버 주소") as HTMLInputElement;
  const saveBtn = () => url.closest("form")!.querySelector("button")!;
  expect(url.value).toBe("");
  expect(saveBtn().disabled).toBe(true);

  // 설정을 늦게 읽으면 그 값으로 채운다. 비우면 저장 못 한다.
  await act(async () => useSettings.setState({ settings: S }));
  expect(url.value).toBe(S.serverUrl);
  await act(async () => fireEvent.change(url, { target: { value: " " } }));
  expect(saveBtn().disabled).toBe(true);

  // 서버 주소를 바꾸면 만료 표시를 푼다.
  expect(screen.getByText(/로그인이 만료됐어요/)).toBeTruthy();
  await act(async () => fireEvent.change(url, { target: { value: "http://localhost:8080" } }));
  await act(async () => fireEvent.click(saveBtn()));
  expect(screen.queryByText(/로그인이 만료됐어요/)).toBeNull();
});
