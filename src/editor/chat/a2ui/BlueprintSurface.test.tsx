// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Blueprint } from "../../../api";
import BlueprintSurface from "./BlueprintSurface";
import { Changed } from "./views";

afterEach(cleanup);

const bp: Blueprint = {
  status: "active",
  version: 1,
  buildPending: false,
  buildAttempts: 0,
  slots: {},
  components: [
    { id: "root", component: "Blueprint", title: "영수증 정리", children: ["pick"] },
    {
      id: "pick",
      component: "ChipSelect",
      label: "뽑을 것",
      multiple: true,
      options: [
        { value: "date", label: "날짜" },
        { value: "shop", label: "가게" },
      ],
      value: { "@path": "/pick" },
    },
  ],
  dataModel: { pick: ["date"] },
};

it("renders_chip_select_from_v1_surface_and_reports_data", async () => {
  const onData = vi.fn();
  render(
    <BlueprintSurface blueprint={bp} locked={false} busy={false} onSubmit={vi.fn()} onRetry={vi.fn()} onPickSlot={vi.fn()} onData={onData} />,
  );
  expect(await screen.findByRole("group", { name: "영수증 정리" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "날짜", pressed: true })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "가게" }));
  expect(onData).toHaveBeenLastCalledWith({ pick: ["date", "shop"] });
});

it("StrictMode에서도 리렌더 뒤 카드가 남고 onData가 불리며, 스냅샷은 서로 다른 객체다", async () => {
  const onData = vi.fn();
  const ui = (
    <StrictMode>
      <BlueprintSurface blueprint={bp} locked={false} busy={false} onSubmit={vi.fn()} onRetry={vi.fn()} onPickSlot={vi.fn()} onData={onData} />
    </StrictMode>
  );
  const { rerender } = render(ui);
  await screen.findByRole("group", { name: "영수증 정리" });
  rerender(ui);
  expect(screen.getByRole("group", { name: "영수증 정리" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "가게" }));
  fireEvent.click(screen.getByRole("button", { name: "날짜" }));
  const [a, b] = [onData.mock.calls[0][0], onData.mock.calls[onData.mock.calls.length - 1][0]];
  expect(a).toEqual({ pick: ["date", "shop"] });
  expect(b).toEqual({ pick: ["shop"] });
  expect(a).not.toBe(b);
});

const slot = (event: string, field: string, path: string | null) => ({ event, field, path, pickedVersion: path ? 1 : null, boundNode: null });
const go = { id: "go", component: "SubmitButton" };

/** 시안 B 영수증 설계서 + Select 하나 — 8종을 모두 담는다. */
const receipt: Blueprint = {
  status: "active",
  version: 1,
  buildPending: false,
  buildAttempts: 0,
  slots: {
    watch: slot("local.folder_new_file", "folder", "~/Downloads"),
    dest: slot("local.file_move", "baseFolder", null),
  },
  components: [
    { id: "root", component: "Blueprint", title: "영수증 정리", children: ["flow", "pick", "fmt", "pdf", "name", "watch", "dest", "go"] },
    {
      id: "flow",
      component: "FlowPreview",
      steps: [
        { label: "새 파일", event: "local.folder_new_file" },
        { label: "AI 읽기", event: "local.ai" },
        { label: "이름 바꾸기", event: "local.file_rename" },
        { label: "옮기기" },
      ],
    },
    {
      id: "pick",
      component: "ChipSelect",
      label: "AI가 뽑을 것",
      multiple: true,
      options: [
        { value: "date", label: "날짜" },
        { value: "shop", label: "가게 이름" },
        { value: "amount", label: "금액" },
        { value: "card", label: "카드사" },
      ],
      value: { "@path": "/pick" },
    },
    {
      id: "fmt",
      component: "Select",
      label: "날짜 형식",
      options: [
        { value: "ymd", label: "2026-11-24" },
        { value: "dmy", label: "24.11.2026" },
      ],
      value: { "@path": "/fmt" },
    },
    { id: "pdf", component: "Checkbox", label: "PDF 파일만", value: { "@path": "/pdf" } },
    { id: "name", component: "TextInput", label: "새 이름", hint: "예: 2026-11-24_스타벅스_5600.pdf", value: { "@path": "/name" } },
    { id: "watch", component: "AppGate", slot: "watch", target: { event: "local.folder_new_file", field: "folder" } },
    { id: "dest", component: "AppGate", slot: "dest", target: { event: "local.file_move", field: "baseFolder" } },
    go,
  ],
  dataModel: { pick: ["date", "shop", "amount"], fmt: "ymd", pdf: true, name: "날짜_가게_금액.pdf" },
};

const surface = (b: Blueprint, over: Partial<Parameters<typeof BlueprintSurface>[0]> = {}) => (
  <BlueprintSurface blueprint={b} locked={false} busy={false} onSubmit={vi.fn()} onPickSlot={vi.fn()} onData={vi.fn()} {...over} />
);

it("renders_all_eight_components_from_fixture", async () => {
  render(surface(receipt));
  const card = await screen.findByRole("group", { name: "영수증 정리" });
  expect(within(card).getByText("AI가 조립")).toBeTruthy();
  expect(within(screen.getByRole("list")).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["새 파일", "AI 읽기", "이름 바꾸기", "옮기기"]);
  expect(screen.getByRole("button", { name: "금액", pressed: true })).toBeTruthy();
  expect(screen.getByRole("button", { name: "카드사", pressed: false })).toBeTruthy();
  expect((screen.getByRole("combobox", { name: "날짜 형식" }) as HTMLSelectElement).value).toBe("ymd");
  expect((screen.getByRole("checkbox", { name: "PDF 파일만" }) as HTMLInputElement).checked).toBe(true);
  const name = screen.getByRole("textbox", { name: "새 이름" }) as HTMLInputElement;
  expect(name.value).toBe("날짜_가게_금액.pdf");
  expect(name.maxLength).toBe(200);
  expect(screen.getByText("예: 2026-11-24_스타벅스_5600.pdf")).toBeTruthy();
  expect(screen.getAllByText("앱 전용")).toHaveLength(2);
  expect(screen.getByText("~/Downloads")).toBeTruthy();
  expect(screen.getByRole("button", { name: "이대로 만들기" })).toBeTruthy();
});

