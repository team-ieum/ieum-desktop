import { getVersion } from "@tauri-apps/api/app";
import { ArrowRightLeft, ChevronDown, ChevronUp, Globe, Laptop, Server } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import symbol from "../assets/symbol.png";
import { t } from "../i18n";
import { useSettings } from "../stores/settings";
import { ErrorText, LoginForm } from "./LoginForm";
import { SignupForm } from "./SignupForm";

const PILL = "inline-flex items-center gap-1.5 rounded-full bg-white/10 px-3 py-1.5 typo-caption1_semibold text-white ring-1 ring-white/20";

const host = (url: string) => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

/** 서버 주소 — 펼치면 입력. 허용 목록 밖이면 Rust 오류 문구 그대로. */
function ServerLink() {
  const settings = useSettings((s) => s.settings);
  const save = useSettings((s) => s.save);
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!settings) return;
    setError(null);
    try {
      await save({ ...settings, serverUrl: url.trim() });
      setOpen(false);
    } catch (err) {
      setError(String(err));
    }
  };

  const Chevron = open ? ChevronUp : ChevronDown;
  return (
    <div className="mt-10">
      <button
        type="button"
        className="mx-auto flex items-center gap-1.5 typo-caption1_medium text-neutral-400 hover:text-main-deep-blue"
        aria-expanded={open}
        disabled={!settings}
        onClick={() => {
          setUrl(settings?.serverUrl ?? "");
          setError(null);
          setOpen((o) => !o);
        }}
      >
        <Server className="size-3.5" aria-hidden="true" />
        {t("auth.server")}
        {settings && `: ${host(settings.serverUrl)}`}
        <Chevron className="size-3.5" aria-hidden="true" />
      </button>
      {open && (
        <form className="mt-3 flex items-center gap-2" onSubmit={submit}>
          <input
            aria-label={t("settings.server")}
            className="min-w-0 flex-1 rounded-brand-md bg-main-light-blue px-3 py-2 typo-body2_medium text-main-deep-blue outline-none ring-1 ring-transparent focus:ring-sub-blue"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
          <button
            type="submit"
            className="rounded-brand-md bg-main-deep-blue px-3 py-2 typo-caption1_semibold text-white disabled:opacity-60"
            disabled={!url.trim() || url.trim() === settings?.serverUrl}
          >
            {t("common.save")}
          </button>
        </form>
      )}
      {error && (
        <div className="mt-2 text-center">
          <ErrorText msg={error} />
        </div>
      )}
    </div>
  );
}

/** 로그인 관문(시안 B) — 왼쪽 남색 브랜드 판, 오른쪽 로그인 ↔ 가입. */
export function AuthScreen({ initialEmail }: { initialEmail?: string }) {
  const [mode, setMode] = useState<"login" | "signup">("login");
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion, () => {});
  }, []);

  return (
    <div className="flex h-full overflow-hidden bg-neutral-white">
      <aside className="relative flex basis-1/2 flex-col justify-between overflow-hidden bg-main-deep-blue px-16 py-14 text-white">
        <div className="absolute -top-24 -right-24 h-[360px] w-[360px] rounded-full bg-sub-blue/35" />
        <div className="absolute top-[230px] right-[120px] h-[90px] w-[90px] rounded-full bg-node-yellow/50" />
        <div className="absolute -bottom-32 -left-20 h-[300px] w-[300px] rounded-full bg-sub-blue/20" />
        <div className="relative flex items-center gap-3">
          <img src={symbol} className="h-11 w-11 brightness-0 invert" alt="" />
          <span className="text-3xl font-black tracking-wide">{t("app.name")}</span>
        </div>
        <div className="relative">
          <p className="text-[40px] leading-tight font-black whitespace-pre-line">{t("banner.tagline")}</p>
          <p className="mt-4 typo-body1_medium text-white/75">{t("auth.intro")}</p>
          <div className="mt-8 flex items-center gap-2">
            <span className={PILL}>
              <Laptop className="size-3.5" aria-hidden="true" />
              {t("nav.local")}
            </span>
            <ArrowRightLeft className="size-4 text-white/60" aria-hidden="true" />
            <span className={PILL}>
              <Globe className="size-3.5" aria-hidden="true" />
              {t("nav.web")}
            </span>
          </div>
        </div>
        <p className="relative typo-caption1_medium text-white/55">IEUM desktop{version && ` · v${version}`}</p>
      </aside>
      <section className="flex basis-1/2 items-center justify-center overflow-y-auto bg-neutral-white px-12">
        <div className="w-[420px]">
          {mode === "login" ? (
            <LoginForm initialEmail={initialEmail} onSignup={() => setMode("signup")} />
          ) : (
            <SignupForm onBack={() => setMode("login")} />
          )}
          <ServerLink />
        </div>
      </section>
    </div>
  );
}
