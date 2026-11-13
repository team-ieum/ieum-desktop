/** 채팅 자리 — 갈래 D가 채운다. 닫혀도 언마운트되지 않고 `visible`만 바뀐다. */
export function ChatSlot({ visible }: { workflowId: string; visible: boolean }) {
  return <div data-slot="chat" hidden={!visible} className="min-h-0 flex-1" />;
}
