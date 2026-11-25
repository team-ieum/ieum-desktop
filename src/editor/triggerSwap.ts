import type { WfNode, Workflow } from "../api";
import { DEFS } from "../nodes/defs";

export const isTrigger = (n: WfNode) => n.type?.toUpperCase() === "TRIGGER";

/** 팔레트에서 고른 노드를 어떻게 넣을지 — 워크플로우는 시작 조건이 정확히 하나여야 한다. */
export type AddPlan =
  | { kind: "append" } // 트리거 아님, 또는 이미 트리거 2개 이상(사람이 고칠 몫)
  | { kind: "prepend" } // 트리거 0개에서 트리거
  | { kind: "select"; id: string } // 트리거 1개, 같은 event
  | { kind: "ask"; old: WfNode }; // 트리거 1개, 다른 event

export function planAdd(wf: Workflow, event: string): AddPlan {
  if (DEFS[event]?.node_type !== "TRIGGER") return { kind: "append" };
  const triggers = wf.nodes.filter(isTrigger);
  if (triggers.length === 0) return { kind: "prepend" };
  if (triggers.length > 1) return { kind: "append" };
  const [old] = triggers;
  return old.event === event ? { kind: "select", id: old.id } : { kind: "ask", old };
}

/** 첫 트리거는 맨 앞·가장 왼쪽 노드의 왼쪽에 — 자리 없는 노드는 nextPosition과 같은 i*320으로 친다. */
export function prependTrigger(wf: Workflow, node: WfNode): Workflow {
  const at = wf.nodes.map((n, i) => n.position ?? { x: i * 320, y: 0 });
  const left = at.length ? at.reduce((a, b) => (b.x < a.x ? b : a)) : null;
  const position = left ? { x: left.x - 320, y: left.y } : { x: 0, y: 0 };
  return { ...wf, nodes: [{ ...node, position }, ...wf.nodes] };
}

/** 같은 id·자리에서 노드만 바꾼다 — 엣지는 id로 이어져 그대로 남는다. 옛 자리가 없으면 키도 두지 않는다. */
export function replaceTrigger(wf: Workflow, oldId: string, fresh: WfNode): Workflow {
  const { position: _p, ...rest } = fresh;
  return {
    ...wf,
    nodes: wf.nodes.map((n) => (n.id === oldId ? { ...rest, id: oldId, ...(n.position ? { position: n.position } : {}) } : n)),
  };
}

/** 다른 노드 중 이 노드의 결과(`{{nodes.<id>.output.…}}`)를 쓰는 것. */
export function referencingNodes(wf: Workflow, id: string): WfNode[] {
  const ref = `{{nodes.${id}.output.`;
  return wf.nodes.filter((n) => n.id !== id && JSON.stringify(n.config).includes(ref));
}
