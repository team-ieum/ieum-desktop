import { Info, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type TermCliState, type TermTool, type TermTools } from "../../api";
import { type LogoId, ProviderLogo } from "../../assets/providers";
import { t } from "../../i18n";

/** localStorage — 마지막으로 고른 CLI. */
export const TOOL_KEY = "ieum.assistant.tool";
// 고유명사라 번역하지 않는다.
const TOOL_NAME: Record<TermTool, string> = { claude: "Claude Code", codex: "Codex", agy: "Antigravity" };
const TOOL_LOGO: Record<TermTool, LogoId> = { claude: "claude", codex: "openai", agy: "antigravity" };
/** 고를 수 있는 상태. */
const USABLE: TermCliState[] = ["ready", "unknown"];
const STATE_CLASS: Record<TermCliState, string> = {
  ready: "bg-[#e5f3ec] text-[#287357]",
  unknown: "bg-main-light-blue text-main-deep-blue",
  notInstalled: "bg-[#eef1f3] text-[#5a6975]",
  loggedOut: "bg-[#fff4e0] text-[#8a5a12]",
};
/** 스킬 설치 알림이 떠 있는 시간. */
export const NOTE_MS = 6000;
/** xterm 색 — 패널의 남색에 맞춘 어두운 바탕. */
const XTERM_THEME = {
  background: "#0f1b26",
  foreground: "#e3ebf2",
  cursor: "#5cc8ee",
  cursorAccent: "#0f1b26",
  selectionBackground: "#29537c99",
};

/** 지금 화면에 붙어 있는 xterm 한 벌. */
interface Live {
  dispose(): void;
}

function lastTool(): string | null {
  try {
    return localStorage.getItem(TOOL_KEY);
  } catch {
    return null;
  }
}

