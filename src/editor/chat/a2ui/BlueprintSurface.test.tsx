// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
