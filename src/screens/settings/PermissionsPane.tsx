import { api, type KeychainStatus, type Providers, type Settings } from "../../api";
import { t, type Key } from "../../i18n";
import { FoldersStep } from "../../onboarding/Onboarding";
import { cliName, PROVIDERS } from "./AiPane";
import { Switch, useSettingsUpdate } from "./GeneralPane";
import { GHOST, Row } from "./Row";

type NotifyKey = "notifyManualStart" | "notifyManualEnd" | "notifyWatchEnd";
const NOTIFY: { key: NotifyKey; label: Key }[] = [
  { key: "notifyManualStart", label: "settings.notifyManualStart" },
  { key: "notifyManualEnd", label: "settings.notifyManualEnd" },
  { key: "notifyWatchEnd", label: "settings.notifyWatchEnd" },
];

/** 연결된 AI 한 줄 — 감지된 CLI + 키체인에 키가 있는 공급원. */
export const aiStatus = (providers: Providers | null, keychain: KeychainStatus | null) => {
  const names = [...(providers?.clis ?? []).map((c) => cliName(c.id)), ...PROVIDERS.filter((p) => keychain?.[p.id] === "stored").map((p) => p.name)];
  return names.length ? t("settings.aiConnected").replace("{names}", names.join(", ")) : t("settings.aiNone");
};

/** 권한·준비 — 업데이트 뒤 풀린 권한을 여기서 복구한다. 폴더는 열 때마다 다시 확인. */
export function PermissionsPane({ providers, keychain, onAi }: { providers: Providers | null; keychain: KeychainStatus | null; onAi: () => void }) {
  const { settings, update, error, setError } = useSettingsUpdate();
  const run = (call: Promise<void>) => {
    setError(null);
    call.catch((e) => setError(String(e)));
  };
  const toggle = (patch: Partial<Settings>) => void update(patch);

  return (
    <>
      <FoldersStep checkOnOpen />
      <Row title={t("settings.notifyTest")} desc={t("settings.notifyTestDesc")} also={[t("settings.notifyTestSend"), t("onboarding.openSettings")]}>
        <div className="flex items-center gap-2">
          <button type="button" className={GHOST} onClick={() => run(api.openSystemSettings("notifications"))}>
            {t("onboarding.openSettings")}
          </button>
          <button type="button" className={GHOST} onClick={() => run(api.sendTestNotification())}>
            {t("settings.notifyTestSend")}
          </button>
        </div>
      </Row>
      {NOTIFY.map(({ key, label }) => (
        <Row key={key} title={t(label)}>
          <Switch on={settings?.[key] ?? true} label={t(label)} disabled={!settings} onChange={(v) => toggle({ [key]: v })} />
        </Row>
      ))}
      <Row title={t("settings.autostart")} desc={t("settings.autostartDesc")} also={[t("settings.loginItems")]}>
        <div className="flex items-center gap-3">
          <button type="button" className={GHOST} onClick={() => run(api.openSystemSettings("login_items"))}>
            {t("settings.loginItems")}
          </button>
          <Switch on={settings?.autostart ?? false} label={t("settings.autostart")} disabled={!settings} onChange={(v) => toggle({ autostart: v })} />
        </div>
      </Row>
      <Row title={t("settings.aiStatus")} desc={aiStatus(providers, keychain)} also={[t("settings.toAi")]}>
        <button type="button" className={GHOST} onClick={onAi}>
          {t("settings.toAi")}
        </button>
      </Row>
      <Row title={t("settings.replayOnboarding")} desc={t("settings.replayOnboardingDesc")}>
        <button type="button" className={GHOST} disabled={!settings} onClick={() => toggle({ onboardingDone: false })}>
          {t("settings.replayOnboarding")}
        </button>
      </Row>
      {error && <p className="pb-3 typo-caption1_semibold text-danger-700">{error}</p>}
    </>
  );
}
