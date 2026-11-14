import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../../api";
import symbol from "../../assets/symbol.png";
import { t } from "../../i18n";
import { ChatSlot } from "./ChatSlot";
import { TerminalSlot } from "./TerminalSlot";

export type AssistantMode = "terminal" | "chat";
/** localStorage — 마지막으로 고른 입구. */
export const MODE_KEY = "ieum.assistant.mode";
const MODES: AssistantMode[] = ["terminal", "chat"];

function lastMode(): string | null {
  try {
    return localStorage.getItem(MODE_KEY);
  } catch {
    return null;
  }
}

/** 기억한 입구가 지금 쓸 수 있으면 그것, 아니면 쓸 수 있는 다른 쪽. 둘 다 안 되면 null. 모르는 값은 채팅으로 본다. */
export function availableMode(saved: string | null, ok: { terminal: boolean; chat: boolean }): AssistantMode | null {
  const want: AssistantMode = saved === "terminal" ? "terminal" : "chat";
  const other: AssistantMode = want === "terminal" ? "chat" : "terminal";
  return ok[want] ? want : ok[other] ? other : null;
}

/** 캔버스 우상단 마스코트 버튼 + 떠 있는 패널(캔버스를 밀지 않는다). 한 번 연 뒤에는 닫아도 숨기기만 한다 — 터미널 세션이 살아 있어야 한다. */
export function AssistantPanel({
  workflowId,
  onOpenChange,
  nodeNames,
}: {
  workflowId: string;
  onOpenChange?: (open: boolean) => void;
  /** 노드 id → 이름. 채팅의 테스트 결과 카드가 쓴다. */
  nodeNames?: Record<string, string>;
}) {
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState(false);
  const [saved, setSaved] = useState(lastMode);
  // 터미널을 쓸 수 있나(설치·로그인된 CLI가 있거나 세션이 살아 있다). 확인 전(null)에는 숨기지 않는다.
  const [terminalOk, setTerminalOk] = useState<boolean | null>(null);
  useEffect(() => {
    if (!opened || !workflowId) return;
    let gone = false;
    api
      .termTools(workflowId)
      .then((r) => !gone && setTerminalOk(r.tools.length > 0 || r.running !== null))
      .catch(() => !gone && setTerminalOk(false));
    return () => {
      gone = true;
    };
  }, [opened, workflowId]);
  const terminal = terminalOk !== false;
  // 채팅 탭은 늘 있다(AI 연결이 없으면 채팅 자리 안에서 알린다).
  const mode = availableMode(saved, { terminal, chat: true }) ?? "chat";
  // 기억은 사용자가 탭을 누를 때만 바뀐다.
  const pick = (m: AssistantMode) => {
    setSaved(m);
    try {
      localStorage.setItem(MODE_KEY, m);
    } catch {
      // 저장하지 못해도 이번 선택은 그대로 쓴다.
    }
  };
  return (
    <>
      <button
        type="button"
        onClick={() => {
          setOpen(true);
          setOpened(true);
          onOpenChange?.(true);
        }}
        aria-label={t("assistant.open")}
        title={t("assistant.open")}
        aria-expanded={open}
        className="absolute right-4 top-4 z-10 grid h-12 w-12 cursor-pointer place-items-center rounded-full bg-main-deep-blue transition-opacity hover:opacity-90"
        style={{ boxShadow: "0 8px 24px -4px rgba(41, 83, 124, 0.45), 0 4px 8px -2px rgba(16, 24, 40, 0.1)" }}
      >
        <img src={symbol} alt="" className="h-7 w-7 object-contain" />
      </button>
      {opened && (
        <aside
          aria-label="IEUM Assistant"
          hidden={!open}
          className={`absolute right-4 top-4 bottom-4 z-30 flex flex-col overflow-hidden rounded-2xl border border-neutral-200 bg-white ${
            mode === "terminal" ? "w-[560px]" : "w-[380px]"
          }`}
          style={{ boxShadow: "0 24px 48px -8px rgba(16, 24, 40, 0.18), 0 8px 16px -4px rgba(16, 24, 40, 0.1)" }}
        >
          <div className="flex shrink-0 items-center gap-3 bg-main-deep-blue px-4 py-3.5 text-white">
            <img src={symbol} alt="" className="h-5 w-5 object-contain" />
            <h2 className="min-w-0 flex-1 text-sm font-semibold">IEUM Assistant</h2>
            <div role="tablist" className="flex gap-1 rounded-[10px] bg-neutral-100 p-1">
              {MODES.filter((m) => m !== "terminal" || terminal).map((m) => (
                <button
                  key={m}
                  type="button"
                  role="tab"
                  aria-selected={mode === m}
                  onClick={() => pick(m)}
                  className={`cursor-pointer rounded-lg px-3 py-1 typo-caption1_semibold ${
                    mode === m ? "bg-white text-neutral-900 shadow-sm" : "text-neutral-500 hover:text-neutral-700"
                  }`}
                >
                  {t(`assistant.tab.${m}`)}
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                onOpenChange?.(false);
              }}
              aria-label={t("assistant.close")}
              title={t("assistant.close")}
              className="grid h-7 w-7 shrink-0 cursor-pointer place-items-center rounded-lg transition-opacity hover:opacity-80"
              style={{ background: "rgba(255,255,255,.12)" }}
            >
              <X size={14} aria-hidden="true" />
            </button>
          </div>
          {workflowId && (
            <>
              <TerminalSlot workflowId={workflowId} visible={open && mode === "terminal"} />
              <ChatSlot workflowId={workflowId} visible={open && mode === "chat"} nodeNames={nodeNames} />
            </>
          )}
        </aside>
      )}
    </>
  );
}
