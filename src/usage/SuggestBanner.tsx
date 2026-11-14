import { useEffect, useState } from "react";
import type { Suggestion } from "../api";
import { t, type Key } from "../i18n";
import { BUTTON, PRIMARY } from "../screens/Editor";
import { useCredentials } from "../stores/credentials";
import { useDefaultPlans } from "../stores/plans";
import { formatReset } from "./format";
import { SwitchAccountDialog } from "./SwitchAccount";

const NAME = { claude: "Claude", codex: "Codex" } as const;

/** 권하기 띠 자리 — 상태 영역은 비어 있어도 늘 두고 내용만 바꾼다(화면 낭독기가 바뀐 내용을 읽게). */
export function SuggestBanner({ s, onClose }: { s: Suggestion | null; onClose: () => void }) {
  return (
    <div role="status">
      {s && <Banner key={`${s.company}:${s.toId}:${s.resetsAt}`} s={s} onClose={onClose} />}
    </div>
  );
}

/** 띠가 닫히면 포커스를 본문으로(버튼이 사라져 body로 떨어지지 않게). */
function focusMain() {
  const m = document.querySelector<HTMLElement>("main");
  if (!m) return;
  if (!m.hasAttribute("tabindex")) m.tabIndex = -1;
  m.focus();
}

/** 기본 계정이 한도에 닿았을 때 본문 위에 뜨는 권하기 띠. [바꾸기] → 확인 창, 창이 닫히면 띠도 닫는다. */
function Banner({ s, onClose }: { s: Suggestion; onClose: () => void }) {
  const [switching, setSwitching] = useState(false);
  // 바꾼 뒤엔 새 기본 계정(다시 읽은 자격 목록 기준)의 사용량을 캐시 없이 읽고 닫는다.
  const { refreshNow } = useDefaultPlans(useCredentials((c) => c.view));
  const [switched, setSwitched] = useState(false);
  const close = () => {
    onClose();
    focusMain();
  };
  useEffect(() => {
    if (!switched) return;
    refreshNow();
    close();
  }, [switched]);
  const text = t("usage.suggest.text")
    .replace("{company}", NAME[s.company])
    .replace("{window}", t(`usage.window.${s.windowKind}` as Key))
    .replace("{reset}", formatReset(s.resetsAt, new Date()))
    .replace("{to}", s.toLabel)
    .replace("{pct}", String(Math.round(s.toUsedPct)));
  return (
    <div className="flex items-center gap-3 border-b border-[#cde9f4] bg-[#eef8fc] px-4 py-2 typo-body3_semibold text-main-deep-blue">
      <span className="min-w-0 flex-1">{text}</span>
      <button type="button" className={BUTTON} onClick={close}>
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
          onClose={(ok) => (ok ? setSwitched(true) : close())}
        />
      )}
    </div>
  );
}
