import { Copy, Plus, ShieldAlert } from "lucide-react";
import { useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import {
  api,
  keyStates,
  type ApiKeyMeta,
  type ApiProvider,
  type CliAccountBegin,
  type CliAccountMeta,
  type CredentialKind,
  type CredentialsView,
  type KeyState,
  type PlanUsage,
  type SubKind,
} from "../../api";
import { ProviderLogo, type LogoId } from "../../assets/providers";
import { t, type Key } from "../../i18n";
import { GHOST, matches, QueryContext } from "./Row";

// 키 받기 주소는 Rust 고정값(open_provider_console) — 화면은 주소를 들고 있지 않는다.
export const PROVIDERS: { id: ApiProvider; name: string; logo: LogoId }[] = [
  { id: "anthropic", name: "Anthropic", logo: "anthropic" },
  { id: "openai", name: "OpenAI", logo: "openai" },
  { id: "gemini", name: "Gemini", logo: "gemini" },
];

export type SubId = "claude" | "codex" | "antigravity";
export const SUBS: { id: SubId; name: string; cmd: string; logo: LogoId }[] = [
  { id: "claude", name: "Claude", cmd: "claude", logo: "claude" },
  { id: "codex", name: "Codex", cmd: "codex", logo: "openai" },
  { id: "antigravity", name: "Antigravity", cmd: "agy", logo: "antigravity" },
];

/** 회사별 박스 — 구독 하나 + API 키 하나. */
export type BoxId = "claude" | "openai" | "google";
export const BOXES: { id: BoxId; name: string; logo: LogoId; sub: (typeof SUBS)[number]; api: (typeof PROVIDERS)[number] }[] = [
  { id: "claude", name: "Claude", logo: "claude", sub: SUBS[0], api: PROVIDERS[0] },
  { id: "openai", name: "OpenAI", logo: "openai", sub: SUBS[1], api: PROVIDERS[1] },
  { id: "google", name: "Google", logo: "gemini", sub: SUBS[2], api: PROVIDERS[2] },
];

export const cliName = (id: string) => (id === "claude" ? t("choice.provider.cli:claude") : id);
export const apiName = (name: string) => `${name} API`;

export const SYSTEM = "system";
const SYSTEM_STUB: CliAccountMeta = { id: SYSTEM, label: "이 Mac의 로그인", isDefault: true, createdAt: "", email: null };
/** 구독 계정 목록. 목록을 못 읽었으면 이 Mac의 로그인만. */
export const accountsOf = (view: CredentialsView | null | undefined, kind: SubKind) =>
  view ? (kind === "claude" ? view.claudeAccounts : view.codexAccounts) : [SYSTEM_STUB];
/** Rust가 붙인 한국어 기본 이름표는 화면 언어로. */
export const credLabel = (m: { id: string; label: string }) =>
  m.id === SYSTEM && m.label === "이 Mac의 로그인"
    ? t("settings.systemLabel")
    : m.id === "default" && m.label === "기본"
      ? t("settings.keyDefaultLabel")
      : m.label;

/** `claude:<계정 id>`·`codex:<계정 id>`·`antigravity` */
export const planKey = (kind: SubId, id = SYSTEM) => (kind === "antigravity" ? kind : `${kind}:${id}`);
/** 구독 계정별 연결 상태. 키 없음 = 읽는 중, null = 못 읽음(명령 실패). */
export type Plans = Partial<Record<string, PlanUsage | null>>;
export function usePlans(view: CredentialsView | null | undefined, onlyDefault = false) {
  const [plans, setPlans] = useState<Plans>({});
  const sig = [
    ...(["claude", "codex"] as const).flatMap((k) =>
      accountsOf(view, k)
        .filter((m) => !onlyDefault || m.isDefault)
        .map((m) => planKey(k, m.id)),
    ),
    planKey("antigravity"),
  ].join(",");
  const load = useCallback(
    (only?: SubId, fresh = false) => {
      for (const key of sig.split(",")) {
        const [k, id] = key.split(":") as [SubId, string];
        if (only && k !== only) continue;
        (k === "antigravity" ? api.antigravityPlanUsage(fresh) : api.subscriptionUsage(k, id, fresh)).then(
          (u) => setPlans((p) => ({ ...p, [key]: u })),
          () => setPlans((p) => ({ ...p, [key]: null })),
        );
      }
    },
    [sig],
  );
  useEffect(() => load(), [load]);
  /** [새로 고침] — 앱 캐시도 건너뛴다. */
  const refresh = useCallback((only?: SubId) => load(only, true), [load]);
  return { plans, refresh };
}

type SubState = "loading" | "ok" | "login" | "expired" | "unknown";
const subState = (p: PlanUsage | null | undefined): SubState =>
  p === undefined ? "loading" : p?.state === "ok" ? "ok" : p?.state === "not_logged_in" ? "login" : p?.state === "expired" ? "expired" : "unknown";
const STATE_TEXT: Record<SubState, Key> = { loading: "common.loading", ok: "settings.connected", login: "settings.loginNeeded", expired: "settings.loginExpired", unknown: "settings.unknown" };
const STATE_CLASS: Record<SubState, string> = {
  loading: "bg-neutral-100 text-neutral-500",
  ok: "bg-node-green/10 text-node-green",
  login: "bg-amber-100 text-amber-800",
  expired: "bg-amber-100 text-amber-800",
  unknown: "bg-neutral-100 text-neutral-600",
};
export const subStateText = (p: PlanUsage | null | undefined) => t(STATE_TEXT[subState(p)]);
const StateChip = ({ plan }: { plan: PlanUsage | null | undefined }) => (
  <span className={`shrink-0 rounded-full px-2.5 py-0.5 typo-caption1_semibold ${STATE_CLASS[subState(plan)]}`}>{subStateText(plan)}</span>
);

type Part = { title: string; desc: string; texts: string[]; body: ReactNode };

/** 회사 박스 — 로고·이름·한 줄 설명, 오른쪽 위 [키 받기 ↗], 안에 "구독"·"API 키" 칸.
 *  박스 머리가 검색에 맞으면 칸 전부, 아니면 맞는 칸만. 남는 칸이 없으면 박스째 숨는다. */
function Box({ box, parts }: { box: (typeof BOXES)[number]; parts: Part[] }) {
  const q = useContext(QueryContext);
  const [error, setError] = useState<string | null>(null);
  const desc = t(`settings.boxDesc.${box.id}`);
  const head = matches(q, box.name, desc);
  const shown = parts.filter((p) => head || matches(q, p.title, p.desc, ...p.texts));
  if (!shown.length) return null;
  const openConsole = () => {
    setError(null);
    api.openProviderConsole(box.api.id).catch((e) => setError(String(e)));
  };
  return (
    <article aria-label={box.name} className="rounded-brand-md border border-[#cde9f4] bg-white p-4">
      <div className="flex items-start gap-3">
        <ProviderLogo id={box.logo} />
        <div className="min-w-0 flex-1">
          <div className="typo-body2_semibold text-neutral-800">{box.name}</div>
          <div className="mt-0.5 typo-caption1_regular text-neutral-500">{desc}</div>
        </div>
        <button type="button" className={SMALL} onClick={openConsole}>
          {t("settings.getKey")} ↗
        </button>
      </div>
      {error && <p className="mt-2 pl-12 typo-caption1_semibold text-danger-700">{error}</p>}
      <div className="mt-3 flex flex-col gap-4 pl-12">
        {shown.map((p) => (
          <section key={p.title} aria-label={p.title} className="flex flex-col gap-3 border-t border-neutral-100 pt-3 first:border-0 first:pt-0">
            <div>
              <h4 className="typo-caption1_semibold tracking-[.04em] text-main-gray">{p.title}</h4>
              <p className="mt-0.5 typo-caption1_regular text-neutral-500">{p.desc}</p>
            </div>
            {p.body}
          </section>
        ))}
      </div>
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

const INPUT =
  "h-9 min-w-0 flex-1 rounded-[10px] border border-neutral-200 bg-white px-3 text-sm text-neutral-800 outline-none focus:border-main-blue focus:ring-2 focus:ring-main-blue/15 placeholder:text-neutral-400";
const SAVE = "inline-flex h-9 shrink-0 items-center rounded-[10px] bg-main-deep-blue px-4 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-40";
const DANGER = "inline-flex h-9 shrink-0 items-center rounded-[10px] bg-danger-600 px-4 text-sm font-semibold text-white hover:opacity-90";
const SMALL = "inline-flex h-7 shrink-0 items-center rounded-lg px-2 typo-caption1_semibold text-neutral-600 hover:bg-neutral-100";

/** 명령을 부르고 오류를 보인 뒤 목록을 다시 읽는다. 성공하면 true. */
function useRun(refresh: () => void) {
  const [error, setError] = useState<string | null>(null);
  const run = async (call: Promise<unknown>) => {
    setError(null);
    let ok = true;
    try {
      await call;
    } catch (e) {
      setError(String(e));
      ok = false;
    }
    refresh();
    return ok;
  };
  return { error, run };
}

/** 키·계정 한 줄 — 이름표·기본 배지·[기본으로]·[이름 바꾸기]·[삭제·연결 해제](확인 한 번). */
function CredRow({
  kind,
  m,
  info,
  removeLabel,
  confirmText,
  confirmLabel,
  onRemove,
  refresh,
}: {
  kind: CredentialKind;
  m: { id: string; label: string; isDefault: boolean };
  info: ReactNode;
  removeLabel: string;
  confirmText: string;
  confirmLabel: string;
  onRemove: () => Promise<void>;
  refresh: () => void;
}) {
  const [mode, setMode] = useState<"idle" | "rename" | "remove">("idle");
  const [draft, setDraft] = useState("");
  const { error, run } = useRun(refresh);
  const [busy, setBusy] = useState(false);
  const label = credLabel(m);
  const act = async (call: Promise<unknown>) => {
    setBusy(true);
    if (await run(call)) setMode("idle");
    setBusy(false);
  };
  return (
    <li aria-label={label} className="flex flex-col gap-2 border-b border-neutral-100 pb-3 last:border-0 last:pb-0">
      <div className="flex flex-wrap items-center gap-2">
        {mode === "rename" ? (
          <form
            className="flex min-w-0 flex-1 items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void act(api.renameCredential(kind, m.id, draft.trim()));
            }}
          >
            <input aria-label={t("settings.label")} className={INPUT} value={draft} maxLength={40} autoFocus onChange={(e) => setDraft(e.target.value)} />
            <button type="submit" className={SAVE} disabled={!draft.trim()}>
              {t("common.save")}
            </button>
            <button type="button" className={GHOST} onClick={() => setMode("idle")}>
              {t("common.cancel")}
            </button>
          </form>
        ) : (
          <>
            <span className="min-w-0 truncate typo-body3_semibold text-neutral-800">{label}</span>
            {m.isDefault && (
              <span data-testid="default-badge" className="rounded-full bg-main-light-blue px-2 py-0.5 typo-caption2_semibold text-main-deep-blue">
                {t("settings.defaultBadge")}
              </span>
            )}
            {mode === "idle" && (
              <span className="ml-auto flex flex-wrap gap-0.5">
                {!m.isDefault && (
                  <button type="button" className={SMALL} onClick={() => void run(api.setDefaultCredential(kind, m.id))}>
                    {t("settings.makeDefault")}
                  </button>
                )}
                <button
                  type="button"
                  className={SMALL}
                  onClick={() => {
                    setDraft(label);
                    setMode("rename");
                  }}
                >
                  {t("settings.rename")}
                </button>
                <button type="button" className={SMALL} onClick={() => setMode("remove")}>
                  {removeLabel}
                </button>
              </span>
            )}
          </>
        )}
      </div>
      {info}
      {mode === "remove" && (
        <div className="flex flex-wrap items-center gap-2 rounded-[10px] bg-danger-100 px-3 py-2">
          <span className="min-w-0 flex-1 typo-caption1_semibold text-danger-700">{confirmText}</span>
          <button type="button" className={`${DANGER} disabled:opacity-40`} disabled={busy} onClick={() => void act(onRemove())}>
            {confirmLabel}
          </button>
          <button type="button" className={GHOST} onClick={() => setMode("idle")}>
            {t("common.cancel")}
          </button>
        </div>
      )}
      {error && <p className="typo-caption1_semibold text-danger-700">{error}</p>}
    </li>
  );
}

const loginHint = (cmd: string) => t("settings.loginHint").replace("{cmd}", cmd);

/** 계정 하나의 연결 상태 — 이메일·연결 알약, 로그인 전이면(이 Mac의 로그인만) 터미널 명령 안내. */
function PlanInfo({ plan, email, cmd }: { plan: PlanUsage | null | undefined; email?: string | null; cmd?: string }) {
  const state = subState(plan);
  const [before, after] = t("settings.loginHint").split("{cmd}");
  const shown = email ?? plan?.accountEmail;
  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        {shown && <span className="min-w-0 truncate typo-caption1_medium text-neutral-700">{shown}</span>}
        <StateChip plan={plan} />
      </div>
      {state === "login" && cmd && (
        <div className="flex flex-wrap items-center gap-2">
          <p className="typo-caption1_regular text-neutral-600">
            {before}
            <code className="rounded bg-neutral-100 px-1 font-mono text-neutral-800">{cmd}</code>
            {after}
          </p>
          <CopyButton text={cmd} label={t("settings.copyCmd")} />
        </div>
      )}
      {state === "expired" && <p className="typo-caption1_regular text-neutral-600">{t("settings.expiredHint")}</p>}
    </>
  );
}

