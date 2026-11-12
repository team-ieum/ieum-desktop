import { ChevronDown, ChevronRight, CircleCheck, ShieldAlert, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type ApiProvider, type KeychainStatus, type KeyState, type Providers } from "../../api";
import { t, type Key } from "../../i18n";
import { GHOST, Row } from "./Row";

export const PROVIDERS: { id: ApiProvider; name: string }[] = [
  { id: "anthropic", name: "Anthropic" },
  { id: "openai", name: "OpenAI" },
  { id: "gemini", name: "Gemini" },
];

const CLI_NAMES: Record<string, string> = { claude: "Claude Code" };
export const cliName = (id: string) => CLI_NAMES[id] ?? id;
export const cliDesc = (version: string) => `${t("settings.cliFound")} (${version})`;

/** 찾은 구독형 AI(지금은 Claude). */
export function ProvidersPane({ providers }: { providers: Providers | null }) {
  if (providers && providers.clis.length === 0) return <Row title={t("settings.noCli")} />;
  return (
    <>
      {providers?.clis.map((c) => (
        <Row key={c.id} title={cliName(c.id)} desc={cliDesc(c.version)} also={[t("settings.cliReady")]}>
          <span className="inline-flex shrink-0 items-center gap-1.5 typo-caption1_semibold text-node-green">
            <CircleCheck className="size-[18px]" aria-hidden="true" />
            {t("settings.cliReady")}
          </span>
        </Row>
      ))}
    </>
  );
}

/** 키체인 저장 여부만 — 값은 Rust도 돌려주지 않는다. */
function KeyBadge({ state }: { state?: KeyState }) {
  if (state === "stored")
    return (
      <span className="inline-flex items-center gap-1.5 typo-caption1_semibold text-node-green">
        <span className="size-1.5 rounded-full bg-node-green" />
        {t("settings.keyStored")}
      </span>
    );
  if (state === "error")
    return (
      <span role="alert" className="inline-flex items-center gap-1 typo-caption1_semibold text-danger-700">
        <ShieldAlert size={14} aria-hidden="true" />
        {t("settings.keyError")}
      </span>
    );
  if (state === "missing") return <span>{t("settings.keyMissing")}</span>;
  return null;
}

const BADGE_TEXT: Record<KeyState, Key> = { stored: "settings.keyStored", missing: "settings.keyMissing", error: "settings.keyError" };
/** 키 줄에서 검색에 걸릴 문구 — 상태 배지와 "바꾸기/추가" 버튼. */
export const keyTexts = (state?: KeyState) => [
  ...(state ? [t(BADGE_TEXT[state])] : []),
  state === "stored" ? t("settings.keyChange") : t("settings.keyAdd"),
];

const INPUT =
  "h-10 min-w-0 flex-1 rounded-[10px] border border-main-blue bg-white px-3 text-sm text-neutral-800 outline-none ring-2 ring-main-blue/15 placeholder:text-neutral-400";
const SAVE = "inline-flex h-10 shrink-0 items-center rounded-[10px] bg-main-deep-blue px-4 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-40";

/** Anthropic·OpenAI·Gemini 키 — "바꾸기/추가"로 줄 안에서 입력을 펼친다. */
export function KeysPane({ keychain, refresh }: { keychain: KeychainStatus | null; refresh: () => void }) {
  const [open, setOpen] = useState<ApiProvider | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setOpen(null);
    setDraft("");
  };
  // 입력란은 보내기 전에 비운다 — 키를 상태에 남기지 않는다.
  const run = async (call: Promise<void>) => {
    setError(null);
    try {
      await call;
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };
  const saveKey = (p: ApiProvider) => {
    const key = draft.trim();
    close();
    void run(api.setApiKey(p, key));
  };
  const deleteKey = (p: ApiProvider) => {
    close();
    void run(api.deleteApiKey(p));
  };

  return (
    <>
      {PROVIDERS.map(({ id, name }) => {
        const label = `${name} ${t("settings.apiKey")}`;
        const stored = keychain?.[id] === "stored";
        const isOpen = open === id;
        return (
          <Row
            key={id}
            title={name}
            desc={<KeyBadge state={keychain?.[id]} />}
            also={keyTexts(keychain?.[id])}
            below={
              isOpen && (
                <form
                  className="flex items-center gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    saveKey(id);
                  }}
                >
                  <input
                    type="password"
                    autoComplete="off"
                    autoFocus
                    aria-label={label}
                    placeholder={t("settings.keyPlaceholder")}
                    className={INPUT}
                    value={draft}
                    onChange={(e) => setDraft(e.target.value)}
                  />
                  <button type="submit" className={SAVE} disabled={!draft.trim()}>
                    {t("common.save")}
                  </button>
                  <button type="button" className={`${GHOST} h-10`} onClick={close}>
                    {t("common.cancel")}
                  </button>
                  {stored && (
                    <button type="button" className={`${GHOST} h-10 px-3`} onClick={() => deleteKey(id)} aria-label={`${label} ${t("common.delete")}`}>
                      <Trash2 className="size-4" aria-hidden="true" />
                    </button>
                  )}
                </form>
              )
            }
          >
            <button
              type="button"
              aria-expanded={isOpen}
              onClick={() => (isOpen ? close() : setOpen(id))}
              className={`inline-flex shrink-0 items-center gap-0.5 text-sm font-semibold ${isOpen ? "text-neutral-400" : "text-sub-blue hover:underline"}`}
            >
              {stored ? t("settings.keyChange") : t("settings.keyAdd")}
              {isOpen ? <ChevronDown className="size-4" aria-hidden="true" /> : <ChevronRight className="size-4" aria-hidden="true" />}
            </button>
          </Row>
        );
      })}
      {error && <p className="pb-3 typo-caption1_semibold text-danger-700">{error}</p>}
    </>
  );
}
