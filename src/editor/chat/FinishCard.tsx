import { useEffect, useState } from "react";
import { api } from "../../api";
import { t, tDyn } from "../../i18n";
import { BUTTON, PRIMARY } from "../../screens/Editor";
import { usePendingFinish } from "../../stores/pendingFinish";

/**
 * 완료 확인 카드 — 캔버스 위에 뜬다(패널이 닫혀 있어도 보인다). 띄운 것만으로는 켜지지 않고 [켜기]를 눌러야 켜진다.
 * `version`(저장본 updatedAt)이 바뀌면 다시 읽는다 — 워크플로우가 바뀌면 확인 대기가 사라진다.
 */
export function FinishCard({ workflowId, version }: { workflowId: string; version: string }) {
  const found = usePendingFinish((s) => s.pending[workflowId]);
  const refresh = usePendingFinish((s) => s.refresh);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    void refresh(workflowId);
  }, [workflowId, version, refresh]);
  if (!found) return null;

  const answer = async (accept: boolean) => {
    setBusy(true);
    setError("");
    try {
      const reply = await api.confirmFinish(workflowId, accept);
      if (!reply.ok) setError(reply.message ?? reply.code ?? "");
    } catch (e) {
      setError(String(e));
    }
    await refresh(workflowId);
    setBusy(false);
  };

  return (
    <div
      role="dialog"
      aria-label={t("finish.title")}
      className="absolute bottom-4 left-1/2 z-30 flex w-[420px] -translate-x-1/2 flex-col gap-3 rounded-2xl border border-neutral-200 bg-white p-5 shadow-[0_14px_40px_rgba(43,72,86,0.18)]"
    >
      <h2 className="typo-body2_bold text-neutral-900">{t("finish.title")}</h2>
      <dl className="flex flex-col gap-2 typo-caption1_medium text-neutral-700">
        <div>
          <dt className="text-neutral-500">{t("finish.folder")}</dt>
          <dd className="break-all font-mono text-xs">{found.folder}</dd>
        </div>
        {found.actions.length > 0 && (
          <div>
            <dt className="text-neutral-500">{t("finish.actions")}</dt>
            <dd>
              <ul className="flex flex-col gap-1">
                {found.actions.map((a) => {
                  const what = tDyn(`finish.action.${a.event}`)?.replace("{target}", a.target ?? "");
                  return (
                    <li key={a.nodeId} className="flex flex-col">
                      <span className="typo-caption1_semibold text-neutral-800">{a.label}</span>
                      {what && <span className="break-all">{what}</span>}
                    </li>
                  );
                })}
              </ul>
            </dd>
          </div>
        )}
        {(found.ai?.length ?? 0) > 0 && (
          <div>
            <dt className="text-neutral-500">{t("finish.ai")}</dt>
            <dd>
              <ul className="flex flex-col gap-1">
                {found.ai?.map((a) => (
                  <li key={a.nodeId} data-testid="finish-ai" className="flex flex-col">
                    <span className="typo-caption1_semibold text-neutral-800">{a.label || t("node.local.ai.title")}</span>
                    <span>{tDyn(`choice.provider.${a.provider}`) ?? a.provider}</span>
                    {/* 파일 내용이 밖으로 나가는 노드는 눈에 띄게. */}
                    <span className={a.sendContent ? "typo-caption1_semibold text-danger-700" : "text-neutral-500"}>
                      {t(a.sendContent ? "finish.ai.sendsContent" : "finish.ai.metaOnly")}
                    </span>
                  </li>
                ))}
              </ul>
            </dd>
          </div>
        )}
      </dl>
      {error && (
        <p role="alert" className="typo-caption1_semibold text-danger-700">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <button type="button" className={BUTTON} disabled={busy} onClick={() => void answer(false)}>
          {t("finish.decline")}
        </button>
        <button type="button" className={PRIMARY} disabled={busy} onClick={() => void answer(true)}>
          {t("finish.accept")}
        </button>
      </div>
    </div>
  );
}
