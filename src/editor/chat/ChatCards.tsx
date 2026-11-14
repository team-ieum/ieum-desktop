import { useEffect, useState } from "react";
import { api, type StoredRun } from "../../api";
import { type Key, t } from "../../i18n";

export const CARD = "rounded-xl border border-neutral-200 bg-white px-3 py-2.5 text-xs max-w-[280px]";
const CARD_BUTTON =
  "cursor-pointer rounded-xl border border-main-blue px-3 py-2 text-xs font-semibold text-main-blue hover:bg-main-blue/5 disabled:opacity-40";

/** 실행 기록·카드의 상태 글자 → 화면 글. 모르는 값은 그대로 보인다. */
const STATUS: Record<string, Key> = {
  succeeded: "chat.status.success",
  success: "chat.status.success",
  failed: "chat.status.failed",
  skipped: "chat.status.skipped",
};
const statusText = (s: string) => (STATUS[s] ? t(STATUS[s]) : s);
const shown = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v)).slice(0, 200);

/** 테스트 결과 — 값은 실행 기록에서 직접 읽어 화면에만 보인다(AI에게 다시 가지 않는다). */
export function TestCard({ runId, status }: { runId: string; status: string }) {
  const [run, setRun] = useState<StoredRun | null>(null);
  useEffect(() => {
    api.getRun(runId).then(setRun, () => {});
  }, [runId]);
  const nodes = Object.entries(run?.run.nodes ?? {}).filter(([, n]) => n.status !== "notRun");
  const skipped = run?.undo?.skipped ?? [];
  return (
    <div role="group" aria-label={t("chat.card.test")} className={`${CARD} flex flex-col gap-1.5 text-neutral-700`}>
      <p className="font-semibold text-neutral-900">
        {t("chat.card.test")} · {statusText(status)}
      </p>
      {nodes.map(([id, n]) => (
        <div key={id} className="flex flex-col gap-0.5 border-t border-neutral-100 pt-1.5">
          <p className="font-semibold">{statusText(n.status)}</p>
          {Object.entries(n.output ?? {}).map(([k, v]) => (
            <p key={k} className="break-all">
              {k}: {shown(v)}
            </p>
          ))}
          {n.error && <p className="break-all text-danger-700">{n.error}</p>}
        </div>
      ))}
      {skipped.length > 0 && (
        <div className="flex flex-col gap-0.5 border-t border-neutral-100 pt-1.5">
          <p className="font-semibold text-danger-700">{t("chat.card.undoFailed")}</p>
          {skipped.map(([op], i) => (
            <p key={i} className="break-all">
              {op.from}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}

/** 폴더 고르기 — 시스템 창은 이 버튼을 사람이 눌러야만 열린다. 고르면 `onPicked`로 다음 턴을 잇는다. */
export function PickFolderCard({
  workflowId,
  nodeId,
  field,
  disabled,
  onPicked,
}: {
  workflowId: string;
  nodeId: string;
  field: string;
  disabled: boolean;
  onPicked: () => void;
}) {
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const pick = async () => {
    setPicking(true);
    setError(null);
    try {
      const r = await api.chatPickFolder(workflowId, nodeId, field);
      if (r.ok) {
        setPath(String((r.data as { path?: unknown } | undefined)?.path ?? ""));
        onPicked();
      } else if (r.code !== "PICK_CANCELLED") setError(r.message ?? t("chat.error.failed"));
    } catch {
      setError(t("chat.error.failed"));
    } finally {
      setPicking(false);
    }
  };
  return (
    <div className={`${CARD} flex flex-col gap-2 text-neutral-700`}>
      <p>{t("chat.card.pickFolder")}</p>
      {path !== null ? (
        <p className="break-all font-semibold text-neutral-900">{path}</p>
      ) : (
        <button type="button" onClick={pick} disabled={disabled || picking} className={CARD_BUTTON}>
          {t("chat.card.pickButton")}
        </button>
      )}
      {error && (
        <p role="alert" className="text-danger-700">
          {error}
        </p>
      )}
    </div>
  );
}
