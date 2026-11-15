// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { api, type UpdateAvailable, type UpdateProgress } from "../api";
import { setLang } from "../i18n";
import { UpdateToast } from "./UpdateToast";

let emit: (p: UpdateAvailable) => void;
let progress: (p: UpdateProgress) => void;

beforeAll(() => setLang("ko"));
beforeEach(() => {
  vi.spyOn(api, "pendingUpdate").mockResolvedValue(null);
  vi.spyOn(api, "onUpdateAvailable").mockImplementation(async (cb) => ((emit = cb), () => {}));
  vi.spyOn(api, "onUpdateProgress").mockImplementation(async (cb) => ((progress = cb), () => {}));
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const mount = async () => {
  await act(async () => {
    render(<UpdateToast />);
  });
};

test("remembered_version_shows_on_mount", async () => {
  vi.mocked(api.pendingUpdate).mockResolvedValue("1.0.1");
  await mount();
  expect(screen.getByText("새 버전 1.0.1이 나왔어요")).toBeTruthy();
  expect(screen.getByText("업데이트하면 앱이 다시 시작돼요.")).toBeTruthy();
});

test("later_hides_same_version_until_manual_check", async () => {
  await mount();
  expect(screen.queryByRole("button", { name: "업데이트" })).toBeNull();
  await act(async () => emit({ version: "1.0.2", manual: false }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "나중에" })));
  expect(screen.queryByText(/1\.0\.2/)).toBeNull();
  // 6시간마다 다시 확인해도 같은 버전은 이번 실행 동안 다시 안 띄운다.
  await act(async () => emit({ version: "1.0.2", manual: false }));
  expect(screen.queryByText(/1\.0\.2/)).toBeNull();
  // 다시 마운트돼도(설정 화면 오가기) 그대로.
  cleanup();
  vi.mocked(api.pendingUpdate).mockResolvedValue("1.0.2");
  await mount();
  expect(screen.queryByText(/1\.0\.2/)).toBeNull();
  // 설정에서 직접 확인하면 다시 띄운다.
  await act(async () => emit({ version: "1.0.2", manual: true }));
  expect(screen.getByText("새 버전 1.0.2이 나왔어요")).toBeTruthy();
});

test("newer_version_after_later_shows_again", async () => {
  await mount();
  await act(async () => emit({ version: "1.0.3", manual: false }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "나중에" })));
  await act(async () => emit({ version: "1.0.4", manual: false }));
  expect(screen.getByText("새 버전 1.0.4이 나왔어요")).toBeTruthy();
});

test("install_shows_progress_then_error", async () => {
  let fail!: (e: unknown) => void;
  const install = vi.spyOn(api, "installUpdateAndRestart").mockImplementation(() => new Promise((_, rej) => (fail = rej)));
  vi.spyOn(console, "error").mockImplementation(() => {});
  await mount();
  await act(async () => emit({ version: "1.0.5", manual: false }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "업데이트" })));
  expect(install).toHaveBeenCalledOnce();
  const busy = screen.getByRole("button", { name: "받는 중…" }) as HTMLButtonElement;
  expect(busy.disabled).toBe(true);
  expect((screen.getByRole("button", { name: "나중에" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => fail("업데이트를 받지 못했어요."));
  expect(screen.getByText("업데이트를 받지 못했어요. 네트워크를 확인하고 다시 눌러 주세요.")).toBeTruthy();
  expect((screen.getByRole("button", { name: "업데이트" }) as HTMLButtonElement).disabled).toBe(false);
});

test("download_progress_then_installing_and_kept_across_remount", async () => {
  vi.spyOn(api, "installUpdateAndRestart").mockImplementation(() => new Promise(() => {}));
  await mount();
  await act(async () => emit({ version: "1.0.6", manual: false }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "업데이트" })));
  await act(async () => progress({ downloaded: 3_355_443, total: 6_920_601, installing: false }));
  expect(screen.getByText("3.2 / 6.6MB · 48%")).toBeTruthy();
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("48");
  // 화면이 바뀌어 다시 그려져도 받는 중 그대로(두 번 누를 수 없다).
  cleanup();
  vi.mocked(api.pendingUpdate).mockResolvedValue("1.0.6");
  await mount();
  expect((screen.getByRole("button", { name: "받는 중…" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText("3.2 / 6.6MB · 48%")).toBeTruthy();
  await act(async () => progress({ downloaded: 0, total: null, installing: true }));
  expect(screen.getByRole("button", { name: "설치 중…" })).toBeTruthy();
});

test("up_to_date_closes_the_toast", async () => {
  vi.spyOn(api, "installUpdateAndRestart").mockRejectedValue("UP_TO_DATE");
  await mount();
  await act(async () => emit({ version: "1.0.7", manual: false }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "업데이트" })));
  expect(screen.queryByText(/1\.0\.7/)).toBeNull();
});
