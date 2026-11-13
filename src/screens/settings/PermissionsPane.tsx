import { FolderOpen } from "lucide-react";
import { api, type Settings } from "../../api";
import { t, type Key } from "../../i18n";
import { useWeb } from "../../stores/web";
import { Switch, useSettingsUpdate } from "./GeneralPane";
import { GHOST, Row } from "./Row";

type NotifyKey = "notifyManualStart" | "notifyManualEnd" | "notifyWatchEnd";
const NOTIFY: { key: NotifyKey; label: Key }[] = [
  { key: "notifyManualStart", label: "settings.notifyManualStart" },
  { key: "notifyManualEnd", label: "settings.notifyManualEnd" },
  { key: "notifyWatchEnd", label: "settings.notifyWatchEnd" },
];

/** 버튼 오류를 줄 아래에 보여주는 설정 한 칸 바꾸기. */
function useRun() {
  const { settings, update, error, setError } = useSettingsUpdate();
  const run = (call: Promise<void>) => {
    setError(null);
    call.catch((e) => setError(String(e)));
  };
  const toggle = (patch: Partial<Settings>) => void update(patch);
  return { settings, run, toggle, error };
}

const ErrorLine = ({ error }: { error: string | null }) => (error ? <p className="pb-3 typo-caption1_semibold text-danger-700">{error}</p> : null);

export const notificationTexts = () => [
  t("settings.notifyTest"),
  t("settings.notifyTestDesc"),
  t("settings.notifyTestSend"),
  t("onboarding.openSettings"),
  ...NOTIFY.map((n) => t(n.label)),
];

/** 알림 — 테스트 알림·시스템 설정 알림 열기, 알림 토글 3개. */
export function NotificationsPane() {
  const { settings, run, toggle, error } = useRun();
  return (
    <>
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
      <ErrorLine error={error} />
    </>
  );
}

export const macosTexts = () => [t("settings.filesHint"), t("onboarding.openSettings"), t("settings.autostart"), t("settings.autostartDesc"), t("settings.loginItems")];

/** macOS 권한 — 업데이트 뒤 풀린 권한을 여기서 복구한다. 폴더는 확인하지 않고 시스템 설정 길만. */
export function MacosPane() {
  const { settings, run, toggle, error } = useRun();
  return (
    <>
      <Row title={t("settings.filesHint")} also={[t("onboarding.openSettings")]}>
        <button type="button" className={GHOST} onClick={() => run(api.openSystemSettings("files"))}>
          {t("onboarding.openSettings")}
        </button>
      </Row>
      <Row title={t("settings.autostart")} desc={t("settings.autostartDesc")} also={[t("settings.loginItems")]}>
        <div className="flex items-center gap-3">
          <button type="button" className={GHOST} onClick={() => run(api.openSystemSettings("login_items"))}>
            {t("settings.loginItems")}
          </button>
          <Switch on={settings?.autostart ?? false} label={t("settings.autostart")} disabled={!settings} onChange={(v) => toggle({ autostart: v })} />
        </div>
      </Row>
      <ErrorLine error={error} />
    </>
  );
}

/** 서버 주소 — 로그인한 계정의 주소, 없으면 저장된 설정. 바꾸는 곳은 로그인 관문. */
const useServerUrl = () => {
  const account = useWeb((s) => s.account);
  const { settings } = useSettingsUpdate();
  return account?.serverUrl ?? settings?.serverUrl ?? "";
};

export const useAdvancedTexts = () => {
  const url = useServerUrl();
  return [
    t("settings.server"),
    url,
    t("settings.dataFolder"),
    t("settings.dataFolderDesc"),
    t("settings.openDataFolder"),
    t("settings.replayOnboarding"),
    t("settings.replayOnboardingDesc"),
  ];
};

/** 고급 — 서버 주소(표시만)·데이터 폴더 열기·온보딩 다시 보기. */
export function AdvancedPane() {
  const { settings, run, toggle, error } = useRun();
  const url = useServerUrl();
  return (
    <>
      <Row title={t("settings.server")} desc={url} />
      <Row title={t("settings.dataFolder")} desc={t("settings.dataFolderDesc")} also={[t("settings.openDataFolder")]}>
        <button type="button" className={GHOST} onClick={() => run(api.openDataFolder())}>
          <FolderOpen className="size-[15px]" aria-hidden="true" />
          {t("settings.openDataFolder")}
        </button>
      </Row>
      <Row title={t("settings.replayOnboarding")} desc={t("settings.replayOnboardingDesc")}>
        <button type="button" className={GHOST} disabled={!settings} onClick={() => toggle({ onboardingDone: false })}>
          {t("settings.replayOnboarding")}
        </button>
      </Row>
      <ErrorLine error={error} />
    </>
  );
}

/** 코드에 실제로 걸린 단축키만(App ⌘N·⌘K, 설정 ⌘F, Esc, 캔버스 ⌫, 찾기 ↑↓↵). */
const SHORTCUTS: { keys: string[]; label: Key }[] = [
  { keys: ["⌘K"], label: "nav.search" },
  { keys: ["⌘N"], label: "nav.newCanvas" },
  { keys: ["⌘F"], label: "settings.search" },
  { keys: ["Esc"], label: "settings.shortcut.close" },
  { keys: ["⌫"], label: "settings.shortcut.delete" },
  { keys: ["↑", "↓"], label: "settings.shortcut.move" },
  { keys: ["↵"], label: "settings.shortcut.open" },
];
const KBD = "inline-flex h-6 min-w-6 items-center justify-center rounded-md border border-[#cde9f4] bg-main-light-blue px-1.5 font-mono text-[12px] font-semibold text-main-deep-blue";

export const shortcutTexts = () => SHORTCUTS.flatMap((s) => [t(s.label), ...s.keys]);

/** 단축키 — 읽기 전용 목록. */
export function ShortcutsPane() {
  return (
    <>
      {SHORTCUTS.map((s) => (
        <Row key={s.label} title={t(s.label)} also={s.keys}>
          <span className="flex items-center gap-1">
            {s.keys.map((k) => (
              <kbd key={k} className={KBD}>
                {k}
              </kbd>
            ))}
          </span>
        </Row>
      ))}
    </>
  );
}
