import { useState } from "react";
import { api } from "../api";
import { t } from "../i18n";
import { useCredentials } from "../stores/credentials";

const NAME = { claude: "Claude", codex: "Codex" } as const;
const BTN = "h-8 rounded-lg px-3 typo-caption1_semibold";

/** 기본 계정 바꾸기 확인 창 — 팝오버 계정 클릭과 권하기 띠가 같이 쓴다. 실패하면 창 안에 오류를 남기고 닫지 않는다. */
export function SwitchAccountDialog({
  kind,
  fromName,
  to,
  onClose,
}: {
  kind: "claude" | "codex";
  fromName: string;
  to: { id: string; name: string; usedPct: number | null };
  onClose: (switched: boolean) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(false);
  const title = t("usage.switch.title").replace("{company}", NAME[kind]);
  const line =
    `${fromName} → ${to.name}` + (to.usedPct === null ? "" : ` (${t("usage.used").replace("{pct}", String(to.usedPct))})`);
  const go = async () => {
    setBusy(true);
    setErr(false);
    try {
      await api.setDefaultCredential(kind, to.id);
      await useCredentials.getState().load();
      onClose(true);
    } catch {
      setErr(true);
      setBusy(false);
    }
  };
  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-neutral-900/20" onMouseDown={() => onClose(false)}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="w-96 rounded-xl border border-[#cde9f4] bg-white p-4 text-neutral-800 shadow-[0_20px_60px_-20px_rgba(41,83,124,.35)]"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          // 팝오버(화살표)·상태 줄(Esc 닫기)로 새지 않게.
          e.stopPropagation();
          if (e.key === "Escape") onClose(false);
        }}
      >
        <div className="typo-body3_semibold text-main-deep-blue">{title}</div>
        <p className="mt-1 typo-caption1_medium text-neutral-700">{line}</p>
        <p className="mt-3 typo-caption1_regular text-neutral-500">{t(kind === "claude" ? "usage.switch.noteClaude" : "usage.switch.noteCodex")}</p>
        {err && (
          <p role="alert" className="mt-2 typo-caption1_medium text-danger-700">
            {t("common.error")}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" className={`${BTN} text-neutral-600 hover:bg-neutral-100`} onClick={() => onClose(false)}>
            {t("common.cancel")}
          </button>
          <button autoFocus type="button" disabled={busy} className={`${BTN} bg-main-blue text-white hover:bg-main-deep-blue disabled:opacity-50`} onClick={go}>
            {t("usage.switch.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
