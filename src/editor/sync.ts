import type { StoredWorkflow } from "../api";
import { nextPosition } from "../nodes/defs";

/** 채팅이 열린 뒤 캔버스 편집을 저장하기까지 기다리는 시간. */
export const AUTOSAVE_MS = 500;
/** 저장본이 그사이 바뀌었을 때 `save_workflow`가 주는 오류 문구. */
export const STALE = "STALE";

/** 저장본으로 바꾸되 자리는 지킨다: 같은 id는 화면 자리, 없으면 저장본 자리, 둘 다 없으면 nextPosition으로 차례로. */
export function mergeRemote(local: StoredWorkflow, remote: StoredWorkflow): StoredWorkflow {
  const screen = new Map(local.workflow.nodes.map((n) => [n.id, n.position]));
  const kept = remote.workflow.nodes.map((n) => ({ ...n, position: screen.get(n.id) ?? n.position }));
  const placed = kept.filter((n) => n.position);
  const nodes = kept.map((n) => {
    if (n.position) return n;
    const put = { ...n, position: nextPosition(placed) };
    placed.push(put);
    return put;
  });
  return { ...remote, workflow: { ...remote.workflow, nodes } };
}
