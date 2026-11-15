import { useRef, useState } from "react";
import { api, type HttpLine, type WfNode } from "../api";

const method = (n: WfNode) => (typeof n.config.method === "string" && n.config.method ? n.config.method : "GET").toUpperCase();

/** 엔진 `render::VARIABLE`과 같은 모양. */
const REF = /\{\{nodes\.([a-zA-Z0-9-]+)\.output\.([a-zA-Z0-9_.]+)\}\}/g;
const refsIn = (v: unknown): [string, string][] =>
  typeof v === "string"
    ? [...v.matchAll(REF)].map((m) => [m[1], m[2]])
    : typeof v === "object" && v !== null
      ? Object.values(v).flatMap(refsIn)
      : [];

/** 빌더 `finish::http_line`과 같은 줄 — 머리말은 이름만, 참조식은 어느 노드의 어느 출력인지만. */
function httpLine(n: WfNode, nodes: WfNode[]): HttpLine {
  const headers = n.config.headers;
  const sends: HttpLine["sends"] = [];
  for (const [nodeId, output] of ["url", "headers", "body"].flatMap((k) => refsIn(n.config[k]))) {
    if (sends.some((s) => s.nodeId === nodeId && s.output === output)) continue;
    sends.push({ nodeId, nodeLabel: nodes.find((m) => m.id === nodeId)?.label ?? "", output });
  }
  return {
    nodeId: n.id,
    label: n.label ?? "",
    method: method(n),
    url: typeof n.config.url === "string" ? n.config.url : "",
    headerKeys: typeof headers === "object" && headers !== null && !Array.isArray(headers) ? Object.keys(headers).sort() : [],
    sends,
    hasBody: n.config.body !== undefined && n.config.body !== null,
  };
}

/**
 * 파일을 골라 한 번 실행 — 편집기 툴바와 실행 보기가 같이 쓴다.
 * `nodes`는 `runNow`가 실제로 돌리는 저장본의 노드다. GET이 아닌 HTTP 요청이 있으면 `ask`에 줄을 올리고
 * 사람이 `answer(true)`해야 돌린다(되돌릴 수 없는 요청).
 */
export function useTestRun(workflowId: string, nodes: WfNode[]) {
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ask, setAsk] = useState<HttpLine[] | null>(null);
  const go = async () => {
    setError(null);
    const path = await api.pickPath(false).catch((e) => (setError(String(e)), null));
    if (!path) return;
    setRunning(true);
    try {
      await api.runNow(workflowId, path);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };
  const run = async () => {
    const http = nodes.filter((n) => n.event === "http.request");
    if (!http.some((n) => method(n) !== "GET")) return go();
    setAsk(http.map((n) => httpLine(n, nodes)));
  };
  // 두 번 눌러도 한 번만 돌린다.
  const asking = useRef(false);
  asking.current = !!ask;
  const answer = (send: boolean) => {
    if (!asking.current) return;
    asking.current = false;
    setAsk(null);
    if (send) void go();
  };
  return { running, error, run, ask, answer };
}
