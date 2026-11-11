import { create } from "zustand";
import { api, type Settings } from "../api";
import { resolveLang, setLang, type Lang } from "../i18n";

interface SettingsState {
  settings: Settings | null;
  lang: Lang;
  load: () => Promise<void>;
  /** 저장 뒤 다시 읽는다(언어가 바뀌면 화면 전체가 다시 그려진다). 실패하면 Rust 오류 그대로 던진다. */
  save: (s: Settings) => Promise<void>;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  lang: resolveLang("system", navigator.language),
  load: async () => {
    let settings: Settings | null = null;
    try {
      settings = await api.getSettings();
    } catch (e) {
      console.error("[ieum] 설정을 읽지 못했어요", e);
    }
    const lang = resolveLang(settings?.language ?? "system", navigator.language);
    setLang(lang);
    set({ settings, lang });
  },
  save: async (s) => {
    await api.setSettings(s);
    await get().load();
  },
}));

setLang(useSettings.getState().lang);
