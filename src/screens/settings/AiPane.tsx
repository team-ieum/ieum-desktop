import { Copy, RefreshCw, ShieldAlert } from "lucide-react";
import { useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, type ApiProvider, type KeychainStatus, type KeyState, type PlanUsage, type UsageSummary } from "../../api";
import { t, type Key } from "../../i18n";
import { formatReset, formatTokens, formatUsd } from "../../usage/format";
import { GHOST, matches, QueryContext } from "./Row";

// 회사 로고 대신 글자·색 표시(상표 그림 파일 안 넣음).
export const PROVIDERS: { id: ApiProvider; name: string; url: string; mark: string; color: string }[] = [
  { id: "anthropic", name: "Anthropic", url: "https://console.anthropic.com/settings/keys", mark: "A", color: "#C96442" },
  { id: "openai", name: "OpenAI", url: "https://platform.openai.com/api-keys", mark: "O", color: "#10A37F" },
  { id: "gemini", name: "Gemini", url: "https://aistudio.google.com/apikey", mark: "G", color: "#4285F4" },
];

export type SubId = "claude" | "codex" | "antigravity";
export const SUBS: { id: SubId; name: string; cmd: string; mark: string; color: string; load: () => Promise<PlanUsage> }[] = [
  { id: "claude", name: "Claude", cmd: "claude", mark: "C", color: "#D97757", load: () => api.subscriptionUsage("claude", "system") },
  { id: "codex", name: "Codex", cmd: "codex", mark: "C", color: "#111827", load: () => api.subscriptionUsage("codex", "system") },
  { id: "antigravity", name: "Antigravity", cmd: "agy", mark: "A", color: "#34A853", load: () => api.antigravityPlanUsage() },
];

export const cliName = (id: string) => (id === "claude" ? t("choice.provider.cli:claude") : id);
export const subName = (name: string) => t("settings.subName").replace("{name}", name);
export const apiName = (name: string) => `${name} API`;

/** 구독 3개 연결 상태. 키 없음 = 읽는 중, null = 못 읽음(명령 실패). */
export type Plans = Partial<Record<SubId, PlanUsage | null>>;
export function usePlans() {
  const [plans, setPlans] = useState<Plans>({});
  const refresh = useCallback((only?: SubId) => {
    for (const s of SUBS)
      if (!only || s.id === only)
        s.load().then(
          (u) => setPlans((p) => ({ ...p, [s.id]: u })),
          () => setPlans((p) => ({ ...p, [s.id]: null })),
        );
  }, []);
  useEffect(() => refresh(), [refresh]);
  return { plans, refresh };
}

type SubState = "loading" | "ok" | "login" | "unknown";
const subState = (p: PlanUsage | null | undefined): SubState =>
  p === undefined ? "loading" : p?.state === "ok" ? "ok" : p?.state === "not_logged_in" ? "login" : "unknown";
const STATE_TEXT: Record<SubState, Key> = { loading: "common.loading", ok: "settings.connected", login: "settings.loginNeeded", unknown: "settings.unknown" };
const STATE_CLASS: Record<SubState, string> = {
  loading: "bg-neutral-100 text-neutral-500",
  ok: "bg-node-green/10 text-node-green",
  login: "bg-amber-100 text-amber-800",
  unknown: "bg-neutral-100 text-neutral-600",
};
export const subStateText = (p: PlanUsage | null | undefined) => t(STATE_TEXT[subState(p)]);

/** "66% 남음 · 2일 10시간 뒤 초기화" */
export const leftText = (w: PlanUsage["windows"][number], now: Date) =>
  `${Math.max(0, Math.min(100, Math.round(100 - w.usedPct)))}% ${t("usage.left")} · ${t("usage.resetsIn").replace("{time}", formatReset(w.resetsAt, now))}`;

