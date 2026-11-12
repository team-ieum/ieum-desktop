import { ChevronDown, ChevronRight, CircleCheck, ShieldAlert, Trash2 } from "lucide-react";
import { useState } from "react";
import { api, type ApiProvider, type KeychainStatus, type KeyState, type PlanUsage, type Providers, type UsageSummary } from "../../api";
import { t, type Key } from "../../i18n";
import { formatReset, formatTokens, formatUsd } from "../../usage/format";
import { GHOST, Row } from "./Row";

export const PROVIDERS: { id: ApiProvider; name: string }[] = [
  { id: "anthropic", name: "Anthropic" },
  { id: "openai", name: "OpenAI" },
  { id: "gemini", name: "Gemini" },
];

export const cliName = (id: string) => (id === "claude" ? t("choice.provider.cli:claude") : id);
export const cliDesc = (version: string) => `${t("settings.cliFound")} (${version})`;

/** Claude 구독 창별 남은 비율 막대·초기화 시각. undefined = 읽는 중, null = 못 읽음. */
function PlanBars({ plan }: { plan: PlanUsage | null | undefined }) {
  if (plan === undefined) return null;
  if (plan?.state === "not_logged_in") return <p className="typo-caption1_regular text-neutral-500">{t("usage.claudeNotLoggedIn")}</p>;
  if (!plan || plan.state !== "ok" || plan.windows.length === 0) return <p className="typo-caption1_regular text-neutral-500">{t("usage.empty")}</p>;
  const now = new Date();
  return (
    <ul className="flex flex-col gap-2.5">
      {plan.windows.map((w) => {
        const left = Math.max(0, Math.min(100, Math.round(100 - w.usedPct)));
        return (
          <li key={w.kind}>
            <div className="flex justify-between gap-2 typo-caption1_medium text-neutral-700">
              <span>{t(`usage.window.${w.kind}`)}</span>
              <span className="tabular-nums text-neutral-500">
                {left}% {t("usage.left")} · {t("usage.resetsIn").replace("{time}", formatReset(w.resetsAt, now))}
              </span>
            </div>
            <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-neutral-100" role="progressbar" aria-label={t(`usage.window.${w.kind}`)} aria-valuenow={left} aria-valuemin={0} aria-valuemax={100}>
              <div className="h-full rounded-full bg-main-blue" style={{ width: `${left}%` }} />
            </div>
          </li>
        );
      })}
    </ul>
  );
}

/** 찾은 구독형 AI(지금은 Claude). */
export function ProvidersPane({ providers, plan }: { providers: Providers | null; plan?: PlanUsage | null }) {
  if (providers && providers.clis.length === 0) return <Row title={t("settings.noCli")} />;
  return (
    <>
      {providers?.clis.map((c) => (
        <Row key={c.id} title={cliName(c.id)} desc={cliDesc(c.version)} also={[t("settings.cliReady")]} below={c.id === "claude" && <PlanBars plan={plan} />}>
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

/** "이번 달 12.3만 토큰 · 예상 $1.85" — 못 읽으면 빈 상태 문구. */
const monthText = (usage: UsageSummary | null, id: ApiProvider) => {
  if (!usage) return t("usage.empty");
  const s = usage.bySource.find((x) => x.source === id);
  return t("usage.month")
    .replace("{tokens}", formatTokens(s ? s.input + s.output : 0))
    .replace("{usd}", formatUsd(s ? s.usd : 0));
};

const INPUT =
  "h-10 min-w-0 flex-1 rounded-[10px] border border-main-blue bg-white px-3 text-sm text-neutral-800 outline-none ring-2 ring-main-blue/15 placeholder:text-neutral-400";
const SAVE = "inline-flex h-10 shrink-0 items-center rounded-[10px] bg-main-deep-blue px-4 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-40";

/** Anthropic·OpenAI·Gemini 키 — "바꾸기/추가"로 줄 안에서 입력을 펼친다. */
export function KeysPane({ keychain, refresh, usage }: { keychain: KeychainStatus | null; refresh: () => void; usage?: UsageSummary | null }) {
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
            desc={
              <>
                <KeyBadge state={keychain?.[id]} />
                {usage !== undefined && <span className="mt-0.5 block">{monthText(usage, id)}</span>}
              </>
            }
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
      {usage && (
        <p className="pb-3 typo-caption2_regular text-neutral-400">
          {t("usage.asOf").replace("{date}", usage.asOf)} · {t("usage.onlyIeum")}
        </p>
      )}
    </>
  );
}
