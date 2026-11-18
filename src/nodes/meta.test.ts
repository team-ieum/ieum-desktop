import { beforeAll, expect, test } from "vitest";
import { setLang } from "../i18n";
import { nodeMeta, toneStyle } from "./meta";

beforeAll(() => setLang("ko"));

test("node_meta_app_nodes_are_pink", () => {
  const slack = nodeMeta("slack.send_message", "ACTION");
  expect(slack.color).toBe("#c75146");
  expect(slack.label).toBe("Slack");
  const move = nodeMeta("local.file_move", "ACTION");
  expect(move.color).toBe("#287fa4");
  expect(move.label).toBe("파일 작업");
  expect(nodeMeta("local.ai", "AI").color).toBe("#6d5ce7");
  expect(nodeMeta("local.folder_new_file", "TRIGGER").label).toBe("시작 조건");
  expect(nodeMeta("branch.if", "CONDITION").color).toBe("#9a6700");
});

test("node_meta_github_dark_head", () => {
  const gh = nodeMeta("github.create_issue", "ACTION");
  expect(gh.label).toBe("GitHub");
  expect([gh.color, gh.tint, gh.head, gh.shadow]).toEqual(["#24292f", "#f6f8fa", "#24292f", "#d0d7de"]);
  const s = toneStyle(gh) as Record<string, string>;
  expect([s["--node-head"], s["--node-head-fg"], s["--node-icon-bg"], s["--node-icon-fg"]]).toEqual(["#24292f", "#fff", "#fff", "#24292f"]);
  // 브랜드 색이 없는 노드는 예전 그대로(머리띠 = tint, 아이콘 상자 = color).
  const slack = toneStyle(nodeMeta("slack.send_message", "ACTION")) as Record<string, string>;
  expect([slack["--node-head"], slack["--node-head-fg"], slack["--node-icon-bg"], slack["--node-shadow"]]).toEqual(["#fce8e5", "#c75146", "#c75146", "#fce8e5"]);
});

test("node_meta_builtin_colors", () => {
  expect(nodeMeta("formatter.text_trim", undefined).color).toBe("#287fa4");
  expect(nodeMeta("formatter.text_trim", undefined).tint).toBe("#e2f2f8");
  expect(nodeMeta("transform.map", undefined).color).toBe("#287fa4");
  expect(nodeMeta("http.request", undefined).color).toBe("#c75146");
  expect(nodeMeta("http.request", undefined).tint).toBe("#fce8e5");
  expect(nodeMeta("http.request", undefined).label).toBe("HTTP");
});
