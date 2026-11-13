import { useEffect, useState } from "react";
import type { Suggestion } from "../api";
import { t, type Key } from "../i18n";
import { BUTTON, PRIMARY } from "../screens/Editor";
import { useCredentials } from "../stores/credentials";
import { useDefaultPlans } from "../stores/plans";
import { formatReset } from "./format";
import { SwitchAccountDialog } from "./SwitchAccount";

const NAME = { claude: "Claude", codex: "Codex" } as const;

/** 기본 계정이 한도에 닿았을 때 본문 위에 뜨는 권하기 띠. [바꾸기] → 확인 창, 창이 닫히면 띠도 닫는다. */
export function SuggestBanner({ s, onClose }: { s: Suggestion; onClose: () => void }) {
  const [switching, setSwitching] = useState(false);
  // 바꾼 뒤엔 새 기본 계정(다시 읽은 자격 목록 기준)의 사용량을 캐시 없이 읽고 닫는다.
  const { refreshNow } = useDefaultPlans(useCredentials((c) => c.view));
  const [switched, setSwitched] = useState(false);
  useEffect(() => {
    if (!switched) return;
    refreshNow();
    onClose();
  }, [switched]);
  const text = t("usage.suggest.text")
    .replace("{company}", NAME[s.company])
    .replace("{window}", t(`usage.window.${s.windowKind}` as Key))
    .replace("{reset}", formatReset(s.resetsAt, new Date()))
    .replace("{to}", s.toLabel)
    .replace("{pct}", String(Math.round(s.toUsedPct)));
  return (
    <div role="status" className="flex items-center gap-3 border-b border-[#cde9f4] bg-[#eef8fc] px-4 py-2 typo-body3_semibold text-main-deep-blue">
      <span className="min-w-0 flex-1">{text}</span>
      <button type="button" className={BUTTON} onClick={onClose}>
        {t("usage.suggest.later")}
      </button>
      <button type="button" className={PRIMARY} onClick={() => setSwitching(true)}>
        {t("usage.switch.confirm")}
      </button>
      {switching && (
        <SwitchAccountDialog
          kind={s.company}
          fromName={s.fromLabel}
          to={{ id: s.toId, name: s.toLabel, usedPct: Math.round(s.toUsedPct) }}
          onClose={(ok) => (ok ? setSwitched(true) : onClose())}
        />
      )}
    </div>
  );
}