/** 시작 전 — CLI 카드. 쓸 수 있는 것만 누를 수 있고, 누르면 바로 시작한다. */
function Picker({ clis, last, onPick }: { clis: TermTools["clis"]; last: string | null; onPick: (tool: TermTool) => void }) {
  const usable = clis.filter((c) => USABLE.includes(c.state));
  // 지난번 것을 강조한다. 지난번 것을 못 쓰면 쓸 수 있는 첫 번째.
  const hot = usable.find((c) => c.tool === last)?.tool ?? usable[0]?.tool;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-0.5">
        <p className="typo-body2_semibold text-main-deep-blue">{t("assistant.terminal.pick")}</p>
        <p className="typo-caption1_medium text-neutral-500">{t("assistant.terminal.pickHint")}</p>
      </div>
      <ul className="flex flex-col gap-2">
        {clis.map(({ tool, state }) => {
          const ok = USABLE.includes(state);
          return (
            <li key={tool}>
              <button
                type="button"
                disabled={!ok}
                onClick={() => onPick(tool)}
                aria-label={t("assistant.terminal.startWith").replace("{name}", TOOL_NAME[tool])}
                aria-describedby={`cli-${tool}-state`}
                className={`flex w-full items-start gap-3 rounded-xl border bg-white p-3 text-left transition-colors ${
                  !ok
                    ? "cursor-not-allowed border-neutral-200 opacity-60"
                    : tool === hot
                      ? "cursor-pointer border-main-blue shadow-[0_0_0_3px_rgba(0,123,167,.12)] hover:bg-main-light-blue/30"
                      : "cursor-pointer border-[#d4e4ec] hover:border-main-blue hover:bg-main-light-blue/30"
                }`}
              >
                <ProviderLogo id={TOOL_LOGO[tool]} size={34} />
                <span className="flex min-w-0 flex-1 flex-col gap-1">
                  <span className="flex items-center gap-1.5">
                    <span className="typo-body3_semibold text-neutral-900">{TOOL_NAME[tool]}</span>
                    {ok && tool === last && (
                      <span className="rounded-full bg-main-blue px-1.5 py-px text-[10px] font-semibold text-white">{t("assistant.terminal.last")}</span>
                    )}
                    <span id={`cli-${tool}-state`} className={`ml-auto shrink-0 rounded-full px-2 py-0.5 text-[11px] font-semibold ${STATE_CLASS[state]}`}>
                      {t(`assistant.terminal.state.${state}`)}
                    </span>
                  </span>
                  {tool === "agy" && ok && (
                    <span className="flex items-start gap-1 typo-caption2_medium text-neutral-500">
                      <Info size={12} className="mt-px shrink-0" aria-hidden="true" />
                      {t("assistant.terminal.agyNote")}
                    </span>
                  )}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {usable.length === 0 && <p className="typo-caption1_medium text-neutral-600">{t("assistant.terminal.none")}</p>}
    </div>
  );
}

/**
 * 앱 안 터미널. 세션(PTY)은 Rust에 살아서, 닫아도(`visible=false`) 화면을 떠나도(언마운트) 끝나지 않는다.
 * 끝나는 때는 [세션 종료]·CLI가 스스로 끝남·워크플로우 삭제·로그아웃·앱 종료뿐이다.
 */
export function TerminalSlot({ workflowId, visible }: { workflowId: string; visible: boolean }) {
  const [clis, setClis] = useState<TermTools["clis"] | null>(null);
  const [tool, setTool] = useState<TermTool | null>(null);
  const [live, setLive] = useState(false);
  const [ended, setEnded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [skillNote, setSkillNote] = useState(false);
  const el = useRef<HTMLDivElement>(null);
  const term = useRef<Live | null>(null);
  const unlisten = useRef<Promise<() => void> | null>(null);
  const asked = useRef(false);
  const gone = useRef(false);

  const close = () => {
    term.current?.dispose();
    term.current = null;
    setLive(false);
  };

  /** 세션을 띄우거나(없으면) 살아 있는 세션에 붙는다 — 어느 쪽인지는 Rust가 정한다. */
  const open = async (which: TermTool) => {
    setTool(which);
    setLive(true);
    setEnded(false);
    setError(null);
    let observer: ResizeObserver | null = null;
    try {
      // 세션이 끝나면(스스로 끝남·종료 버튼·삭제) 고르기 화면으로 돌아간다. 첫 세션 때 한 번만 건다.
      unlisten.current ??= api.onTermExit((p) => {
        if (p.workflowId !== workflowId || gone.current) return;
        close();
        setEnded(true);
      });
      // 동적 import — 터미널을 안 쓰면 본 번들에 싣지 않는다.
      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import("@xterm/xterm"),
        import("@xterm/addon-fit"),
        import("@xterm/xterm/css/xterm.css"),
      ]);
      if (gone.current || !el.current) return;
      const x = new Terminal({
        fontFamily: '"SF Mono", ui-monospace, Menlo, monospace',
        fontSize: 13,
        lineHeight: 1.25,
        theme: XTERM_THEME,
        cursorBlink: true,
      });
      term.current = {
        dispose: () => {
          observer?.disconnect();
          x.dispose();
        },
      };
      const fit = new FitAddon();
      x.loadAddon(fit);
      x.open(el.current);
      fit.fit();
      const r = await api.termSpawn(workflowId, which, x.cols, x.rows, (b) => x.write(b));
      if (gone.current) return;
      x.onData((d) => void api.termWrite(workflowId, d).catch(() => {}));
      let sent = `${x.cols}x${x.rows}`;
      observer = new ResizeObserver(() => {
        fit.fit(); // 숨겨져 크기를 모를 때는 아무것도 안 한다
        const now = `${x.cols}x${x.rows}`;
        if (now === sent) return;
        sent = now;
        void api.termResize(workflowId, x.cols, x.rows).catch(() => {});
      });
      observer.observe(el.current);
      setTool(r.tool);
      if (r.skillInstalled) setSkillNote(true);
    } catch (e) {
      close();
      setError(String(e));
    }
  };

  // 스킬 설치 알림은 잠깐만 보인다.
  useEffect(() => {
    if (!skillNote) return;
    const timer = setTimeout(() => setSkillNote(false), NOTE_MS);
    return () => clearTimeout(timer);
  }, [skillNote]);

  // 처음 보일 때 한 번: CLI 상태를 묻고, 살아 있는 세션이 있으면 묻지 않고 붙는다.
  useEffect(() => {
    if (!visible || asked.current) return;
    asked.current = true;
    api
      .termTools(workflowId)
      .then((r) => {
        if (gone.current) return;
        setClis(r.clis);
        if (r.running) void open(r.running);
      })
      .catch((e) => {
        if (gone.current) return;
        setClis([]);
        setError(String(e));
      });
  }, [visible, workflowId]);

  useEffect(() => {
    gone.current = false;
    return () => {
      // 화면만 정리한다 — 세션은 죽이지 않는다.
      gone.current = true;
      term.current?.dispose();
      term.current = null;
      void unlisten.current?.then((f) => f()).catch(() => {});
      unlisten.current = null;
    };
  }, []);

  const start = (which: TermTool) => {
    try {
      localStorage.setItem(TOOL_KEY, which);
    } catch {
      // 저장하지 못해도 이번 선택은 그대로 쓴다.
    }
    void open(which);
  };

  return (
    <div data-slot="terminal" hidden={!visible} className="flex min-h-0 flex-1 flex-col bg-[#f7fafd]">
      {live && tool && (
        <div className="flex shrink-0 items-center gap-2 border-b border-[#e3eef4] bg-white px-3 py-1.5">
          <span className="inline-flex min-w-0 items-center gap-1.5 rounded-full border border-[#d4e4ec] bg-white py-0.5 pr-2.5 pl-1 typo-caption1_semibold text-neutral-800">
            <ProviderLogo id={TOOL_LOGO[tool]} size={18} />
            <span className="truncate">{TOOL_NAME[tool]}</span>
            <span className="size-1.5 shrink-0 rounded-full bg-[#2f9e6e]" aria-hidden="true" />
          </span>
          <button
            type="button"
            onClick={() => void api.termKill(workflowId).catch(() => {})}
            className="ml-auto cursor-pointer rounded-md px-2 py-1 typo-caption2_semibold text-neutral-500 transition-colors hover:bg-danger-100 hover:text-danger-700"
          >
            {t("assistant.terminal.stop")}
          </button>
        </div>
      )}
      {!live && clis && (
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
          {ended && <p className="rounded-xl bg-white px-3 py-2 typo-caption1_medium text-neutral-600 ring-1 ring-[#e3eef4]">{t("assistant.terminal.ended")}</p>}
          {error && (
            <p role="alert" className="rounded-xl border border-danger-300 bg-danger-100 px-3 py-2 typo-caption1_medium text-danger-700">
              {t("assistant.terminal.failed")} {error}
            </p>
          )}
          <Picker clis={clis} last={lastTool()} onPick={start} />
        </div>
      )}
      <div hidden={!live} className="relative m-3 mt-2.5 min-h-0 flex-1 overflow-hidden rounded-xl p-2.5 ring-1 ring-black/10" style={{ background: XTERM_THEME.background }}>
        {skillNote && (
          <div role="status" className="absolute top-2 right-2 left-2 z-10 flex items-center gap-2 rounded-lg bg-white/95 px-3 py-2 typo-caption1_medium text-neutral-700 shadow-lg">
            <span className="min-w-0 flex-1">{t("assistant.terminal.skillInstalled")}</span>
            <button
              type="button"
              onClick={() => setSkillNote(false)}
              aria-label={t("assistant.terminal.dismiss")}
              className="grid size-5 shrink-0 cursor-pointer place-items-center rounded text-neutral-500 hover:bg-neutral-100"
            >
              <X size={12} aria-hidden="true" />
            </button>
          </div>
        )}
        <div ref={el} className="h-full w-full" />
      </div>
    </div>
  );
}
