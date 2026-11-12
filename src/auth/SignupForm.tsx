import { AtSign } from "lucide-react";
import { useState, type FormEvent } from "react";
import { api } from "../api";
import { t } from "../i18n";
import { webErrorText } from "../stores/web";
import { ErrorText, Field, SUBMIT } from "./LoginForm";

/** 가입 ① 이메일 → 인증번호 받기. ②·③(인증번호·비밀번호·가입)은 Task 8. */
export function SignupForm({ onBack }: { onBack(): void }) {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await api.sendCode(email.trim());
      setSent(true);
    } catch (err) {
      setError(webErrorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <h1 className="typo-title2_bold text-main-deep-blue">{t("auth.signup")}</h1>
      <p className="mt-2 typo-body2_regular text-neutral-500">{t("auth.bind")}</p>
      <form className="mt-10 space-y-6" onSubmit={submit}>
        <Field
          label={t("settings.email")}
          icon={AtSign}
          type="email"
          autoComplete="email"
          placeholder={t("auth.emailPlaceholder")}
          value={email}
          onChange={(e) => {
            setEmail(e.target.value);
            setSent(false);
          }}
        />
        {sent && <p className="typo-body2_medium text-main-blue">{t("auth.codeSent")}</p>}
        <ErrorText msg={error} />
        <button type="submit" className={SUBMIT} disabled={busy || !email.trim()}>
          {t("auth.sendCode")}
        </button>
      </form>
      <p className="mt-6 text-center typo-body2_regular text-neutral-500">
        {t("auth.haveAccount")}{" "}
        <button type="button" className="typo-body2_bold text-main-blue hover:underline" onClick={onBack}>
          {t("auth.login")}
        </button>
      </p>
    </>
  );
}
