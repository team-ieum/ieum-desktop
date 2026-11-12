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

test("settings_clears_key_input", async () => {
  const setKey = vi.spyOn(api, "setApiKey").mockResolvedValue();
  await act(async () => {
    render(<Settings />);
  });

  const key = screen.getByLabelText("Anthropic API 키") as HTMLInputElement;
  await act(async () => fireEvent.change(key, { target: { value: "sk-secret" } }));
  await act(async () => fireEvent.click(screen.getAllByRole("button", { name: "저장" })[0]));
  expect(setKey).toHaveBeenCalledWith("anthropic", "sk-secret");
  expect(key.value).toBe("");
});
