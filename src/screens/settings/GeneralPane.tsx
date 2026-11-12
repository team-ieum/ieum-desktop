import { useState } from "react";
import type { Settings } from "../../api";
import { t } from "../../i18n";
import { useSettings } from "../../stores/settings";
import { Row } from "./Row";

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

export function GeneralPane() {
  const { settings, update, error } = useSettingsUpdate();

  return (
    <>
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
      {error && <p className="pb-3 typo-caption1_semibold text-danger-700">{error}</p>}
    </>
  );
}
