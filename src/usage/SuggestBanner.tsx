import type { Suggestion } from "../api";
import { t, type Key } from "../i18n";
import { BUTTON, PRIMARY } from "../screens/Editor";
import { formatReset } from "./format";

const NAME = { claude: "Claude", codex: "Codex" } as const;

/** 기본 계정이 한도에 닿았을 때 앱 위쪽에 뜨는 권하기 띠. */
export function SuggestBanner({ s, onSwitch, onLater }: { s: Suggestion; onSwitch: () => void; onLater: () => void }) {
  const text = t("usage.suggest.text")
    .replace("{company}", NAME[s.company])
    .replace("{window}", t(`usage.window.${s.windowKind}` as Key))
    .replace("{reset}", formatReset(s.resetsAt, new Date()))
    .replace("{to}", s.toLabel)
    .replace("{pct}", String(Math.round(s.toUsedPct)));
  return (
    <div role="status" className="flex items-center gap-3 border-b border-[#cde9f4] bg-[#eef8fc] px-4 py-2 typo-body3_semibold text-main-deep-blue">
      <span className="min-w-0 flex-1">{text}</span>
      <button type="button" className={BUTTON} onClick={onLater}>
        {t("usage.suggest.later")}
      </button>
      <button type="button" className={PRIMARY} onClick={onSwitch}>
        {t("usage.switch.confirm")}
      </button>
    </div>
  );
}