/** 구독 창별 남은 비율 막대·초기화 시각. */
function PlanBars({ plan }: { plan: PlanUsage }) {
  const now = new Date();
  return (
    <ul className="flex flex-col gap-2.5">
      {plan.windows.map((w) => {
        const left = Math.max(0, Math.min(100, Math.round(100 - w.usedPct)));
        return (
          <li key={w.kind}>
            <div className="flex justify-between gap-2 typo-caption1_medium text-neutral-700">
              <span>{t(`usage.window.${w.kind}`)}</span>
              <span className="tabular-nums text-neutral-500">{leftText(w, now)}</span>
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

/** 공급자 카드 틀 — 글자 아이콘·이름·한 줄 설명, 오른쪽 위 `aside`. 설정 검색에 안 맞으면 숨는다. */
function Card({ name, desc, mark, color, aside, texts, children }: { name: string; desc: string; mark: string; color: string; aside?: ReactNode; texts: string[]; children: ReactNode }) {
  const q = useContext(QueryContext);
  if (!matches(q, name, desc, ...texts)) return null;
  return (
    <article aria-label={name} className="rounded-brand-md border border-[#cde9f4] bg-white p-4">
      <div className="flex items-start gap-3">
        <span aria-hidden="true" className="grid size-9 shrink-0 place-items-center rounded-[10px] text-sm font-bold text-white" style={{ background: color }}>
          {mark}
        </span>
        <div className="min-w-0 flex-1">
          <div className="typo-body2_semibold text-neutral-800">{name}</div>
          <div className="mt-0.5 typo-caption1_regular text-neutral-500">{desc}</div>
        </div>
        {aside}
      </div>
      <div className="mt-3 flex flex-col gap-3 pl-12">{children}</div>
    </article>
  );
}

/** 클립보드 복사 — 누르면 잠깐 "복사됨". */
function CopyButton({ text, label }: { text: string; label: string }) {
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const copy = () => {
    setError(null);
    navigator.clipboard.writeText(text).then(
      () => {
        setDone(true);
        setTimeout(() => setDone(false), 1500);
      },
      (e) => setError(String(e)),
    );
  };
  return (
    <>
      <button type="button" className={GHOST} onClick={copy}>
        <Copy className="size-4" aria-hidden="true" />
        {done ? t("settings.copied") : label}
      </button>
      {error && <span className="typo-caption1_semibold text-danger-700">{error}</span>}
    </>
  );
}

const loginHint = (cmd: string) => t("settings.loginHint").replace("{cmd}", cmd);

const subTexts = (s: (typeof SUBS)[number], plan: PlanUsage | null | undefined) => [
  subStateText(plan),
  loginHint(s.cmd),
  t("settings.copyCmd"),
  t("settings.refreshUsage"),
  ...(plan?.accountEmail ? [plan.accountEmail] : []),
];

/** Claude·Codex·Antigravity 구독 — 연결 상태, 계정, 창별 남은 비율. 로그인 전이면 터미널 명령 안내. */
export function SubscriptionCard({ sub, plan, onRefresh }: { sub: (typeof SUBS)[number]; plan: PlanUsage | null | undefined; onRefresh: () => void }) {
  const state = subState(plan);
  const [before, after] = t("settings.loginHint").split("{cmd}");
  const checked = plan?.checkedAt ? Date.parse(plan.checkedAt) : NaN;
  return (
    <Card
      name={subName(sub.name)}
      desc={t(`settings.subDesc.${sub.id}`)}
      mark={sub.mark}
      color={sub.color}
      texts={subTexts(sub, plan)}
      aside={<span className={`shrink-0 rounded-full px-2.5 py-0.5 typo-caption1_semibold ${STATE_CLASS[state]}`}>{t(STATE_TEXT[state])}</span>}
    >
      {state === "ok" && plan && (
        <>
          {plan.accountEmail && <p className="typo-caption1_medium text-neutral-700">{plan.accountEmail}</p>}
          {plan.windows.length > 0 && <PlanBars plan={plan} />}
        </>
      )}
      {state === "login" && (
        <p className="typo-caption1_regular text-neutral-600">
          {before}
          <code className="rounded bg-neutral-100 px-1 font-mono text-neutral-800">{sub.cmd}</code>
          {after}
        </p>
      )}
      {state !== "loading" && (
        <div className="flex flex-wrap items-center gap-2">
          {state === "login" && <CopyButton text={sub.cmd} label={t("settings.copyCmd")} />}
          <button type="button" className={GHOST} onClick={onRefresh}>
            <RefreshCw className="size-4" aria-hidden="true" />
            {t("settings.refreshUsage")}
          </button>
          {state === "ok" && !Number.isNaN(checked) && (
            <span className="typo-caption2_regular text-neutral-400">
              {t("usage.checkedAgo").replace("{time}", formatReset(new Date().toISOString(), new Date(checked)))}
            </span>
          )}
        </div>
      )}
    </Card>
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
  if (state === "missing") return <span className="typo-caption1_semibold text-neutral-500">{t("settings.keyMissing")}</span>;
  return null;
}

const BADGE_TEXT: Record<KeyState, Key> = { stored: "settings.keyStored", missing: "settings.keyMissing", error: "settings.keyError" };

/** "이번 달 12.3만 토큰 · 예상 $1.85" — 못 읽으면 빈 상태 문구. */
const monthText = (usage: UsageSummary | null, id: ApiProvider) => {
  if (!usage) return t("usage.empty");
  const s = usage.bySource.find((x) => x.source === id);
  return t("usage.month")
    .replace("{tokens}", formatTokens(s ? s.input + s.output : 0))
    .replace("{usd}", formatUsd(s ? s.usd : 0));
};

const INPUT =
  "h-9 min-w-0 flex-1 rounded-[10px] border border-neutral-200 bg-white px-3 text-sm text-neutral-800 outline-none focus:border-main-blue focus:ring-2 focus:ring-main-blue/15 placeholder:text-neutral-400";
const SAVE = "inline-flex h-9 shrink-0 items-center rounded-[10px] bg-main-deep-blue px-4 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-40";

/** Anthropic·OpenAI·Gemini 키 — 입력란은 보내기 전에 비운다(키를 상태에 남기지 않는다). */
export function ApiKeyCard({ p, state, usage, refresh }: { p: (typeof PROVIDERS)[number]; state?: KeyState; usage?: UsageSummary | null; refresh: () => void }) {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const label = `${p.name} ${t("settings.apiKey")}`;
  const host = p.url.replace("https://", "");
  const run = async (call: Promise<void>) => {
    setError(null);
    try {
      await call;
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };
  const save = () => {
    const key = draft.trim();
    setDraft("");
    void run(api.addApiKey(p.id, t("settings.keyDefaultLabel"), key).then(() => {}));
  };
  // ponytail: 키 목록 화면은 Task 3 — 그 전까지 [삭제]는 기본 키를 지운다.
  const removeDefault = async () => {
    const d = (await api.listCredentials()).apiKeys[p.id]?.find((k) => k.isDefault);
    if (d) await api.deleteCredential(p.id, d.id);
  };
  return (
    <Card
      name={apiName(p.name)}
      desc={t("settings.apiDesc")}
      mark={p.mark}
      color={p.color}
      texts={[...(state ? [t(BADGE_TEXT[state])] : []), t("settings.getKey"), host]}
      aside={
        // 외부 주소를 여는 명령이 없다(임의 URL 명령 금지) — 주소를 보이고 복사만.
        <span className="shrink-0 text-right typo-caption2_regular text-neutral-500">
          {t("settings.getKey")} ↗
          <span className="block select-all font-mono text-neutral-700" title={p.url}>
            {host}
          </span>
        </span>
      }
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 typo-caption1_regular text-neutral-500">
        <KeyBadge state={state} />
        {usage !== undefined && <span>{monthText(usage, p.id)}</span>}
      </div>
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <input
          type="password"
          autoComplete="off"
          aria-label={label}
          placeholder={t("settings.keyPlaceholder")}
          className={INPUT}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
        <button type="submit" className={SAVE} disabled={!draft.trim()}>
          {t("common.save")}
        </button>
        {state === "stored" && (
          <button type="button" className={GHOST} onClick={() => void run(removeDefault())}>
            {t("common.delete")}
          </button>
        )}
        <CopyButton text={p.url} label={t("settings.copyUrl")} />
      </form>
      {error && <p className="typo-caption1_semibold text-danger-700">{error}</p>}
    </Card>
  );
}

/** 설정 검색에 걸릴 카드 문구 전부. */
export const aiTexts = (plans: Plans, keychain: KeychainStatus | null) => [
  ...SUBS.flatMap((s) => [subName(s.name), t(`settings.subDesc.${s.id}`), ...subTexts(s, plans[s.id])]),
  ...PROVIDERS.flatMap((p) => [apiName(p.name), t("settings.apiDesc"), t("settings.getKey"), p.url, ...(keychain?.[p.id] ? [t(BADGE_TEXT[keychain[p.id]])] : [])]),
];

/** AI 제공자 — 구독 3장 + API 키 3장. 설정과 온보딩이 같이 쓴다. */
export function AiProvidersPane({
  plans,
  refreshPlan,
  keychain,
  refreshKeys,
  usage,
}: {
  plans: Plans;
  refreshPlan: (id: SubId) => void;
  keychain: KeychainStatus | null;
  refreshKeys: () => void;
  usage?: UsageSummary | null;
}) {
  return (
    <div className="flex flex-col gap-3">
      {SUBS.map((s) => (
        <SubscriptionCard key={s.id} sub={s} plan={plans[s.id]} onRefresh={() => refreshPlan(s.id)} />
      ))}
      {PROVIDERS.map((p) => (
        <ApiKeyCard key={p.id} p={p} state={keychain?.[p.id]} usage={usage} refresh={refreshKeys} />
      ))}
      {usage && (
        <p className="typo-caption2_regular text-neutral-400">
          {t("usage.asOf").replace("{date}", usage.asOf)} · {t("usage.onlyIeum")}
        </p>
      )}
    </div>
  );
}
