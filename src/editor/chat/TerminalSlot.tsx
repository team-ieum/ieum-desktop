/** 터미널 자리 — 갈래 C가 채운다. 닫혀도 언마운트되지 않고 `visible`만 바뀐다. */
export function TerminalSlot({ visible }: { workflowId: string; visible: boolean }) {
  return <div data-slot="terminal" hidden={!visible} className="min-h-0 flex-1" />;
}
