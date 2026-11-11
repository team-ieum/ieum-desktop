// 노드 정의(Task 2와 같은 파일) — 앱 번들에 포함한다.
import branchIf from "../../catalog/events/branch/if.json";
import localAi from "../../crates/ieum-local/defs/local.ai.json";
import fileMove from "../../crates/ieum-local/defs/local.file_move.json";
import fileRename from "../../crates/ieum-local/defs/local.file_rename.json";
import fileTrash from "../../crates/ieum-local/defs/local.file_trash.json";
import folderNewFile from "../../crates/ieum-local/defs/local.folder_new_file.json";
import type { Workflow, WfNode } from "../api";

export interface Field {
  key: string;
  title?: string;
  type: string;
  required?: boolean;
  ref?: boolean;
  list?: boolean;
  default?: unknown;
  description?: string;
  choices?: { id: string; name: string }[];
  children?: Field[];
}

export interface NodeDef {
  event: string;
  node_type: "TRIGGER" | "AI" | "ACTION" | "CONDITION";
  inputFields: Field[];
  outputFields?: { key: string }[];
}

const raw: [string, unknown][] = [
  ["local.folder_new_file", folderNewFile],
  ["local.ai", localAi],
  ["local.file_move", fileMove],
  ["local.file_rename", fileRename],
  ["local.file_trash", fileTrash],
  ["branch.if", branchIf],
];

/** 팔레트 순서 그대로. */
export const DEFS: Record<string, NodeDef> = Object.fromEntries(
  raw.map(([event, d]) => [event, { ...(d as Omit<NodeDef, "event">), event }]),
);
export const PALETTE = raw.map(([e]) => e);

/** 폴더 고르기 버튼만 쓰는 칸(사람이 고른 폴더 — 참조·직접 입력 금지). */
export const FOLDER_FIELDS = new Set(["folder", "baseFolder"]);

export const isLocal = (event: string) => event.startsWith("local.");

/** i18n 키 — `path`는 목록 칸 안이면 `branches.rules.left`처럼 부모 키를 잇는다. */
export const fieldKey = (event: string, path: string) => `field.${event}.${path}`;

/** 계약: 갈래 key = `b_` + 16진 8자. */
export function newBranchKey(): string {
  const b = crypto.getRandomValues(new Uint8Array(4));
  return "b_" + [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
}

export function defaultValue(f: Field): unknown {
  if (f.default !== undefined) return f.default;
  if (f.list) return [];
  if (f.type === "boolean") return false;
  return "";
}

export function newItem(fields: Field[], parent: string): Record<string, unknown> {
  const item: Record<string, unknown> = {};
  for (const c of fields) {
    if (c.key === "key" && parent === "branches") item.key = newBranchKey();
    else if (c.list && c.children) item[c.key] = [newItem(c.children, c.key)];
    else item[c.key] = c.choices && c.default === undefined ? c.choices[0].id : defaultValue(c);
  }
  return item;
}

export function newNode(event: string, position: { x: number; y: number }, label: string): WfNode {
  const def = DEFS[event];
  const config: Record<string, unknown> = {};
  for (const f of def.inputFields) if (f.default !== undefined) config[f.key] = f.default;
  if (event === "branch.if") config.branches = [newItem(def.inputFields.find((f) => f.key === "branches")!.children!, "branches")];
  return { id: crypto.randomUUID(), event, type: def.node_type, label, position, config };
}

/** 새 노드 자리 — 가장 오른쪽 노드의 오른쪽(Canvas 기본 배치와 같은 320 간격). */
export function nextPosition(nodes: WfNode[]): { x: number; y: number } {
  const at = nodes.map((n, i) => n.position ?? { x: i * 320, y: 0 });
  if (at.length === 0) return { x: 0, y: 0 };
  const right = at.reduce((a, b) => (b.x > a.x ? b : a));
  return { x: right.x + 320, y: right.y };
}

/** 조건 노드의 나가는 갈래 값(엣지 conditionType) — 엔진 `condition_branches`와 같은 규칙. */
export function branchHandles(node: WfNode): { id: string; name: string }[] {
  const branches = node.config.branches;
  if (!Array.isArray(branches) || branches.length === 0)
    return [
      { id: "true", name: "true" },
      { id: "false", name: "else" },
    ];
  const out = branches
    .filter((b) => typeof b?.key === "string" && b.key)
    .map((b) => ({ id: b.key as string, name: String(b.name ?? "") }));
  if (node.config.otherwise !== "stop") out.push({ id: "else", name: "else" });
  return out;
}

/** 노드가 내는 출력 칸 이름. */
export function outputKeys(node: WfNode): string[] {
  if (node.event === "local.ai") {
    const of = node.config.outputFields;
    const keys = Array.isArray(of) ? of.map((f) => f?.key).filter((k): k is string => typeof k === "string" && !!k) : [];
    return keys.length ? keys : ["output"];
  }
  return DEFS[node.event]?.outputFields?.map((f) => f.key) ?? [];
}

/** `id`보다 앞(엣지를 거슬러 닿는) 노드들. */
export function upstream(wf: Workflow, id: string): WfNode[] {
  const seen = new Set<string>();
  const stack = [id];
  while (stack.length) {
    const cur = stack.pop()!;
    for (const e of wf.edges) if (e.target === cur && !seen.has(e.source)) seen.add(e.source), stack.push(e.source);
  }
  seen.delete(id);
  return wf.nodes.filter((n) => seen.has(n.id));
}