/** [계정 연결] → 이름표 → 로그인 명령 복사 → 터미널에서 로그인 → [확인]. 숨긴 이 Mac의 로그인은 여기서 되살린다. */
function ConnectFlow({ kind, systemHidden, refresh }: { kind: SubKind; systemHidden: boolean; refresh: () => void }) {
  const [open, setOpen] = useState(false);
  const [label, setLabel] = useState("");
  const [pending, setPending] = useState<CliAccountBegin | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { error, run } = useRun(refresh);
  const close = () => {
    setOpen(false);
    setPending(null);
    setNotice(null);
    setLabel("");
  };
  // 응답 전 연타는 보이지 않는 대기 폴더를 하나 더 만든다 — 끝날 때까지 막는다.
  const begin = async () => {
    if (busy) return;
    setBusy(true);
    const r = api.beginCliAccount(kind, label.trim());
    if (await run(r)) setPending(await r);
    setBusy(false);
  };
  const confirm = async () => {
    if (!pending) return;
    setBusy(true);
    setNotice(null);
    const r = api.confirmCliAccount(kind, pending.id);
    if (await run(r)) {
      const u = await r;
      if (u.accountEmail) close();
      else setNotice(u.state === "not_logged_in" ? t("settings.notLoggedInYet") : t("settings.unknown"));
    }
    setBusy(false);
  };
  const cancel = async () => {
    if (pending && !(await run(api.cancelCliAccount(kind, pending.id)))) return;
    close();
  };
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <button type="button" className={GHOST} disabled={open} onClick={() => setOpen(true)}>
          <Plus className="size-4" aria-hidden="true" />
          {t("settings.connectAccount")}
        </button>
      </div>
      {open && (
        <div className="flex flex-col gap-2 rounded-[10px] bg-neutral-50 p-3">
          {systemHidden && !pending && (
            <button type="button" className={`${GHOST} self-start`} onClick={() => void run(api.setSystemHidden(kind, false)).then((ok) => ok && close())}>
              {t("settings.reuseSystem")}
            </button>
          )}
          {!pending ? (
            <form
              className="flex items-center gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void begin();
              }}
            >
              <input
                aria-label={t("settings.newAccountLabel")}
                placeholder={t("settings.labelPlaceholder")}
                className={INPUT}
                maxLength={40}
                value={label}
                onChange={(e) => setLabel(e.target.value)}
              />
              <button type="submit" className={SAVE} disabled={!label.trim() || busy}>
                {t("settings.next")}
              </button>
              <button type="button" className={GHOST} onClick={close}>
                {t("common.cancel")}
              </button>
            </form>
          ) : (
            <>
              <code className="block rounded-lg bg-neutral-900 px-3 py-2 font-mono text-[12px] break-all text-neutral-50 select-all">{pending.command}</code>
              <p className="typo-caption1_regular text-neutral-600">{t("settings.accountCmdHint")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <CopyButton text={pending.command} label={t("settings.copyCmd")} />
                <button type="button" className={SAVE} disabled={busy} onClick={() => void confirm()}>
                  {t("settings.confirm")}
                </button>
                <button type="button" className={GHOST} disabled={busy} onClick={() => void cancel()}>
                  {t("common.cancel")}
                </button>
                {notice && <span className="typo-caption1_semibold text-amber-800">{notice}</span>}
              </div>
            </>
          )}
        </div>
      )}
      {error && <p className="typo-caption1_semibold text-danger-700">{error}</p>}
    </div>
  );
}

