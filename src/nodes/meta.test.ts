import { beforeAll, expect, test } from "vitest";
import { setLang } from "../i18n";
import { nodeMeta } from "./meta";

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

test("node_meta_builtin_colors", () => {
  expect(nodeMeta("formatter.text_trim", undefined).color).toBe("#287fa4");
  expect(nodeMeta("formatter.text_trim", undefined).tint).toBe("#e2f2f8");
  expect(nodeMeta("transform.map", undefined).color).toBe("#287fa4");
  expect(nodeMeta("http.request", undefined).color).toBe("#c75146");
  expect(nodeMeta("http.request", undefined).tint).toBe("#fce8e5");
  expect(nodeMeta("http.request", undefined).label).toBe("HTTP");
});
