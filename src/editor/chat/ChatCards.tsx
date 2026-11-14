import { FlaskConical, Folder, FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type StoredRun } from "../../api";
import { type Key, t } from "../../i18n";

export const CARD =
  "rounded-xl border border-[#cde9f4] bg-white px-3 py-2.5 text-xs max-w-[280px] shadow-[0_4px_14px_-10px_rgba(41,83,124,.35)]";
const CARD_BUTTON =
  "inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-xl border border-main-blue px-3 py-2 text-xs font-semibold text-main-blue transition-colors hover:bg-main-blue/5 disabled:opacity-40";
/** 상태 점 색 — 상태 글이 같이 있어 색만으로 전하지 않는다. */
const DOT: Record<string, string> = { succeeded: "bg-[#2f9e6e]", success: "bg-[#2f9e6e]", failed: "bg-[#d0573b]" };
const Dot = ({ status }: { status: string }) => <span aria-hidden="true" className={`size-2 shrink-0 rounded-full ${DOT[status] ?? "bg-neutral-400"}`} />;

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
export function TestCard({ runId, status, nodeNames }: { runId: string; status: string; nodeNames?: Record<string, string> }) {
  const [run, setRun] = useState<StoredRun | null>(null);
  useEffect(() => {
    api.getRun(runId).then(setRun, () => {});
  }, [runId]);
  const nodes = Object.entries(run?.run.nodes ?? {}).filter(([, n]) => n.status !== "notRun");
  const skipped = run?.undo?.skipped ?? [];
  return (
    <div role="group" aria-label={t("chat.card.test")} className={`${CARD} flex flex-col gap-1.5 text-neutral-700`}>
      <p className="flex items-center gap-1.5 font-semibold text-main-deep-blue">
        <FlaskConical size={13} aria-hidden="true" />
        {t("chat.card.test")} · {statusText(status)}
      </p>
      {nodes.map(([id, n]) => (
        <div key={id} className="flex flex-col gap-0.5 border-t border-[#e8f1f5] pt-1.5">
          <p className="flex items-center gap-1.5 font-semibold">
            <Dot status={n.status} />
            <span>{nodeNames?.[id] ? `${nodeNames[id]} · ${statusText(n.status)}` : statusText(n.status)}</span>
          </p>
          {Object.entries(n.output ?? {}).map(([k, v]) => (
            <p key={k} className="break-all">
              {k}: {shown(v)}
            </p>
          ))}
          {n.error && <p className="break-all text-danger-700">{n.error}</p>}
        </div>
      ))}
      {skipped.length > 0 && (
        <div className="flex flex-col gap-0.5 border-t border-[#e8f1f5] pt-1.5">
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
      <p className="font-semibold text-main-deep-blue">{t("chat.card.pickFolder")}</p>
      {path !== null ? (
        <p className="flex items-start gap-1.5 break-all rounded-lg bg-main-light-blue/50 px-2 py-1.5 font-semibold text-neutral-900">
          <Folder size={13} className="mt-px shrink-0 text-main-blue" aria-hidden="true" />
          {path}
        </p>
      ) : (
        <button type="button" onClick={pick} disabled={disabled || picking} className={CARD_BUTTON}>
          <FolderOpen size={13} aria-hidden="true" />
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