it("app_gate_label_comes_from_target_not_llm", async () => {
  const b = { ...receipt, components: receipt.components.map((c) => (c.id === "watch" ? { ...c, label: "LLM 라벨" } : c)) };
  render(surface(b));
  expect(await screen.findByRole("group", { name: "감시할 폴더" })).toBeTruthy();
  expect(screen.getByRole("group", { name: "기준 폴더" })).toBeTruthy();
  expect(screen.queryByText("LLM 라벨")).toBeNull();
});

it("submit_disabled_until_all_slots_picked", async () => {
  render(surface(receipt));
  expect(((await screen.findByRole("button", { name: "이대로 만들기" })) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText("앱 전용 칸을 모두 고르면 만들 수 있어요")).toBeTruthy();
});

it("submit_with_no_slots_is_enabled", async () => {
  const onSubmit = vi.fn();
  const b: Blueprint = { ...bp, components: [{ ...bp.components[0], children: ["pick", "go"] }, bp.components[1], go] };
  render(surface(b, { onSubmit }));
  const btn = (await screen.findByRole("button", { name: "이대로 만들기" })) as HTMLButtonElement;
  expect(btn.disabled).toBe(false);
  expect(screen.queryByText("앱 전용 칸을 모두 고르면 만들 수 있어요")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "가게" }));
  fireEvent.click(btn);
  expect(onSubmit).toHaveBeenCalledWith({ pick: ["date", "shop"] });
});

it("locked_disables_inputs_and_hides_submit", async () => {
  const b: Blueprint = { ...receipt, status: "submitted", buildPending: false };
  render(surface(b, { locked: true }));
  expect(await screen.findByText("만들었어요")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "이대로 만들기" })).toBeNull();
  expect(screen.queryByText("앱 전용 칸을 모두 고르면 만들 수 있어요")).toBeNull();
  expect((screen.getByRole("checkbox", { name: "PDF 파일만" }) as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("textbox", { name: "새 이름" }) as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("combobox", { name: "날짜 형식" }) as HTMLSelectElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "날짜" }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "폴더 고르기" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "이어서 만들기" })).toBeNull();
});

it("cancelled_shows_cancelled", async () => {
  render(surface({ ...receipt, status: "cancelled" }, { locked: true }));
  expect(await screen.findByText("취소됐어요")).toBeTruthy();
});

it("retry_shows_when_build_pending", async () => {
  const onRetry = vi.fn();
  const b: Blueprint = { ...receipt, status: "submitted", buildPending: true };
  const { rerender } = render(surface(b, { locked: true, onRetry }));
  fireEvent.click(await screen.findByRole("button", { name: "이어서 만들기" }));
  expect(onRetry).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("만들었어요")).toBeNull();
  rerender(surface(b, { locked: true, onRetry, busy: true }));
  expect((screen.getByRole("button", { name: "이어서 만들기" }) as HTMLButtonElement).disabled).toBe(true);
  rerender(surface(b, { locked: true }));
  expect(screen.queryByRole("button", { name: "이어서 만들기" })).toBeNull();
});

it("busy_disables_inputs", async () => {
  render(surface(receipt, { busy: true }));
  expect(((await screen.findByRole("checkbox", { name: "PDF 파일만" })) as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("textbox", { name: "새 이름" }) as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("combobox", { name: "날짜 형식" }) as HTMLSelectElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "날짜" }) as HTMLButtonElement).disabled).toBe(true);
});

it("pick_button_calls_on_pick_slot_and_is_off_while_busy", async () => {
  const onPickSlot = vi.fn();
  const b: Blueprint = {
    ...receipt,
    slots: { watch: slot("local.folder_new_file", "folder", null), dest: slot("local.file_move", "baseFolder", "~/Documents/영수증") },
  };
  const { rerender } = render(surface(b, { busy: true, onPickSlot }));
  expect(((await screen.findByRole("button", { name: "폴더 고르기" })) as HTMLButtonElement).disabled).toBe(true);
  rerender(surface(b, { onPickSlot }));
  fireEvent.click(screen.getByRole("button", { name: "폴더 고르기" }));
  expect(onPickSlot).toHaveBeenCalledWith("watch");
});

it("new_version_rebuilds_surface_without_error", async () => {
  const err = vi.spyOn(console, "error");
  const { rerender } = render(surface(receipt));
  await screen.findByRole("group", { name: "영수증 정리" });
  const v2: Blueprint = { ...receipt, version: 2, components: [{ ...receipt.components[0], title: "영수증 정리 2" }, ...receipt.components.slice(1)] };
  rerender(surface(v2));
  expect(await screen.findByRole("group", { name: "영수증 정리 2" })).toBeTruthy();
  expect(screen.queryByRole("group", { name: "영수증 정리" })).toBeNull();
  expect(err).not.toHaveBeenCalled();
  err.mockRestore();
});

it("changed_highlights_for_two_seconds", () => {
  vi.useFakeTimers();
  render(<Changed on>칸</Changed>);
  const box = screen.getByText("칸");
  expect(box.className).toContain("bg-amber-50");
  act(() => vi.advanceTimersByTime(2000));
  expect(box.className).not.toContain("bg-amber-50");
  vi.useRealTimers();
});
