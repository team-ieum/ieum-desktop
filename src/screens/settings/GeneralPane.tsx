import { ExternalLink, RefreshCw } from "lucide-react";
import { useState } from "react";
import { api, type Settings, type UpdateCheck } from "../../api";
import { t } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { GHOST, Group, Row } from "./Row";

/** 웹 ActiveToggle 모양 — 켜짐은 이음 남색. */
export function Switch({ on, label, disabled, onChange }: { on: boolean; label: string; disabled: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200 disabled:opacity-40 ${on ? "bg-main-deep-blue" : "bg-neutral-300"}`}
    >
      <span className={`absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200 ${on ? "translate-x-5" : ""}`} />
    </button>
  );
}

/** 설정 한 칸 바꿔 저장 — 실패하면 `error`에 Rust 오류. */
export function useSettingsUpdate() {
  const settings = useSettings((s) => s.settings);
  const save = useSettings((s) => s.save);
  const [error, setError] = useState<string | null>(null);

  const update = async (patch: Partial<Settings>) => {
    if (!settings) return;
    setError(null);
    try {
      await save({ ...settings, ...patch });
    } catch (e) {
      setError(String(e));
    }
  };
  return { settings, update, error, setError };
}

const BASIC = () => [t("settings.language"), t("settings.lang.system"), t("settings.lang.ko"), t("settings.lang.en")];
const CONFIRM = () => [t("settings.confirmDeleteWorkflow"), t("settings.confirmDeleteWorkflowDesc"), t("settings.confirmDiscard"), t("settings.confirmDiscardDesc")];
const appName = (version: string) => `IEUM desktop${version && ` v${version}`}`;
const ABOUT = (version: string) => [appName(version), t("settings.checkUpdate"), t("settings.updateAuto"), t("settings.help"), t("settings.helpDesc"), t("settings.openHelp")];

/** 설정 검색이 쓰는 이 페이지의 문구 — 박스 제목 포함. */
export const generalTexts = (version: string) => [
  t("settings.section.basic"),
  ...BASIC(),
  t("settings.section.confirm"),
  ...CONFIRM(),
  t("settings.section.about"),
  ...ABOUT(version),
];

const updateText = (r: UpdateCheck) =>
  r.status === "installed" ? t("settings.updateInstalled").replace("{version}", r.version) : t(r.status === "dev" ? "settings.updateDev" : "settings.updateLatest");

/** 언어 · 확인 창 · 정보(버전·업데이트 확인·안내). */
export function GeneralPane({ version }: { version: string }) {
  const { settings, update, error } = useSettingsUpdate();
  const [checking, setChecking] = useState(false);
  const [updateMsg, setUpdateMsg] = useState<{ text: string; failed: boolean } | null>(null);

  const checkUpdate = async () => {
    setChecking(true);
    setUpdateMsg(null);
    try {
      setUpdateMsg({ text: updateText(await api.checkUpdateNow()), failed: false });
    } catch (e) {
      console.error("[ieum] 업데이트 확인 실패", e);
      setUpdateMsg({ text: t("settings.updateFailed"), failed: true });
    } finally {
      setChecking(false);
    }
  };

  return (
    <>
      <Group title={t("settings.section.basic")} texts={BASIC()}>
        <Row title={t("settings.language")} also={[t("settings.lang.system"), t("settings.lang.ko"), t("settings.lang.en")]}>
          <select
            aria-label={t("settings.language")}
            className="h-9 shrink-0 rounded-[10px] border border-[#cde9f4] bg-white px-3 text-sm text-neutral-700 outline-none focus:border-main-blue"
            value={settings?.language ?? "system"}
            disabled={!settings}
            onChange={(e) => void update({ language: e.target.value as Settings["language"] })}
          >
            <option value="system">{t("settings.lang.system")}</option>
            <option value="ko">{t("settings.lang.ko")}</option>
            <option value="en">{t("settings.lang.en")}</option>
          </select>
        </Row>
      </Group>
      <Group title={t("settings.section.confirm")} texts={CONFIRM()}>
        <Row title={t("settings.confirmDeleteWorkflow")} desc={t("settings.confirmDeleteWorkflowDesc")}>
          <Switch
            on={settings?.confirmDeleteWorkflow !== false}
            label={t("settings.confirmDeleteWorkflow")}
            disabled={!settings}
            onChange={(v) => void update({ confirmDeleteWorkflow: v })}
          />
        </Row>
        <Row title={t("settings.confirmDiscard")} desc={t("settings.confirmDiscardDesc")}>
          <Switch
            on={settings?.confirmDiscardUnsaved !== false}
            label={t("settings.confirmDiscard")}
            disabled={!settings}
            onChange={(v) => void update({ confirmDiscardUnsaved: v })}
          />
        </Row>
      </Group>
      <Group title={t("settings.section.about")} texts={ABOUT(version)}>
        <Row
          title={appName(version)}
          desc={
            updateMsg ? <span className={updateMsg.failed ? "text-danger-700" : undefined}>{updateMsg.text}</span> : t("settings.updateAuto")
          }
          also={[t("settings.checkUpdate"), t("settings.updateAuto")]}
        >
          <button type="button" className={GHOST} disabled={checking} onClick={() => void checkUpdate()}>
            <RefreshCw className={`size-[15px] ${checking ? "animate-spin" : ""}`} aria-hidden="true" />
            {t(checking ? "settings.checkingUpdate" : "settings.checkUpdate")}
          </button>
        </Row>
        <Row title={t("settings.help")} desc={t("settings.helpDesc")} also={[t("settings.openHelp")]}>
          <button
            type="button"
            className={GHOST}
            onClick={() => api.openHelp().catch((e: unknown) => console.error("[ieum] 도움말을 열지 못했어요", e))}
          >
            <ExternalLink className="size-[15px]" aria-hidden="true" />
            {t("settings.openHelp")}
          </button>
        </Row>
      </Group>
      {error && <p className="pb-3 typo-caption1_semibold text-danger-700">{error}</p>}
    </>
  );
}
