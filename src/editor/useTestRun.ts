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

/** 칸들의 참조식을 등장 순서·중복 없이 — 어느 노드의 어느 출력인지만. */
function sendsOf(values: unknown[], nodes: WfNode[]): HttpLine["sends"] {
  const sends: HttpLine["sends"] = [];
  for (const [nodeId, output] of values.flatMap(refsIn)) {
    if (sends.some((s) => s.nodeId === nodeId && s.output === output)) continue;
    sends.push({ nodeId, nodeLabel: nodes.find((m) => m.id === nodeId)?.label ?? "", output });
  }
  return sends;
}

/** 빌더 `finish::http_line`과 같은 줄 — 머리말은 이름만, 참조식은 어느 노드의 어느 출력인지만. */
function httpLine(n: WfNode, nodes: WfNode[]): HttpLine {
  const headers = n.config.headers;
  const sends = sendsOf(["url", "headers", "body"].map((k) => n.config[k]), nodes);
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

const GITHUB_WRITE = [
  "create_issue",
  "update_issue",
  "find_or_create_issue",
  "add_labels",
  "create_comment",
  "create_pull_request",
  "update_pull_request",
  "request_reviewers",
  "find_or_create_pull_request",
  "submit_pr_review",
].map((k) => `github.${k}`);
const isGithubWrite = (n: WfNode) => GITHUB_WRITE.includes(n.event);
/** 칸 원문 — 참조식·AI가 JSON 숫자로 넣은 번호도 글로(빌더 `finish::raw`와 같게). */
const raw = (n: WfNode, k: string) => {
  const v = n.config[k];
  return typeof v === "string" || typeof v === "number" ? String(v) : "";
};

/** 빌더 `finish::github_line`과 같은 줄 — config 전체의 참조식을 `sends`로. */
function githubLine(n: WfNode, nodes: WfNode[]): HttpLine {
  const create = ["create_issue", "find_or_create_issue", "create_pull_request", "find_or_create_pull_request"].some((k) => n.event === `github.${k}`);
  const byPull = ["update_pull_request", "request_reviewers", "submit_pr_review"].some((k) => n.event === `github.${k}`);
  return {
    nodeId: n.id,
    label: n.label ?? "",
    method: "",
    url: "",
    headerKeys: [],
    sends: sendsOf(Object.values(n.config), nodes),
    hasBody: raw(n, "body").trim() !== "",
    github: { event: n.event, repo: `${raw(n, "owner")}/${raw(n, "repo")}`, summary: create ? raw(n, "title") : `#${raw(n, byPull ? "pull_number" : "issue_number")}` },
  };
}

/** 파일 없이 바로 도는 트리거(스케줄·수동). 그 밖(폴더)은 파일을 고른다. */
export const runsWithoutFile = (nodes: WfNode[]) => nodes.some((n) => n.event.startsWith("schedule.") || n.event === "manual.run");

/**
 * 한 번 실행(폴더 트리거는 파일을 골라서) — 편집기 툴바와 실행 보기가 같이 쓴다.
 * `nodes`는 `runNow`가 실제로 돌리는 저장본의 노드다. GET이 아닌 HTTP 요청이나 GitHub 쓰기 노드가 있으면 `ask`에 줄을 올리고
 * 사람이 `answer(true)`해야 돌린다(되돌릴 수 없는 요청).
 */
export function useTestRun(workflowId: string, nodes: WfNode[]) {
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ask, setAsk] = useState<HttpLine[] | null>(null);
  const go = async () => {
    setError(null);
    const direct = runsWithoutFile(nodes);
    const path = direct ? null : await api.pickPath(false).catch((e) => (setError(String(e)), null));
    if (!direct && !path) return;
    setRunning(true);
    try {
      await (path ? api.runNow(workflowId, path) : api.runNow(workflowId));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };
  const run = async () => {
    const asked = nodes.filter((n) => n.event === "http.request" || isGithubWrite(n));
    if (!asked.some((n) => isGithubWrite(n) || method(n) !== "GET")) return go();
    setAsk(asked.map((n) => (isGithubWrite(n) ? githubLine(n, nodes) : httpLine(n, nodes))));
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
