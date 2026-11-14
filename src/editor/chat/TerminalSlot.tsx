import { useEffect, useRef, useState } from "react";
import { api, type TermTool } from "../../api";
import { t } from "../../i18n";

/** localStorage — 마지막으로 고른 CLI. */
export const TOOL_KEY = "ieum.assistant.tool";
// 고유명사라 번역하지 않는다.
const TOOL_NAME: Record<TermTool, string> = { claude: "Claude Code", codex: "Codex", agy: "Antigravity" };

/** 지금 화면에 붙어 있는 xterm 한 벌. */
interface Live {
  dispose(): void;
}

/**
 * 앱 안 터미널. 세션(PTY)은 Rust에 살아서, 닫아도(`visible=false`) 화면을 떠나도(언마운트) 끝나지 않는다.
 * 끝나는 때는 [세션 종료]·CLI가 스스로 끝남·워크플로우 삭제·로그아웃·앱 종료뿐이다.
 */
export function TerminalSlot({ workflowId, visible }: { workflowId: string; visible: boolean }) {
  const [tools, setTools] = useState<TermTool[] | null>(null);
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
      const x = new Terminal({ fontFamily: "ui-monospace, Menlo, monospace", fontSize: 13 });
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

  // 처음 보일 때 한 번: 쓸 수 있는 CLI를 묻고, 살아 있는 세션이 있으면 묻지 않고 붙는다.
  useEffect(() => {
    if (!visible || asked.current) return;
    asked.current = true;
    api
      .termTools(workflowId)
      .then((r) => {
        if (gone.current) return;
        setTools(r.tools);
        let last: string | null = null;
        try {
          last = localStorage.getItem(TOOL_KEY);
        } catch {
          // 기억을 못 읽으면 첫 번째를 고른다.
        }
        setTool(r.running ?? r.tools.find((x) => x === last) ?? r.tools[0] ?? null);
        if (r.running) void open(r.running);
      })
      .catch((e) => {
        if (gone.current) return;
        setTools([]);
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

  const start = () => {
    if (!tool) return;
    try {
      localStorage.setItem(TOOL_KEY, tool);
    } catch {
      // 저장하지 못해도 이번 선택은 그대로 쓴다.
    }
    void open(tool);
  };

  return (
    <div data-slot="terminal" hidden={!visible} className="flex min-h-0 flex-1 flex-col">
      {live && tool && (
        <div className="flex items-center gap-2 border-b border-neutral-200 px-4 py-2">
          <span className="min-w-0 flex-1 truncate typo-caption1_semibold text-neutral-700">{TOOL_NAME[tool]}</span>
          <button
            type="button"
            onClick={() => void api.termKill(workflowId).catch(() => {})}
            className="cursor-pointer rounded-lg border border-neutral-200 px-2.5 py-1 typo-caption1_semibold text-neutral-700 hover:bg-neutral-100"
          >
            {t("assistant.terminal.stop")}
          </button>
        </div>
      )}
      {skillNote && (
        <p className="px-4 py-2 typo-caption1_medium text-neutral-500">{t("assistant.terminal.skillInstalled")}</p>
      )}
      {!live && tools && (
        <div className="flex flex-col gap-3 p-5">
          {ended && <p className="typo-body3_medium text-neutral-500">{t("assistant.terminal.ended")}</p>}
          {error && (
            <p role="alert" className="typo-body3_medium text-danger-700">
              {t("assistant.terminal.failed")} {error}
            </p>
          )}
          {tools.length > 0 && tool && (
            <div className="flex items-center gap-2">
              <select
                aria-label={t("assistant.terminal.tool")}
                value={tool}
                onChange={(e) => setTool(e.target.value as TermTool)}
                className="min-w-0 flex-1 rounded-[10px] border border-neutral-200 bg-white px-3 py-2 typo-body3_medium text-neutral-900"
              >
                {tools.map((x) => (
                  <option key={x} value={x}>
                    {TOOL_NAME[x]}
                  </option>
                ))}
              </select>
              <button
                type="button"
                onClick={start}
                className="cursor-pointer rounded-[10px] bg-main-blue px-4 py-2 typo-body3_semibold text-white hover:opacity-90"
              >
                {t("assistant.terminal.start")}
              </button>
            </div>
          )}
        </div>
      )}
      <div ref={el} hidden={!live} className="min-h-0 flex-1 overflow-hidden bg-black p-2" />
    </div>
  );
}
