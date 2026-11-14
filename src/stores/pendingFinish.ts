import { create } from "zustand";
import { api, type PendingFinish } from "../api";

interface PendingFinishState {
  /** 사람의 확인을 기다리는 완료 요청 — 워크플로우 id별. */
  pending: Record<string, PendingFinish>;
  /** 그 워크플로우의 확인 대기를 다시 읽는다. 없거나 못 읽으면 지운다. */
  refresh: (workflowId: string) => Promise<void>;
}

export const usePendingFinish = create<PendingFinishState>((set) => ({
  pending: {},
  refresh: async (workflowId) => {
    if (!workflowId) return;
    const found = await api.pendingFinish(workflowId).catch(() => null);
    set((s) => {
      const { [workflowId]: _old, ...rest } = s.pending;
      return { pending: found ? { ...rest, [workflowId]: found } : rest };
    });
  },
}));
