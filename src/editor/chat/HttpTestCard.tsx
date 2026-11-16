import { useEffect, useRef, useState } from "react";
import { api, type HttpLine } from "../../api";
import { t } from "../../i18n";
import { BUTTON, PRIMARY } from "../../screens/Editor";
import { nodeTitle } from "../Canvas";

/** 주소의 참조식을 가린다 — 화면 표시용. 값이 무엇이 될지는 사람이 알 수 없으니 자리만 보인다. */
export const showRefs = (url: string) => url.replace(/\{\{\s*nodes\.[^{}]*\}\}/g, t("http.confirm.earlier"));

/** 확인 카드·[켜기] 카드가 같이 쓰는 틀 — 줄이 많거나 주소가 길어도 제목·버튼은 화면 안에, 가운데만 스크롤. */
export const CARD_BOX =
  "absolute bottom-4 left-1/2 z-30 flex max-h-[calc(100%-2rem)] w-[420px] -translate-x-1/2 flex-col gap-3 rounded-2xl border border-neutral-200 bg-white p-5 shadow-[0_14px_40px_rgba(43,72,86,0.18)]";
export const CARD_SCROLL = "min-h-0 overflow-y-auto";

/** 스킴 · 계정·호스트 자리 · 나머지. 브라우저 파서처럼 `\`도 호스트 자리를 끝낸다. */
const AUTHORITY = /^([a-z][a-z0-9+.-]*:[/\\]*)([^/\\?#]*)([\s\S]*)$/i;

/** 실제로 연결될 호스트 — 계정 정보(`good.com@evil.com`)·혼동 글자에 속지 않게 브라우저 파서로 읽는다. 못 읽거나 참조식이면 null. */
function destination(url: string) {
  const m = url.match(AUTHORITY);
  if (!m || m[2].includes("{{")) return null;
  let host: string;
  try {
    host = new URL(url).host;
  } catch {
    return null;
  }
  if (!host) return null;
  const at = m[2].lastIndexOf("@");
  const typed = m[2].slice(at + 1);
  return { scheme: m[1], user: at >= 0, host, typed: typed.toLowerCase() === host ? null : typed, rest: showRefs(m[3]) };
}

/** HTTP 요청 한 줄 — 메서드·주소(참조식은 자리만)와 실려 나가는 것(머리말 이름·본문 유무·앞 노드 값 이름). */
export function HttpRequest({ line, prefix = "" }: { line: HttpLine; prefix?: string }) {
  const to = destination(line.url);
  const sends = line.sends.map((s) =>
    t("http.confirm.sendItem")
      .replace("{node}", () => s.nodeLabel || s.nodeId)
      .replace("{output}", () => s.output),
  );
  return (
    <>
      <span data-testid="http-request" className="break-all font-mono text-xs">
        {`${prefix}${line.method} `}
        {to ? (
          <>
            {to.scheme}
            {to.user && <span className="text-neutral-400">…@</span>}
            <strong className="font-bold text-neutral-900">{to.host}</strong>
            {to.typed && <span className="text-neutral-400">{` (${to.typed})`}</span>}
            {to.rest}
          </>
        ) : (
          showRefs(line.url)
        )}
      </span>
      {!to && <span className="text-xs font-semibold text-danger-700">{t("http.confirm.refHost")}</span>}
      {line.headerKeys.length > 0 && (
        <span className="text-xs text-neutral-500">{t("http.confirm.headers").replace("{names}", () => line.headerKeys.join(", "))}</span>
      )}
      {line.hasBody && <span className="text-xs text-neutral-500">{t("http.confirm.body")}</span>}
      {sends.length > 0 && (
        <span className="text-xs font-semibold text-danger-700">{t("http.confirm.sends").replace("{items}", () => sends.join(", "))}</span>
      )}
    </>
  );
}

/** 실제 요청을 보내기 전 확인 — 빌더 테스트(HttpTestCard)와 사람의 테스트 실행(useTestRun)이 같이 쓴다. */
export function HttpConfirm({
  lines,
  skipLabel,
  busy,
  changed,
  onAnswer,
}: {
  lines: HttpLine[];
  skipLabel: string;
  busy?: boolean;
  /** 떠 있는 동안 줄이 바뀌었다. */
  changed?: boolean;
  onAnswer: (send: boolean) => void;
}) {
  return (
    <div role="dialog" aria-label={t("http.confirm.title")} className={CARD_BOX}>
      <h2 className="typo-body2_bold text-neutral-900">{t("http.confirm.title")}</h2>
      {changed && (
        <p role="status" className="typo-caption1_semibold text-danger-700">
          {t("http.confirm.changed")}
        </p>
      )}
      <ul className={`flex flex-col gap-2 typo-caption1_medium text-neutral-700 ${CARD_SCROLL}`}>
        {lines.map((l) => (
          <li key={l.nodeId} className="flex flex-col gap-0.5">
            <span className="typo-caption1_semibold text-neutral-800">{l.label || nodeTitle("http.request")}</span>
            <HttpRequest line={l} />
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
/** 줄이 바뀐 뒤 버튼을 막는 시간. */
const LOCK_MS = 1000;

/** 빌더 테스트의 HTTP 확인 — 이 편집 화면의 워크플로우 것만 띄운다. 화면을 떠나면 사라진다(백엔드가 시간 초과로 취소). */
export function HttpTestCard({ workflowId }: { workflowId: string }) {
  const [lines, setLines] = useState<HttpLine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [changed, setChanged] = useState(false);
  const [locked, setLocked] = useState(false);
  const sent = useRef(false);
  const shown = useRef<HttpLine[] | null>(null);
  shown.current = lines;
  useEffect(() => {
    let gone = false;
    let off: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let lockTimer: ReturnType<typeof setTimeout> | undefined;
    api
      .onHttpTestConfirm((p) => {
        if (p.id !== workflowId) return;
        // 읽던 카드의 줄이 바뀌면 표시하고 잠깐 막는다 — 읽은 것과 다른 요청에 [보내기]가 눌리지 않게.
        if (shown.current && JSON.stringify(shown.current) !== JSON.stringify(p.lines)) {
          setChanged(true);
          setLocked(true);
          clearTimeout(lockTimer);
          lockTimer = setTimeout(() => setLocked(false), LOCK_MS);
        }
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
      clearTimeout(lockTimer);
    };
  }, [workflowId]);
  // 카드가 닫히면 바뀜 표시도 지운다 — 다음 카드는 새로 읽는 것.
  if (!lines && changed) setChanged(false);
  if (!lines) return null;

  const answer = async (send: boolean) => {
    if (sent.current || locked) return;
    sent.current = true;
    setBusy(true);
    // false(대기 없음·이미 시간 초과)나 실패여도 닫는다 — 답할 대상이 없거나, 백엔드가 시간 초과로 건너뛴다.
    await api.answerHttpConfirm(workflowId, send).catch(() => false);
    setLines(null);
    setBusy(false);
  };
  return (
    <HttpConfirm lines={lines} skipLabel={t("http.confirm.skip")} busy={busy || locked} changed={changed} onAnswer={(s) => void answer(s)} />
  );
}
