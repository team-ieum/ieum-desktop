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
