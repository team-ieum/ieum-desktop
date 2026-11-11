import { create } from "zustand";
import { api, type StoredWorkflow, type WorkflowEntry } from "../api";

interface WorkflowsState {
  list: WorkflowEntry[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
  /** 저장 결과(새 id·updatedAt 포함)를 돌려준다. 실패하면 throw. */
  save: (wf: StoredWorkflow) => Promise<StoredWorkflow>;
  remove: (id: string) => Promise<void>;
}

export const useWorkflows = create<WorkflowsState>((set, get) => ({
  list: [],
  loaded: false,
  error: null,
  load: async () => {
    try {
      set({ list: await api.listWorkflows(), loaded: true, error: null });
    } catch (e) {
      set({ loaded: true, error: String(e) });
    }
  },
  save: async (wf) => {
    const saved = await api.saveWorkflow(wf);
    await get().load();
    return saved;
  },
  remove: async (id) => {
    await api.deleteWorkflow(id);
    await get().load();
  },
}));
