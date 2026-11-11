import { create } from "zustand";
import { api, type Settings } from "../api";
import { resolveLang, setLang, type Lang } from "../i18n";

// 읽기만(언어). 설정 화면·저장은 16b.
interface SettingsState {
  settings: Settings | null;
  lang: Lang;
  load: () => Promise<void>;
}

export const useSettings = create<SettingsState>((set) => ({
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
}));

setLang(useSettings.getState().lang);