const subTexts = (s: (typeof SUBS)[number], view: CredentialsView | null | undefined, plans: Plans) => {
  const accounts = s.id === "antigravity" ? [] : accountsOf(view, s.id);
  const keys = s.id === "antigravity" ? [planKey("antigravity")] : accounts.map((m) => planKey(s.id, m.id));
  return [
    ...keys.map((k) => subStateText(plans[k])),
    loginHint(s.cmd),
    t("settings.copyCmd"),
    ...(s.id === "antigravity" ? [] : [t("settings.connectAccount"), t("settings.disconnect")]),
    ...accounts.flatMap((m) => [credLabel(m), m.email ?? ""]),
    ...keys.flatMap((k) => (plans[k]?.accountEmail ? [plans[k]!.accountEmail!] : [])),
  ];
};

/** Claude·Codex 구독 — 계정 목록(이름표·이메일·연결 상태)과 [계정 연결]. Antigravity는 지금 로그인 하나만.
 *  사용량은 설정 › 통계 및 사용량. */
function SubscriptionBody({
  sub,
  view,
  plans,
  onRefresh,
  refreshCreds,
}: {
  sub: (typeof SUBS)[number];
  view: CredentialsView | null | undefined;
  plans: Plans;
  onRefresh: () => void;
  refreshCreds: () => void;
}) {
  if (sub.id === "antigravity") return <PlanInfo plan={plans[planKey("antigravity")]} cmd={sub.cmd} />;
  const kind = sub.id;
  const accounts = accountsOf(view, kind);
  const refreshAll = () => {
    refreshCreds();
    onRefresh();
  };
  return (
    <>
      {accounts.length ? (
        <ul className="flex flex-col gap-3">
          {accounts.map((m) => {
            const system = m.id === SYSTEM;
            return (
              <CredRow
                key={m.id}
                kind={kind}
                m={m}
                info={<PlanInfo plan={plans[planKey(kind, m.id)]} email={m.email} cmd={system ? sub.cmd : undefined} />}
                removeLabel={t("settings.disconnect")}
                confirmLabel={t("settings.disconnect")}
                confirmText={t(system ? "settings.disconnectSystem" : "settings.disconnectAdded")}
                onRemove={() => (system ? api.setSystemHidden(kind, true) : api.deleteCredential(kind, m.id))}
                refresh={refreshAll}
              />
            );
          })}
        </ul>
      ) : (
        <p className="typo-caption1_regular text-neutral-500">{t("settings.noAccounts")}</p>
      )}
      <ConnectFlow kind={kind} systemHidden={!!view && !accounts.some((m) => m.id === SYSTEM)} refresh={refreshAll} />
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
  if (state === "missing") return <span className="typo-caption1_semibold text-neutral-500">{t("settings.keyMissing")}</span>;
  return null;
}

const BADGE_TEXT: Record<KeyState, Key> = { stored: "settings.keyStored", missing: "settings.keyMissing", error: "settings.keyError" };

/** Anthropic·OpenAI·Gemini 키 목록 + [키 추가]. 키 입력란은 보내기 전에 비운다(키를 상태에 남기지 않는다). */
function ApiKeyBody({ p, keys, state, refresh }: { p: (typeof PROVIDERS)[number]; keys: ApiKeyMeta[]; state?: KeyState; refresh: () => void }) {
  const [adding, setAdding] = useState(false);
  const [label, setLabel] = useState("");
  const [draft, setDraft] = useState("");
  const { error, run } = useRun(refresh);
  const first = keys.length === 0;
  const name = label.trim() || (first ? t("settings.keyDefaultLabel") : "");
  const save = async () => {
    const key = draft.trim();
    setDraft("");
    if (await run(api.addApiKey(p.id, name, key))) {
      setAdding(false);
      setLabel("");
    }
  };
  return (
    <>
      <KeyBadge state={state} />
      {!first && (
        <ul className="flex flex-col gap-3">
          {keys.map((k) => (
            <CredRow
              key={k.id}
              kind={p.id}
              m={k}
              info={k.last4 && <span className="typo-caption1_regular font-mono text-neutral-500">…{k.last4}</span>}
              removeLabel={t("common.delete")}
              confirmLabel={t("common.confirmDelete")}
              confirmText={t("settings.deleteKeyConfirm")}
              onRemove={() => api.deleteCredential(p.id, k.id)}
              refresh={refresh}
            />
          ))}
        </ul>
      )}
      {first || adding ? (
        <form
          className="flex flex-wrap items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <input
            aria-label={`${apiName(p.name)} ${t("settings.label")}`}
            placeholder={first ? t("settings.keyDefaultLabel") : t("settings.labelPlaceholder")}
            className={`${INPUT} max-w-36`}
            maxLength={40}
            value={label}
            onChange={(e) => setLabel(e.target.value)}
          />
          <input
            type="password"
            autoComplete="off"
            aria-label={`${p.name} ${t("settings.apiKey")}`}
            placeholder={t("settings.keyPlaceholder")}
            className={INPUT}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          <button type="submit" className={SAVE} disabled={!draft.trim() || !name}>
            {t("common.save")}
          </button>
          {!first && (
            <button type="button" className={GHOST} onClick={() => setAdding(false)}>
              {t("common.cancel")}
            </button>
          )}
        </form>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <button type="button" className={GHOST} onClick={() => setAdding(true)}>
            <Plus className="size-4" aria-hidden="true" />
            {t("settings.addKey")}
          </button>
        </div>
      )}
      {error && <p className="typo-caption1_semibold text-danger-700">{error}</p>}
    </>
  );
}

const apiTexts = (p: (typeof PROVIDERS)[number], view: CredentialsView | null | undefined) => {
  const state = view ? keyStates(view)[p.id] : undefined;
  return [apiName(p.name), t("settings.getKey"), t("settings.addKey"), ...(view?.apiKeys[p.id] ?? []).map(credLabel), ...(state ? [t(BADGE_TEXT[state])] : [])];
};

/** 박스 하나의 칸 제목·설명·검색 문구. 본문은 화면에서 붙인다. */
const partHeads = (b: (typeof BOXES)[number], view: CredentialsView | null | undefined, plans: Plans) => [
  { title: t("settings.subscription"), desc: t(`settings.subDesc.${b.sub.id}`), texts: [b.sub.name, ...subTexts(b.sub, view, plans)] },
  { title: t("settings.apiKey"), desc: t("settings.apiDesc"), texts: apiTexts(b.api, view) },
];

/** 설정 검색에 걸릴 박스 문구 전부. */
export const aiTexts = (plans: Plans, view: CredentialsView | null | undefined) =>
  BOXES.flatMap((b) => [b.name, t(`settings.boxDesc.${b.id}`), ...partHeads(b, view, plans).flatMap((p) => [p.title, p.desc, ...p.texts])]);

/** AI 제공자 — 회사별 박스 3개(구독 + API 키). 설정과 온보딩이 같이 쓴다. */
export function AiProvidersPane({
  view,
  plans,
  refreshPlan,
  refreshCreds,
}: {
  view: CredentialsView | null | undefined;
  plans: Plans;
  refreshPlan: (id: SubId) => void;
  refreshCreds: () => void;
}) {
  const keychain = view ? keyStates(view) : null;
  return (
    <div className="flex flex-col gap-3">
      {BOXES.map((b) => {
        const [subHead, apiHead] = partHeads(b, view, plans);
        return (
          <Box
            key={b.id}
            box={b}
            parts={[
              {
                ...subHead,
                body: <SubscriptionBody sub={b.sub} view={view} plans={plans} onRefresh={() => refreshPlan(b.sub.id)} refreshCreds={refreshCreds} />,
              },
              {
                ...apiHead,
                body: <ApiKeyBody p={b.api} keys={view?.apiKeys[b.api.id] ?? []} state={keychain?.[b.api.id]} refresh={refreshCreds} />,
              },
            ]}
          />
        );
      })}
    </div>
  );
}
