import { create } from "zustand";
import { api, type CredentialsView } from "../api";

/** 키·계정 목록(값 없음). 설정 카드·노드 칸·캔버스 알약이 같이 본다. 실패하면 Rust 오류를 던지고 목록은 그대로. */
export const useCredentials = create<{ view: CredentialsView | null; load: () => Promise<void> }>((set) => ({
  view: null,
  load: async () => set({ view: await api.listCredentials() }),
}));

/** 노드 공급원(`cli:claude`·`api:openai` …)이 고를 수 있는 키·계정. */
export const choicesFor = (view: CredentialsView, provider: unknown) => {
  if (provider === "cli:claude") return view.claudeAccounts;
  if (provider === "cli:codex") return view.codexAccounts;
  if (typeof provider === "string" && provider.startsWith("api:")) return view.apiKeys[provider.slice(4) as keyof CredentialsView["apiKeys"]] ?? [];
  return [];
};
