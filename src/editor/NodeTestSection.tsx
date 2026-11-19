import { Play } from "lucide-react";
import { createPortal } from "react-dom";
import type { WfNode, Workflow } from "../api";
import { locale, t } from "../i18n";
import { DEFS } from "../nodes/defs";
import { BUTTON } from "../screens/Editor";
import { nodeTitle } from "./Canvas";
import { HttpConfirm } from "./chat/HttpTestCard";
import { OutputView } from "./OutputView";
import { useNodeTest } from "./useNodeTest";

// 색은 시안 B 값 그대로(대응 토큰 없음).
const NOTE = "rounded-brand-sm px-3 py-2.5 text-[12.5px] leading-normal";
const INFO = `${NOTE} bg-[#e8eef6] text-[#27466b]`;
const WARN = `${NOTE} bg-[#fff7e0] text-[#9a6b00]`;
const ERR = `${NOTE} bg-danger-100 text-danger-700`;
const OK = "#2f7a55";

/** 칸 순서와 상관없이 같은 값이면 같은 글. */
const canon = (v: unknown) =>
  JSON.stringify(v, (_k, x) =>
    x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) : x,
  );

/** 1분 안이면 "방금", 오늘이면 "오후 3:04", 그 밖은 "11월 18일 오후 9:12". */
function when(iso: string, now = Date.now()): string {
  const d = new Date(iso);
  if (Math.abs(now - d.getTime()) < 60_000) return t("editor.justNow");
  const today = d.toDateString() === new Date(now).toDateString();
  const o: Intl.DateTimeFormatOptions = today ? { hour: "numeric", minute: "2-digit" } : { month: "long", day: "numeric", hour: "numeric", minute: "2-digit" };
  return new Intl.DateTimeFormat(locale(), o).format(d);
}

/**
 * 노드 패널의 테스트 영역(시안 B) — 이 노드만 실행하고 마지막 테스트 결과(샘플)를 보인다.
 * `lastFailed`·`lastError`는 마지막 전체 실행에서 이 노드가 실패했을 때, `chunk`는 전체 실행 중 AI 출력 조각.
 * `host`가 있으면 확인 카드를 그 안(패널 밖)에 띄운다.
 */
export function NodeTestSection({
  workflowId,
  workflow,
  node,
  onOpen,
  lastFailed,
  lastError,
  chunk,
  host,
}: {
  workflowId: string | null;
  workflow: Workflow;
  node: WfNode;
  onOpen: (nodeId: string) => void;
  lastFailed?: boolean;
  lastError?: string;
  chunk?: string;
  host?: HTMLElement | null;
}) {
  const { sample, result, running, run, ask, answer } = useNodeTest(workflowId, workflow, node);
  const ok = sample?.status === "succeeded";
  const stale = sample && canon(sample.node) !== canon({ event: node.event, config: node.config });
  const ran = result?.status === "succeeded" || result?.status === "failed";
  const card = ask && <HttpConfirm lines={ask} skipLabel={t("common.cancel")} onAnswer={answer} />;
  return (
    <section className="flex flex-col gap-2 border-t border-neutral-200 bg-neutral-50 p-5">
      <div className="flex items-center justify-between gap-2">
        <h3 className="typo-body2_bold text-neutral-800">{t("editor.test")}</h3>
        <button
          type="button"
          className={BUTTON}
          onClick={() => void run()}
          disabled={!workflowId || running}
          title={workflowId ? t("editor.testHint") : t("run.saveFirst")}
        >
          <Play size={14} aria-hidden="true" />
          {t("editor.test")}
        </button>
      </div>
      <p className="typo-caption1_regular text-neutral-500">{t("editor.testHint")}</p>
      {lastFailed && (
        <p className="typo-caption1_semibold text-danger-700">
          {t("editor.lastRunFailed")}
          {lastError ? ` · ${lastError}` : ""}
        </p>
      )}
      {result?.status === "missing" && (
        <>
          <div className={INFO}>{t("editor.testMissing")}</div>
          {result.missing.map((id) => {
            const m = workflow.nodes.find((n) => n.id === id);
            return (
              <div key={id} className="flex items-center justify-between rounded-brand-sm border border-neutral-200 bg-white px-2.5 py-2 text-[12.5px]">
                <span>
                  {m ? m.label || nodeTitle(m.event) : id} <span className="text-neutral-500">· {t("editor.testNoSample")}</span>
                </span>
                <button
                  type="button"
                  className="rounded-brand-sm border border-neutral-300 bg-white px-2 py-[3px] text-xs font-semibold text-neutral-700 hover:bg-neutral-100"
                  onClick={() => onOpen(id)}
                >
                  {t("editor.openNode")}
                </button>
              </div>
            );
          })}
        </>
      )}
      {result?.status === "invalid" && <div className={ERR}>{result.error}</div>}
      {sample ? (
        <>
          <div className="flex items-center gap-1.5 text-[13px] font-semibold" style={{ color: ok ? OK : "var(--color-danger-700)" }}>
            <span aria-hidden="true" className="grid size-4 place-items-center rounded-full text-[10px] text-white" style={{ background: ok ? OK : "var(--color-danger-700)" }}>
              {ok ? "✓" : "!"}
            </span>
            {t(ok ? "status.succeeded" : "status.failed")}
            <span className="ml-auto text-xs font-normal text-neutral-500">{when(sample.testedAt)}</span>
          </div>
          {stale && <div className={WARN}>{t("editor.testOutdated")}</div>}
          {ok ? sample.output && <OutputView output={sample.output} fields={DEFS[node.event]?.outputFields} /> : <div className={ERR}>{sample.error}</div>}
          {ran && result.undone && <div className={INFO}>{t("editor.testUndone")}</div>}
        </>
      ) : (
        !result && <p className="typo-body3_regular text-neutral-500">{t("editor.noResult")}</p>
      )}
      {chunk && (
        <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-brand-sm border border-neutral-200 bg-white p-2.5 font-mono text-xs text-neutral-700">{chunk}</pre>
      )}
      {card && (host ? createPortal(card, host) : card)}
    </section>
  );
}
