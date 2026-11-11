import { LogIn, LogOut, ShieldAlert, ShieldCheck, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from "react";
import { api, type ApiProvider, type KeychainStatus, type KeyState, type Providers, type Settings as SettingsT } from "../api";
import { INPUT } from "../editor/ConfigPanel";
import { t } from "../i18n";
import { useSettings } from "../stores/settings";
import { useWeb, webErrorText } from "../stores/web";
import { BUTTON, PRIMARY } from "./Editor";

const PROVIDERS: { id: ApiProvider; name: string }[] = [
  { id: "anthropic", name: "Anthropic" },
  { id: "openai", name: "OpenAI" },
  { id: "gemini", name: "Gemini" },
];
const EMPTY_KEYS: Record<ApiProvider, string> = { anthropic: "", openai: "", gemini: "" };

/** 키체인 저장 여부만 — 값은 Rust도 돌려주지 않는다. */
function KeyBadge({ state }: { state?: KeyState }) {
  if (state === "stored")
    return (
      <span className="inline-flex items-center gap-1 typo-caption1_semibold text-main-deep-blue">
        <ShieldCheck size={14} aria-hidden="true" />
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
  if (state === "missing") return <span className="typo-caption1_regular text-neutral-500">{t("settings.keyMissing")}</span>;
  return null;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 rounded-brand-md border border-neutral-200 bg-white p-5">
      <h2 className="typo-body1_bold text-neutral-800">{title}</h2>
      {children}
    </section>
  );
}

const Err = ({ msg }: { msg: string | null }) => (msg ? <p className="typo-caption1_semibold text-danger-700">{msg}</p> : null);

export function Settings() {
  const settings = useSettings((s) => s.settings);
  const save = useSettings((s) => s.save);
  const { account, expired, login, logout, load: loadAccount } = useWeb();
  const [providers, setProviders] = useState<Providers | null>(null);
  const [keychain, setKeychain] = useState<KeychainStatus | null>(null);
  const [keys, setKeys] = useState(EMPTY_KEYS);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [generalError, setGeneralError] = useState<string | null>(null);
  const [serverUrl, setServerUrl] = useState(settings?.serverUrl ?? "");
  const [serverError, setServerError] = useState<string | null>(null);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [loginError, setLoginError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    api.detectProviders().then(setProviders, (e) => setKeyError(String(e)));
    api.keychainStatus().then(setKeychain, (e) => setKeyError(String(e)));
  }, []);
  useEffect(refresh, [refresh]);

  const update = async (patch: Partial<SettingsT>) => {
    if (!settings) return;
    setGeneralError(null);
    try {
      await save({ ...settings, ...patch });
    } catch (e) {
      setGeneralError(String(e));
    }
  };

  // 입력란은 보내기 전에 비운다 — 키를 상태에 남기지 않는다.
  const saveKey = async (p: ApiProvider) => {
    const key = keys[p].trim();
    setKeys((k) => ({ ...k, [p]: "" }));
    setKeyError(null);
    try {
      await api.setApiKey(p, key);
    } catch (e) {
      setKeyError(String(e));
    }
    refresh();
  };
  const deleteKey = async (p: ApiProvider) => {
    setKeyError(null);
    try {
      await api.deleteApiKey(p);
    } catch (e) {
      setKeyError(String(e));
    }
    refresh();
  };

  // 서버 주소 허용 목록 밖이면 Rust 오류 문구 그대로.
  const saveServer = async (e: FormEvent) => {
    e.preventDefault();
    if (!settings) return;
    setServerError(null);
    try {
      await save({ ...settings, serverUrl: serverUrl.trim() });
      await loadAccount();
    } catch (err) {
      setServerError(String(err));
    }
    refresh();
  };

  const doLogin = async (e: FormEvent) => {
    e.preventDefault();
    const pw = password;
    setPassword("");
    setLoginError(null);
    setBusy(true);
    try {
      await login(email.trim(), pw);
      setEmail("");
    } catch (err) {
      setLoginError(webErrorText(err));
    } finally {
      setBusy(false);
      refresh();
    }
  };

  const doLogout = async () => {
    setBusy(true);
    try {
      await logout();
    } catch (err) {
      setLoginError(webErrorText(err));
    } finally {
      setBusy(false);
      refresh();
    }
  };

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
      <h1 className="typo-title2_bold text-neutral-800">{t("settings.title")}</h1>

      <Section title={t("settings.ai")}>
        <div className="flex flex-col gap-1">
          <h3 className="typo-body3_semibold text-neutral-700">{t("settings.cli")}</h3>
          {providers && providers.clis.length === 0 && <p className="typo-caption1_regular text-neutral-500">{t("settings.noCli")}</p>}
          <ul className="flex flex-col gap-1">
            {providers?.clis.map((c) => (
              <li key={c.id} className="typo-caption1_regular text-neutral-700">
                <span className="typo-caption1_semibold">{c.id}</span> {c.version} · <span className="break-all text-neutral-500">{c.path}</span>
              </li>
            ))}
          </ul>
        </div>
        <ul className="flex flex-col gap-3">
          {PROVIDERS.map(({ id, name }) => {
            const label = `${name} ${t("settings.apiKey")}`;
            return (
              <li key={id} className="flex flex-col gap-1.5">
                <div className="flex items-center gap-2">
                  <span className="typo-body3_semibold text-neutral-800">{label}</span>
                  <KeyBadge state={keychain?.[id]} />
                </div>
                <form
                  className="flex items-center gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void saveKey(id);
                  }}
                >
                  <input
                    type="password"
                    autoComplete="off"
                    aria-label={label}
                    placeholder={t("settings.keyPlaceholder")}
                    className={INPUT}
                    value={keys[id]}
                    onChange={(e) => setKeys((k) => ({ ...k, [id]: e.target.value }))}
                  />
                  <button type="submit" className={PRIMARY} disabled={!keys[id].trim()}>
                    {t("common.save")}
                  </button>
                  {keychain?.[id] === "stored" && (
                    <button type="button" className={BUTTON} onClick={() => void deleteKey(id)} aria-label={`${label} ${t("common.delete")}`}>
                      <Trash2 size={16} aria-hidden="true" />
                    </button>
                  )}
                </form>
              </li>
            );
          })}
        </ul>
        <Err msg={keyError} />
      </Section>

      <Section title={t("settings.general")}>
        <label className="flex items-center gap-3 typo-body3_semibold text-neutral-800">
          <span className="w-40">{t("settings.language")}</span>
          <select
            className={`${INPUT} w-auto`}
            value={settings?.language ?? "system"}
            disabled={!settings}
            onChange={(e) => void update({ language: e.target.value as SettingsT["language"] })}
          >
            <option value="system">{t("settings.lang.system")}</option>
            <option value="ko">{t("settings.lang.ko")}</option>
            <option value="en">{t("settings.lang.en")}</option>
          </select>
        </label>
        <label className="flex items-center gap-3 typo-body3_semibold text-neutral-800">
          <input
            type="checkbox"
            className="size-4 accent-main-blue"
            checked={settings?.autostart ?? false}
            disabled={!settings}
            onChange={(e) => void update({ autostart: e.target.checked })}
          />
          {t("settings.autostart")}
        </label>
        <Err msg={generalError} />
      </Section>

      <Section title={t("settings.account")}>
        <form className="flex items-end gap-2" onSubmit={saveServer}>
          <label className="flex min-w-0 flex-1 flex-col gap-1 typo-caption1_semibold text-neutral-700">
            {t("settings.server")}
            <input className={INPUT} value={serverUrl} onChange={(e) => setServerUrl(e.target.value)} />
          </label>
          <button type="submit" className={BUTTON} disabled={!settings || serverUrl.trim() === settings.serverUrl}>
            {t("common.save")}
          </button>
        </form>
        <Err msg={serverError} />

        {account ? (
          <div className="flex items-center gap-3">
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="typo-caption1_regular text-neutral-500">{t("settings.loggedInAs")}</span>
              <span className="truncate typo-body3_semibold text-neutral-800">{account.email}</span>
              <KeyBadge state={keychain?.["ieum-session"]} />
            </div>
            <button type="button" className={BUTTON} disabled={busy} onClick={doLogout}>
              <LogOut size={16} aria-hidden="true" />
              {t("settings.logout")}
            </button>
          </div>
        ) : (
          <form className="flex flex-col gap-2" onSubmit={doLogin}>
            {expired && <p className="typo-caption1_semibold text-danger-700">{t("web.err.AUTH_EXPIRED")}</p>}
            <label className="flex flex-col gap-1 typo-caption1_semibold text-neutral-700">
              {t("settings.email")}
              <input type="email" autoComplete="username" className={INPUT} value={email} onChange={(e) => setEmail(e.target.value)} />
            </label>
            <label className="flex flex-col gap-1 typo-caption1_semibold text-neutral-700">
              {t("settings.password")}
              <input
                type="password"
                autoComplete="current-password"
                className={INPUT}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </label>
            <button type="submit" className={`${PRIMARY} self-start`} disabled={busy || !email.trim() || !password}>
              <LogIn size={16} aria-hidden="true" />
              {t("settings.login")}
            </button>
          </form>
        )}
        <Err msg={loginError} />
      </Section>
    </div>
  );
}
