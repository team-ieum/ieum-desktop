import { AtSign, Eye, EyeOff, LockKeyhole, type LucideIcon } from "lucide-react";
import { useState, type FormEvent, type InputHTMLAttributes, type ReactNode } from "react";
import { t } from "../i18n";
import { useWeb, webErrorText } from "../stores/web";

/** 웹 `auth/textInput.tsx`의 이름표·입력틀. */
export function Field({ label, icon: Icon, after, ...input }: { label: string; icon: LucideIcon; after?: ReactNode } & InputHTMLAttributes<HTMLInputElement>) {
  return (
    <div>
      <p className="mb-2 pl-3 typo-body1_medium text-main-deep-blue">{label}</p>
      <label className="flex min-h-12.5 items-center gap-2 rounded-brand-md bg-main-light-blue px-3 shadow-[inset_0_0_0_2px_color-mix(in_srgb,var(--color-sub-blue)_20%,transparent)] ring-1 ring-transparent focus-within:ring-sub-blue">
        <Icon className="size-5 text-sub-blue" aria-hidden="true" />
        <input aria-label={label} className="min-w-0 flex-1 bg-transparent typo-body2_medium text-main-deep-blue outline-none placeholder:text-neutral-400" {...input} />
        {after}
      </label>
    </div>
  );
}

/** 웹 `AuthSubmitButton.tsx`, 배경만 남색. */
export const SUBMIT =
  "mt-4 typo-body2_bold w-full rounded-2xl bg-main-deep-blue py-3.5 text-neutral-50 shadow-[0_4px_14px_4px_color-mix(in_srgb,var(--color-main-deep-blue)_25%,transparent)] hover:brightness-105 disabled:opacity-60";

/** 관문의 `web_*` 오류는 문구로. 그 밖(명령 없음·IPC 실패 등) 원문은 화면에 내지 않고 서버 오류로. */
export function authErrorText(e: unknown): string {
  if (typeof e === "object" && e !== null && "code" in e) return webErrorText(e);
  console.error("[ieum] 인증 요청 실패", e);
  return t("web.err.SERVER");
}

export const ErrorText = ({ msg }: { msg: string | null }) =>
  msg ? (
    <p role="alert" className="typo-caption1_semibold text-danger-700">
      {msg}
    </p>
  ) : null;

export function LoginForm({ initialEmail = "", onSignup }: { initialEmail?: string; onSignup(): void }) {
  const login = useWeb((s) => s.login);
  const expired = useWeb((s) => s.expired);
  const [email, setEmail] = useState(initialEmail);
  const [password, setPassword] = useState("");
  const [show, setShow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // 비밀번호는 보내기 전에 칸에서 비운다.
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const pw = password;
    setPassword("");
    setError(null);
    setBusy(true);
    try {
      await login(email.trim(), pw);
    } catch (err) {
      setError(authErrorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <h1 className="typo-title2_bold text-main-deep-blue">{t("auth.login")}</h1>
      <p className="mt-2 typo-body2_regular text-neutral-500">{t("auth.bind")}</p>
      {expired && <p className="mt-4 typo-body2_medium text-danger-700">{t("auth.expired")}</p>}
      <form className="mt-10 space-y-6" onSubmit={submit}>
        <Field
          label={t("settings.email")}
          icon={AtSign}
          type="email"
          autoComplete="username"
          placeholder={t("auth.emailPlaceholder")}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <Field
          label={t("settings.password")}
          icon={LockKeyhole}
          type={show ? "text" : "password"}
          autoComplete="current-password"
          placeholder={t("auth.passwordPlaceholder")}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          after={
            <button
              type="button"
              className="text-neutral-400 hover:text-main-deep-blue"
              aria-label={t(show ? "auth.hidePassword" : "auth.showPassword")}
              onClick={() => setShow((v) => !v)}
            >
              {show ? <Eye className="size-4.5" aria-hidden="true" /> : <EyeOff className="size-4.5" aria-hidden="true" />}
            </button>
          }
        />
        <ErrorText msg={error} />
        <button type="submit" className={SUBMIT} disabled={busy || !email.trim() || !password}>
          {t("auth.login")}
        </button>
      </form>
      <p className="mt-6 text-center typo-body2_regular text-neutral-500">
        {t("auth.noAccount")}{" "}
        <button type="button" className="typo-body2_bold text-main-blue hover:underline" onClick={onSignup}>
          {t("auth.signup")}
        </button>
      </p>
    </>
  );
}
