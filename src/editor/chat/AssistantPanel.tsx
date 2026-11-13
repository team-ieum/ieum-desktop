import { X } from "lucide-react";
import { useState } from "react";
import symbol from "../../assets/symbol.png";
import { t } from "../../i18n";
import { ChatSlot } from "./ChatSlot";
import { TerminalSlot } from "./TerminalSlot";

export type AssistantMode = "terminal" | "chat";
/** localStorage — 마지막으로 고른 입구. */
export const MODE_KEY = "ieum.assistant.mode";
const MODES: AssistantMode[] = ["terminal", "chat"];

function lastMode(): AssistantMode {
  try {
    const v = localStorage.getItem(MODE_KEY);
    return v === "terminal" ? v : "chat";
  } catch {
    return "chat";
  }
}

/** 캔버스 우상단 마스코트 버튼 + 떠 있는 패널(캔버스를 밀지 않는다). 한 번 연 뒤에는 닫아도 숨기기만 한다 — 터미널 세션이 살아 있어야 한다. */
export function AssistantPanel({ workflowId, onOpenChange }: { workflowId: string; onOpenChange?: (open: boolean) => void }) {
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState(false);
  const [mode, setMode] = useState<AssistantMode>(lastMode);
  const pick = (m: AssistantMode) => {
    setMode(m);
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
        className="absolute right-4 top-4 z-10 grid size-12 cursor-pointer place-items-center rounded-full border border-neutral-200 bg-white shadow-[0_8px_20px_rgba(41,83,124,0.3)] transition-opacity hover:opacity-90"
      >
        <img src={symbol} alt="" className="size-7" />
      </button>
      {opened && (
        <aside
          aria-label="IEUM Assistant"
          hidden={!open}
          className={`absolute right-4 top-4 bottom-4 z-30 flex flex-col overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-[0_14px_40px_rgba(43,72,86,0.18)] ${
            mode === "terminal" ? "w-[560px]" : "w-[380px]"
          }`}
        >
          <div className="flex items-center gap-2.5 border-b border-neutral-200 px-5 py-3">
            <img src={symbol} alt="" className="size-6" />
            <h2 className="min-w-0 flex-1 typo-body2_semibold text-neutral-900">IEUM Assistant</h2>
            <div role="tablist" className="flex gap-1 rounded-[10px] bg-neutral-100 p-1">
              {MODES.map((m) => (
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
              className="grid size-8 shrink-0 place-items-center rounded-[10px] text-neutral-500 transition-colors hover:bg-neutral-100"
            >
              <X size={16} aria-hidden="true" />
            </button>
          </div>
          {workflowId && (
            <>
              <TerminalSlot workflowId={workflowId} visible={open && mode === "terminal"} />
              <ChatSlot workflowId={workflowId} visible={open && mode === "chat"} />
            </>
          )}
        </aside>
      )}
    </>
  );
}
