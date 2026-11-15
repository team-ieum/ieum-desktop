import { useRef, useState } from "react";
import { api, type HttpLine, type WfNode } from "../api";

const method = (n: WfNode) => (typeof n.config.method === "string" && n.config.method ? n.config.method : "GET").toUpperCase();

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
    setAsk(
      http.map((n) => ({
        nodeId: n.id,
        label: n.label ?? "",
        method: method(n),
        url: typeof n.config.url === "string" ? n.config.url : "",
      })),
    );
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
