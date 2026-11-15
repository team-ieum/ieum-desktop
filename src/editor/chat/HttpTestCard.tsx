import { useEffect, useRef, useState } from "react";
import { api, type HttpLine } from "../../api";
import { t } from "../../i18n";
import { BUTTON, PRIMARY } from "../../screens/Editor";
import { nodeTitle } from "../Canvas";

/** 주소의 참조식을 가린다 — 화면 표시용. 값이 무엇이 될지는 사람이 알 수 없으니 자리만 보인다. */
export const showRefs = (url: string) => url.replace(/\{\{\s*nodes\.[^{}]*\}\}/g, t("http.confirm.earlier"));

/** 실제 요청을 보내기 전 확인 — 빌더 테스트(HttpTestCard)와 사람의 테스트 실행(useTestRun)이 같이 쓴다. */
export function HttpConfirm({
  lines,
  skipLabel,
  busy,
  onAnswer,
}: {
  lines: HttpLine[];
  skipLabel: string;
  busy?: boolean;
  onAnswer: (send: boolean) => void;
}) {
  return (
    <div
      role="dialog"
      aria-label={t("http.confirm.title")}
      className="absolute bottom-4 left-1/2 z-30 flex w-[420px] -translate-x-1/2 flex-col gap-3 rounded-2xl border border-neutral-200 bg-white p-5 shadow-[0_14px_40px_rgba(43,72,86,0.18)]"
    >
      <h2 className="typo-body2_bold text-neutral-900">{t("http.confirm.title")}</h2>
      <ul className="flex flex-col gap-2 typo-caption1_medium text-neutral-700">
        {lines.map((l) => (
          <li key={l.nodeId} className="flex flex-col">
            <span className="typo-caption1_semibold text-neutral-800">{l.label || nodeTitle("http.request")}</span>
            <span className="break-all font-mono text-xs">{`${l.method} ${showRefs(l.url)}`}</span>
          </li>
        ))}
      </ul>
      <p className="typo-caption1_semibold text-danger-700">{t("http.confirm.note")}</p>
      <div className="flex justify-end gap-2">
        {/* 처음 포커스는 보내지 않는 쪽 — Enter 한 번으로 요청이 나가지 않게. */}
        <button type="button" className={BUTTON} disabled={busy} autoFocus onClick={() => onAnswer(false)}>
          {skipLabel}
        </button>
        <button type="button" className={PRIMARY} disabled={busy} onClick={() => onAnswer(true)}>
          {t("http.confirm.send")}
        </button>
      </div>
    </div>
  );
}

/** 백엔드는 5분 뒤 대기를 취소한다 — 그 뒤 남은 카드는 답해도 소용없다. */
const WAIT_MS = 5 * 60_000;

/** 빌더 테스트의 HTTP 확인 — 이 편집 화면의 워크플로우 것만 띄운다. 화면을 떠나면 사라진다(백엔드가 시간 초과로 취소). */
export function HttpTestCard({ workflowId }: { workflowId: string }) {
  const [lines, setLines] = useState<HttpLine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const sent = useRef(false);
  useEffect(() => {
    let gone = false;
    let off: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    api
      .onHttpTestConfirm((p) => {
        if (p.id !== workflowId) return;
        sent.current = false;
        setBusy(false);
        setLines(p.lines);
        clearTimeout(timer);
        timer = setTimeout(() => setLines(null), WAIT_MS);
      })
      .then(
        (u) => (gone ? u() : (off = u)),
        () => {},
      );
    return () => {
      gone = true;
      off?.();
      clearTimeout(timer);
    };
  }, [workflowId]);
  if (!lines) return null;

  const answer = async (send: boolean) => {
    if (sent.current) return;
    sent.current = true;
    setBusy(true);
    // false(대기 없음·이미 시간 초과)나 실패여도 닫는다 — 답할 대상이 없거나, 백엔드가 시간 초과로 건너뛴다.
    await api.answerHttpConfirm(workflowId, send).catch(() => false);
    setLines(null);
    setBusy(false);
  };
  return <HttpConfirm lines={lines} skipLabel={t("http.confirm.skip")} busy={busy} onAnswer={(s) => void answer(s)} />;
}
