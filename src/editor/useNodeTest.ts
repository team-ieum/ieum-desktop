import { useEffect, useRef, useState } from "react";
import { api, type HttpLine, type NodeTestResult, type Sample, type WfNode, type Workflow } from "../api";
import { githubLine, httpLine, isGithubWrite } from "./useTestRun";

/** 실행하지 못한 테스트(명령 오류·파일 고르기 실패) — 검증 실패처럼 문구만 보이고 샘플은 그대로 둔다. */
const notRun = (error: string): NodeTestResult => ({
  status: "invalid",
  output: null,
  error,
  errorCode: null,
  testedAt: null,
  missing: [],
  undone: false,
  undoFailed: 0,
  runId: null,
});

/**
 * 노드 패널 [테스트] — 편집본의 이 노드 하나만 돌린다(`testNode`). 앞 노드 값은 앞 노드의 샘플.
 * 결과 상태는 샘플에서: 열 때 `getSamples`로 읽고, 실제로 실행된 결과(성공·실패)만 샘플을 바꾼다.
 * GET이 아닌 HTTP·GitHub 쓰기 노드는 그 노드 한 줄을 `ask`에 올리고 `answer(true)`해야 돌린다.
 */
export function useNodeTest(workflowId: string | null, workflow: Workflow, node: WfNode) {
  const [sample, setSample] = useState<Sample | null>(null);
  const [result, setResult] = useState<NodeTestResult | null>(null);
  const [running, setRunning] = useState(false);
  const [ask, setAsk] = useState<HttpLine[] | null>(null);
  // 카드에 보인 요청 그대로 보내도록, 카드를 띄운 때의 편집본·노드를 잡아 둔다.
  const asked = useRef<{ workflow: Workflow; node: WfNode } | null>(null);

  useEffect(() => {
    if (!workflowId) return;
    let stale = false;
    api.getSamples(workflowId).then(
      (s) => !stale && setSample(s[node.id] ?? null),
      (e) => !stale && setResult(notRun(String(e))),
    );
    return () => {
      stale = true;
    };
  }, [workflowId, node.id]);

  const go = async (workflow: Workflow, node: WfNode) => {
    if (!workflowId) return;
    // 폴더 트리거는 테스트할 파일을 고른다.
    const folder = node.event === "local.folder_new_file";
    const file = folder ? await api.pickPath(false).catch((e) => (setResult(notRun(String(e))), null)) : null;
    if (folder && !file) return;
    setRunning(true);
    try {
      const r = await (file ? api.testNode(workflowId, workflow, node.id, file) : api.testNode(workflowId, workflow, node.id));
      setResult(r);
      if (r.status === "succeeded" || r.status === "failed")
        setSample({
          status: r.status,
          output: r.output,
          error: r.error,
          errorCode: r.errorCode,
          testedAt: r.testedAt ?? new Date().toISOString(),
          node: { event: node.event, config: node.config },
        });
    } catch (e) {
      setResult(notRun(String(e)));
    } finally {
      setRunning(false);
    }
  };

  const run = async () => {
    const line = isGithubWrite(node) ? githubLine(node, workflow.nodes) : node.event === "http.request" ? httpLine(node, workflow.nodes) : null;
    if (line && line.method !== "GET") {
      asked.current = { workflow, node };
      return setAsk([line]);
    }
    return go(workflow, node);
  };
  // 두 번 눌러도 한 번만 돌린다.
  const asking = useRef(false);
  asking.current = !!ask;
  const answer = (send: boolean) => {
    if (!asking.current) return;
    asking.current = false;
    setAsk(null);
    const snap = asked.current;
    asked.current = null;
    if (send && snap) void go(snap.workflow, snap.node);
  };
  return { sample, result, running, run, ask, answer };
}
