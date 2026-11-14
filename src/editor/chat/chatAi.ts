import type { ApiProvider, CredentialsView } from "../../api";
import { t } from "../../i18n";

/** 채팅이 쓸 키·계정 선택지. `value` = `${provider}|${credential}`. 계정이 따로 없는 공급원(Antigravity)은 credential이 빈 글자. */
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

/**
 * AI 노드 공급원과 같은 순서: Claude 구독 → Codex 구독 → Antigravity 구독 → Anthropic → OpenAI → Gemini 키. 묶음 안에서는 기본이 먼저.
 * `clis` = 앱이 켜 두었고 이 Mac에 설치된 구독 CLI의 공급원 id(`api.detectProviders`). Codex·Antigravity는 여기 있을 때만 보인다.
 */
export function aiChoices(view: CredentialsView | null, clis: string[] = []): AiChoice[] {
  if (!view) return [];
  const group = (provider: string, name: string, items: { id: string; label: string; isDefault: boolean }[]) =>
    [...items]
      .sort((a, b) => Number(b.isDefault) - Number(a.isDefault))
      .map((c) => ({ value: `${provider}|${c.id}`, provider, credential: c.id, label: `${name} · ${c.label}` }));
  const agy = "cli:agy";
  return [
    ...group("cli:claude", t("choice.provider.cli:claude"), view.claudeAccounts),
    ...(clis.includes("cli:codex") ? group("cli:codex", t("choice.provider.cli:codex"), view.codexAccounts) : []),
    // 계정 목록이 없다 — 이 Mac의 로그인 하나.
    ...(clis.includes(agy) ? [{ value: `${agy}|`, provider: agy, credential: "", label: t("choice.provider.cli:agy") }] : []),
    ...API_NAMES.flatMap(([p, name]) => group(`api:${p}`, name, view.apiKeys[p] ?? [])),
  ];
}

export const chatAvailable = (view: CredentialsView | null, clis: string[] = []) => aiChoices(view, clis).length > 0;
