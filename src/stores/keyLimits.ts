import { create } from "zustand";
import { api, type KeyLimitsView } from "../api";

/** API 키별 월 한도와 이번 달 지출. 실패하면 Rust 오류를 던지고 값은 그대로 — 부르는 쪽이 받는다. */
export const useKeyLimits = create<{ view: KeyLimitsView | null; load: () => Promise<void> }>((set) => ({
  view: null,
  load: async () => set({ view: await api.keyLimits() }),
}));
