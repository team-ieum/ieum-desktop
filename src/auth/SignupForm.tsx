import { AtSign, Check, ChevronLeft, Eye, EyeOff, LockKeyhole, UserRound } from "lucide-react";
import { useEffect, useRef, useState, type ClipboardEvent, type FormEvent, type KeyboardEvent } from "react";
import { api } from "../api";
import { t, type Key } from "../i18n";
import { useWeb } from "../stores/web";
import { authErrorText, ErrorText, Field, SUBMIT } from "./LoginForm";
import { checkPassword, PASSWORD_RULES } from "./password";

type Step = 1 | 2 | 3;
const RESEND_SECONDS = 180;
const EMPTY_CODE = ["", "", "", "", "", ""];
const STEPS: Key[] = ["auth.step.email", "auth.step.code", "auth.step.info"];

const errCode = (e: unknown) => (typeof e === "object" && e !== null && "code" in e ? (e as { code: string }).code : null);

/** 시안 signup.html의 3단계 진행 표시. */
function Steps({ step }: { step: Step }) {
  return (
    <ol className="mt-6 flex items-center">
      {STEPS.map((key, i) => {
        const n = i + 1;
        const done = n < step;
        const cur = n === step;
        return (
          <li key={key} className={`flex items-center ${i ? "flex-1" : ""}`}>
            {i > 0 && <span className={`mx-3 h-0.5 flex-1 rounded-full ${n <= step ? "bg-main-blue" : "bg-neutral-200"}`} />}
            <span className="flex items-center gap-2" aria-current={cur ? "step" : undefined}>
              {done ? (
                <span className="flex h-7 w-7 items-center justify-center rounded-full bg-main-blue text-white">
                  <Check className="size-4" aria-hidden="true" />
                </span>
              ) : (
                <span
                  className={`flex h-7 w-7 items-center justify-center rounded-full typo-caption1_bold ${cur ? "bg-main-deep-blue text-white ring-4 ring-main-light-blue" : "bg-neutral-100 text-neutral-400"}`}
                >
                  {n}
                </span>
              )}
              <span className={done ? "typo-body3_semibold text-main-blue" : cur ? "typo-body3_bold text-main-deep-blue" : "typo-body3_medium text-neutral-400"}>
                {t(key)}
              </span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}

/** 가입 ① 이메일 → 인증번호 받기 ② 인증번호 6칸 → 확인 ③ 이름·비밀번호 → 가입하기 → 자동 로그인(관문이 닫힌다). */
export function SignupForm({ onBack }: { onBack(): void }) {
  const login = useWeb((s) => s.login);
  const [step, setStep] = useState<Step>(1);
  const [email, setEmail] = useState("");
  const [code, setCode] = useState(EMPTY_CODE);
  const [left, setLeft] = useState(0);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [show, setShow] = useState(false);
  const [tried, setTried] = useState(false);
  // 가입은 됐는데 자동 로그인이 실패한 경우 — 다시 누르면 로그인만.
  const [registered, setRegistered] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const boxes = useRef<(HTMLInputElement | null)[]>([]);

  const ticking = left > 0;
  useEffect(() => {
    if (!ticking) return;
    const id = setInterval(() => setLeft((l) => Math.max(0, l - 1)), 1000);
    return () => clearInterval(id);
  }, [ticking]);

  const go = (s: Step) => {
    setStep(s);
    if (s < 3) {
      setPassword("");
      setConfirm("");
      setTried(false);
    }
    if (s === 2) {
      setCode(EMPTY_CODE);
      setTimeout(() => boxes.current[0]?.focus());
    }
  };

  /** 서버 오류 → 해당 단계로 돌려보내고 문구. */
  const fail = (err: unknown) => {
    const c = errCode(err);
    if (c === "EMAIL_TAKEN") go(1);
    else if (c === "CODE_INVALID") go(2);
    setError(authErrorText(err));
  };

  const run = async (f: () => Promise<void>) => {
    setError(null);
    setBusy(true);
    try {
      await f();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const sendCode = () =>
    run(async () => {
      await api.sendCode(email.trim());
      setLeft(RESEND_SECONDS);
      go(2);
    });

  const issues = checkPassword(password);
  const mismatch = confirm !== "" && confirm !== password;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (step === 1) return sendCode();
    if (step === 2)
      return run(async () => {
        await api.verifyCode(email.trim(), code.join(""));
        go(3);
      });
    setTried(true);
    if (issues.length || password !== confirm) return;
    return run(async () => {
      if (!registered) {
        await api.register(email.trim(), password, name.trim());
        setRegistered(true);
      }
      await login(email.trim(), password);
    });
  };

  const fill = (from: number, digits: string) => {
    if (!digits) return;
    const start = digits.length >= 6 ? 0 : from;
    const next = [...code];
    for (let i = 0; i < digits.length && start + i < 6; i++) next[start + i] = digits[i];
    setCode(next);
    boxes.current[Math.min(start + digits.length, 5)]?.focus();
  };
  const onDigit = (i: number, value: string) => {
    const d = value.replace(/\D/g, "");
    if (!value) setCode(code.map((c, j) => (j === i ? "" : c)));
    else if (d.length >= 6) fill(0, d.slice(0, 6)); // 자동 채움
    else if (d) fill(i, d.slice(-1));
  };
  const onPaste = (i: number, e: ClipboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    fill(i, e.clipboardData.getData("text").replace(/\D/g, "").slice(0, 6));
  };
  const onKey = (i: number, e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Backspace" && !code[i] && i > 0) boxes.current[i - 1]?.focus();
  };

  const eye = (
    <button
      type="button"
      className="text-neutral-400 hover:text-main-deep-blue"
      aria-label={t(show ? "auth.hidePassword" : "auth.showPassword")}
      onClick={() => setShow((v) => !v)}
    >
      {show ? <Eye className="size-4.5" aria-hidden="true" /> : <EyeOff className="size-4.5" aria-hidden="true" />}
    </button>
  );

  const mm = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
  const ready = step === 1 ? !!email.trim() : step === 2 ? code.every(Boolean) : !!name.trim() && !!password && !!confirm;

  return (
    <>
      <button type="button" className="mb-6 flex items-center gap-1 typo-body3_medium text-neutral-500 hover:text-main-deep-blue" onClick={onBack}>
        <ChevronLeft className="size-4" aria-hidden="true" />
        {t("auth.toLogin")}
      </button>
      <h1 className="typo-title2_bold text-main-deep-blue">{t("auth.signup")}</h1>
      <Steps step={step} />
      <form className="mt-10 space-y-6" onSubmit={submit}>
        {step === 1 ? (
          <Field
            label={t("settings.email")}
            icon={AtSign}
            type="email"
            autoComplete="email"
            placeholder={t("auth.emailPlaceholder")}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        ) : (
          <div>
            <p className="mb-2 pl-3 typo-body1_medium text-main-deep-blue">{t("settings.email")}</p>
            <div className="flex min-h-12.5 items-center gap-2 rounded-brand-md bg-neutral-50 px-3 ring-1 ring-neutral-200">
              <AtSign className="size-5 text-neutral-400" aria-hidden="true" />
              <input aria-label={t("settings.email")} readOnly value={email} className="min-w-0 flex-1 bg-transparent typo-body2_medium text-neutral-600 outline-none" />
              <button type="button" className="typo-body3_semibold text-main-blue hover:underline" onClick={() => go(1)}>
                {t("auth.changeEmail")}
              </button>
            </div>
          </div>
        )}
        {step === 2 && (
          <div>
            <p className="mb-2 pl-3 typo-body1_medium text-main-deep-blue">{t("auth.step.code")}</p>
            <div className="flex justify-between gap-2">
              {code.map((c, i) => (
                <input
                  key={i}
                  ref={(el) => {
                    boxes.current[i] = el;
                  }}
                  aria-label={`${t("auth.step.code")} ${i + 1}`}
                  inputMode="numeric"
                  autoComplete={i === 0 ? "one-time-code" : "off"}
                  maxLength={6}
                  value={c}
                  onChange={(e) => onDigit(i, e.target.value)}
                  onPaste={(e) => onPaste(i, e)}
                  onKeyDown={(e) => onKey(i, e)}
                  className="h-14 w-12 rounded-brand-md bg-main-light-blue text-center text-2xl font-bold text-main-deep-blue shadow-[inset_0_0_0_2px_color-mix(in_srgb,var(--color-sub-blue)_20%,transparent)] outline-none focus:ring-2 focus:ring-main-blue/40"
                />
              ))}
            </div>
            <p className="mt-3 pl-3 typo-caption1_regular text-neutral-500">{t("auth.codeHint").replace("{email}", email.trim())}</p>
          </div>
        )}
        {step === 3 && (
          <>
            <Field label={t("auth.name")} icon={UserRound} autoComplete="name" placeholder={t("auth.namePlaceholder")} value={name} onChange={(e) => setName(e.target.value)} />
            <div>
              <Field
                label={t("settings.password")}
                icon={LockKeyhole}
                type={show ? "text" : "password"}
                autoComplete="new-password"
                placeholder={t("auth.passwordPlaceholder")}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                after={eye}
              />
              <ul className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 pl-3">
                {PASSWORD_RULES.map((rule) => {
                  const ok = !issues.includes(rule);
                  const bad = !ok && (tried || (rule === "ascii" && !!password));
                  return (
                    <li
                      key={rule}
                      data-ok={ok}
                      className={`flex items-center gap-1.5 typo-caption1_medium ${ok ? "text-main-blue" : bad ? "text-danger-700" : "text-neutral-400"}`}
                    >
                      {ok ? <Check className="size-3.5" aria-hidden="true" /> : <span aria-hidden="true">•</span>}
                      {t(`auth.pw.${rule}`)}
                    </li>
                  );
                })}
              </ul>
            </div>
            <div>
              <Field
                label={t("auth.passwordConfirm")}
                icon={LockKeyhole}
                type={show ? "text" : "password"}
                autoComplete="new-password"
                placeholder={t("auth.passwordConfirmPlaceholder")}
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
              />
              {mismatch && <p className="mt-2 pl-3 typo-caption1_semibold text-danger-700">{t("auth.pw.mismatch")}</p>}
            </div>
          </>
        )}
        <ErrorText msg={error} />
        <button type="submit" className={SUBMIT} disabled={busy || !ready}>
          {t(step === 1 ? "auth.sendCode" : step === 2 ? "auth.verify" : "auth.register")}
        </button>
      </form>
      {step === 2 && (
        <p className="mt-5 text-center typo-body3_medium text-neutral-500">
          {t("auth.noMail")}{" "}
          <button type="button" className="typo-body3_semibold text-main-blue hover:underline disabled:text-neutral-400 disabled:no-underline" disabled={busy || ticking} onClick={sendCode}>
            {t("auth.resend")}
            {ticking && ` (${mm})`}
          </button>
        </p>
      )}
    </>
  );
}
