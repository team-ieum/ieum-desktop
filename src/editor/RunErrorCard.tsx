import { TriangleAlert, X } from "lucide-react";
import { useState } from "react";
import type { WfNode } from "../api";
import { t } from "../i18n";
import { type Live, opensAiSettings, opensIntegrations } from "../stores/runs";
import { nodeTitle } from "./Canvas";
import { useClosing } from "./useClosing";

/** 실행이 실패하면 우측 하단에 뜨는 카드 — 화면 밖 노드의 실패도 놓치지 않게. 부르는 쪽이 key={runId}로 실행마다 새로 그려 닫음을 실행 단위로 둔다. */
export function RunErrorCard({
  live,
  nodes,
  right,
  onShow,
  onOpenIntegrations,
  onOpenAiSettings,
}: {
  live: Live;
  nodes: WfNode[];
  right: number;
  onShow: (nodeId: string) => void;
  onOpenIntegrations?: () => void;
  onOpenAiSettings?: () => void;
}) {
  const [dismissed, setDismissed] = useState(false);
  const { closing, close } = useClosing(() => setDismissed(true));
  const idx = nodes.findIndex((n) => live.nodes[n.id] === "failed" && live.errors[n.id]);
  if (live.status === "cancelled" || idx < 0 || dismissed) return null;
  const node = nodes[idx];
  const name = node.label || nodeTitle(node.event);
  return (
    <div
      role="alert"
      style={{ right }}
      className={`absolute bottom-4 z-20 flex w-72 flex-col gap-2 rounded-[1rem] border-2 border-[#c2412d] bg-white p-3 shadow-[0.4rem_0.5rem_0_#f6d3cb,0_0_0_4px_rgba(194,65,45,.12)] ${
        closing ? "ieum-slide-out" : "ieum-grow-br"
      }`}
    >
      <div className="flex items-center gap-2">
        <TriangleAlert size={16} className="shrink-0 text-[#c2412d]" aria-hidden="true" />
        <span className="grid size-6 shrink-0 place-items-center rounded-full bg-[#c2412d] text-[0.7rem] font-bold text-white">{idx + 1}</span>
        <span className="min-w-0 flex-1 truncate text-[0.82rem] font-bold text-[#27273b]">{t("editor.runError.stoppedAt").replace("{name}", name)}</span>
        <button type="button" aria-label={t("editor.runError.dismiss")} onClick={close} className="shrink-0 text-neutral-500 hover:text-neutral-800">
          <X size={14} aria-hidden="true" />
        </button>
      </div>
      <div className="flex gap-2">
        {opensIntegrations(live.codes[node.id]) && onOpenIntegrations && (
          <button type="button" onClick={onOpenIntegrations} className="rounded-brand-sm bg-black px-2.5 py-1 text-[0.72rem] font-bold text-white">
            {t("editor.runError.openIntegrations")}
          </button>
        )}
        {opensAiSettings(live.codes[node.id]) && onOpenAiSettings && (
          <button type="button" onClick={onOpenAiSettings} className="rounded-brand-sm bg-black px-2.5 py-1 text-[0.72rem] font-bold text-white">
            {t("editor.runError.openAiSettings")}
          </button>
        )}
        <button type="button" onClick={() => onShow(node.id)} className="rounded-brand-sm border border-neutral-300 px-2.5 py-1 text-[0.72rem] font-bold text-[#27273b]">
          {t("editor.runError.show")}
        </button>
      </div>
    </div>
  );
}
