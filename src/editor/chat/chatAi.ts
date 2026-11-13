import type { ApiProvider, CredentialsView } from "../../api";
import { t } from "../../i18n";

/** 채팅이 쓸 키·계정 선택지. `value` = `${provider}|${credential}`. */
export interface AiChoice {
  value: string;
  provider: string;
  credential: string;
  label: string;
}

/** localStorage — 마지막으로 고른 `{value, model}`. */
export const AI_KEY = "ieum.chat.ai";

const API_NAMES: [ApiProvider, string][] = [
  ["anthropic", "Anthropic"],
  ["openai", "OpenAI"],
  ["gemini", "Gemini"],
];

/** Claude 구독 계정 → Anthropic → OpenAI → Gemini 키. 묶음 안에서는 기본이 먼저. */
export function aiChoices(view: CredentialsView | null): AiChoice[] {
  if (!view) return [];
  const group = (provider: string, name: string, items: { id: string; label: string; isDefault: boolean }[]) =>
    [...items]
      .sort((a, b) => Number(b.isDefault) - Number(a.isDefault))
      .map((c) => ({ value: `${provider}|${c.id}`, provider, credential: c.id, label: `${name} · ${c.label}` }));
  return [
    ...group("cli:claude", t("choice.provider.cli:claude"), view.claudeAccounts),
    ...API_NAMES.flatMap(([p, name]) => group(`api:${p}`, name, view.apiKeys[p] ?? [])),
  ];
}

export const chatAvailable = (view: CredentialsView | null) => aiChoices(view).length > 0;
