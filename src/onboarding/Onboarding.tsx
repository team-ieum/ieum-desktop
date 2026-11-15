import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { ProviderLogo } from "../assets/providers";
import { t, type Key } from "../i18n";
import { BUTTON, PRIMARY } from "../screens/Editor";
import { TOP_ROW } from "../shell/Sidebar";
import { AiProvidersPane, usePlans } from "../screens/settings/AiPane";
import { Switch } from "../screens/settings/GeneralPane";
import { useCredentials } from "../stores/credentials";
import { useSettings } from "../stores/settings";

const STEPS: { title: Key; desc: Key }[] = [
  { title: "onboarding.ai.title", desc: "onboarding.ai.desc" },
  { title: "onboarding.notify.title", desc: "onboarding.notify.desc" },
  { title: "onboarding.autostart.title", desc: "onboarding.autostart.desc" },
];
const NOTIFY = 1;

/** 1단계 — 설정 "AI 제공자"와 같은 카드(구독 3장 + API 키 3장). 길어서 상자 안에서 스크롤. */
function AiStep() {
  const view = useCredentials((s) => s.view);
  const { plans, refresh: refreshPlan } = usePlans(view);
  const refresh = useCallback(() => {
    useCredentials.getState().load().catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);
  return (
    <div className="max-h-[360px] overflow-y-auto pr-1">
      <AiProvidersPane view={view} plans={plans} refreshPlan={refreshPlan} refreshCreds={refresh} />
    </div>
  );
}

/** `failed` = 테스트 알림을 보내지 못함 — 묻지 않고 오류·설정 열기·다시 보내기만. */
type Notice = "idle" | "asked" | "yes" | "no" | "failed";

/** 2단계 — 테스트 알림 → 보였는지 묻는다. 안 보였으면 설정 열기·다시 보내기. */
function NotifyStep({ notice, setNotice }: { notice: Notice; setNotice: (n: Notice) => void }) {
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const send = async () => {
    setError(null);
    setSending(true);
    try {
      await api.sendTestNotification();
      setNotice("asked");
    } catch (e) {
      setError(String(e));
      setNotice("failed");
    }
    setSending(false);
  };
  const openNotifications = () => {
    setError(null);
    api.openSystemSettings("notifications").catch((e) => setError(String(e)));
  };
  return (
    <div className="flex flex-col items-start gap-4">
      {notice === "idle" && (
        <button type="button" className={PRIMARY} disabled={sending} onClick={() => void send()}>
          {t("onboarding.notify.enable")}
        </button>
      )}
      {notice !== "idle" && notice !== "failed" && (
        <div className="flex items-center gap-3">
          <span className="typo-body2_semibold text-neutral-800">{t("onboarding.notify.seen")}</span>
          <button type="button" className={notice === "yes" ? PRIMARY : BUTTON} onClick={() => setNotice("yes")}>
            {t("onboarding.notify.yes")}
          </button>
          <button type="button" className={notice === "no" ? PRIMARY : BUTTON} onClick={() => setNotice("no")}>
            {t("onboarding.notify.no")}
          </button>
        </div>
      )}
      {(notice === "no" || notice === "failed") && (
        <div className="flex items-center gap-2">
          <button type="button" className={BUTTON} onClick={openNotifications}>
            {t("onboarding.openSettings")}
          </button>
          <button type="button" className={BUTTON} disabled={sending} onClick={() => void send()}>
            {t("onboarding.notify.resend")}
          </button>
        </div>
      )}
      {error && <p className="typo-caption1_semibold text-danger-700">{error}</p>}
    </div>
  );
}

/** 첫 실행 온보딩 3단계. [시작하기]가 저장에 성공해야 끝 — 그 전에 끄면 다음에 1단계부터. */
export function Onboarding({ onDone }: { onDone: () => void }) {
  const settings = useSettings((s) => s.settings);
  const [step, setStep] = useState(0);
  const [notice, setNotice] = useState<Notice>("idle");
  // 새 설치(첫 실행 전)면 켜짐 제안, v0.1.0에서 올라온 사람은 지금 값 그대로.
  const [autostart, setAutostart] = useState(() => !!settings && (settings.autostart || !settings.firstRunDone));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const last = step === STEPS.length - 1;

  const finish = async () => {
    if (!settings) return;
    setError(null);
    setSaving(true);
    try {
      await api.setSettings({ ...settings, onboardingDone: true, firstRunDone: true, autostart });
      onDone();
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };

  return (
    <div className="relative flex h-full items-center justify-center overflow-y-auto bg-main-light-blue px-6 pt-14 pb-10">
      {/* 창 끌기 영역 — 신호등 줄 높이. */}
      <div data-tauri-drag-region className={`absolute inset-x-0 top-0 ${TOP_ROW}`} />
      <div className="w-full max-w-[560px] rounded-brand-lg bg-white px-10 py-9 shadow-[0_12px_32px_-12px_rgba(41,83,124,.25)]">
        <div className="flex items-center justify-between">
          <ProviderLogo id="ieum" size={32} />
          <ol className="flex gap-2" aria-label={t("onboarding.progress")}>
            {STEPS.map((s, i) => (
              <li
                key={s.title}
                aria-current={i === step ? "step" : undefined}
                className={`h-2 rounded-full transition-all ${i === step ? "w-6 bg-main-deep-blue" : i < step ? "w-2 bg-main-blue" : "w-2 bg-neutral-200"}`}
              />
            ))}
          </ol>
        </div>
        <h1 className="mt-6 typo-title2_bold text-main-deep-blue">{t(STEPS[step].title)}</h1>
        <p className="mt-2 typo-body2_medium text-neutral-500">{t(STEPS[step].desc)}</p>
        <div className="mt-6 min-h-[180px]">
          {step === 0 && <AiStep />}
          {step === NOTIFY && <NotifyStep notice={notice} setNotice={setNotice} />}
          {last && (
            <div className="flex items-center justify-between gap-6 rounded-brand-md bg-neutral-50 px-4 py-3.5">
              <div className="min-w-0">
                <div className="typo-body2_semibold text-neutral-800">{t("settings.autostart")}</div>
                <div className="mt-0.5 typo-caption1_regular text-neutral-500">{t("onboarding.autostart.note")}</div>
              </div>
              <Switch on={autostart} label={t("settings.autostart")} disabled={saving} onChange={setAutostart} />
            </div>
          )}
        </div>
        {error && <p className="mt-3 typo-caption1_semibold text-danger-700">{error}</p>}
        <div className="mt-8 flex items-center justify-end gap-2">
          {step !== NOTIFY && !last && (
            <button type="button" className={BUTTON} onClick={() => setStep(step + 1)}>
              {t("onboarding.skip")}
            </button>
          )}
          {last ? (
            <button type="button" className={PRIMARY} disabled={!settings || saving} onClick={() => void finish()}>
              {t("onboarding.start")}
            </button>
          ) : (
            <button
              type="button"
              className={PRIMARY}
              disabled={step === NOTIFY && (notice === "idle" || notice === "asked")}
              onClick={() => setStep(step + 1)}
            >
              {t("onboarding.next")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
