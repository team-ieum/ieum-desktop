// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { Blueprint } from "../../../api";
import BlueprintSurface from "./BlueprintSurface";

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
