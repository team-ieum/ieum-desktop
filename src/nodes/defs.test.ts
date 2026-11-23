import { expect, test } from "vitest";
import type { WfNode } from "../api";
import { DEFS, PALETTE, mergeGithubFields, newBranchKey, newNode, nextPosition, outputKeys } from "./defs";

test("defs_has_57_and_transform_map_outputs_follow_mappings", () => {
  expect(Object.keys(DEFS).length).toBe(59);
  expect(DEFS["github.submit_pr_review"].inputFields.find((f) => f.key === "author")?.choices?.map((c) => c.id)).toEqual(["me"]);
  expect(DEFS["github.get_pr_files"].inputFields.find((f) => f.key === "max_files")?.type).toBe("integer");
  const map = (config: Record<string, unknown>): WfNode => ({ id: "t", event: "transform.map", type: "TRANSFORM", config });
  expect(outputKeys(map({ mappings: { a: "1", b: "2" } }))).toEqual(["a", "b"]);
  expect(outputKeys(map({}))).toEqual([]);
  expect(outputKeys(map({ mappings: "x" }))).toEqual([]);
  expect(outputKeys(map({ mappings: ["a"] }))).toEqual([]);
  expect(newNode("http.request", { x: 0, y: 0 }, "h").type).toBe("HTTP");
  expect(newNode("formatter.text_trim", { x: 0, y: 0 }, "t").type).toBe("TRANSFORM");
});

test("new_branch_key_format", () => {
  for (let i = 0; i < 20; i++) expect(newBranchKey()).toMatch(/^b_[0-9a-f]{8}$/);
});

test("add_places_new_node_right_of_rightmost", () => {
  const n = (id: string, position?: { x: number; y: number }): WfNode => ({ id, event: "local.file_trash", position, config: {} });
  expect(nextPosition([])).toEqual({ x: 0, y: 0 });
  expect(nextPosition([n("a", { x: 0, y: 40 }), n("b", { x: 320, y: 40 })])).toEqual({ x: 640, y: 40 });
  // 끌어 옮긴 노드가 가장 오른쪽이면 그 노드 기준.
  expect(nextPosition([n("a", { x: 900, y: 200 }), n("b", { x: 320, y: 0 })])).toEqual({ x: 1220, y: 200 });
  // 위치 없는 노드는 Canvas 기본값(i*320, 0)으로 본다.
  expect(nextPosition([n("a"), n("b")])).toEqual({ x: 640, y: 0 });
});

test("github_defs_merge_table_and_catalog", () => {
  const f = DEFS["github.find_issue"].inputFields;
  expect(f.map((x) => x.key)).toEqual(["owner", "repo", "search_by", "value", "state", "max_results"]);
  const max = f.find((x) => x.key === "max_results")!;
  expect(max.type).toBe("integer");
  expect(max.default).toBe(20);
  expect(f.find((x) => x.key === "owner")!.required).toBe(true);
  expect(DEFS["github.create_issue"].inputFields.find((x) => x.key === "body")!.type).toBe("text");
});

test("github_get_file_fields", () => {
  expect(DEFS["github.get_file"].inputFields.map((x) => x.key)).toEqual(["owner", "repo", "path", "ref"]);
});

test("merge_github_fields_desktop_last", () => {
  const out = mergeGithubFields(
    [{ key: "author", desktop: { default: "me", choices: [{ id: "me", name: "내 GitHub 계정" }] } }],
    { author: { title: "리뷰 작성자", default: "bot", choices: [{ id: "bot", name: "IEUM 봇" }, { id: "me", name: "내 GitHub 계정" }] } },
  );
  expect(out.length).toBe(1);
  expect(out[0].default).toBe("me");
  expect(out[0].choices?.length).toBe(1);
  expect("desktop" in out[0]).toBe(false);
});

test("github_triggers_use_owner_repo_fields", () => {
  for (const e of ["github.new_issue", "github.new_pull_request"]) {
    expect(DEFS[e].node_type).toBe("TRIGGER");
    expect(PALETTE).toContain(e);
  }
  const f = DEFS["github.new_issue"].inputFields;
  expect(f.map((x) => x.key)).toEqual(["owner", "repo"]);
  expect(f[0].optionsSource).toBe("github.owners");
  expect(f[1].optionsSource).toBe("github.repos");
  expect(f[1].optionsInputs).toEqual(["owner"]);
  expect(DEFS["github.new_pull_request"].inputFields.map((x) => x.key)).toEqual([
    "owner", "repo", "onOpened", "onSynchronize", "onReopened", "onReadyForReview", "excludeDrafts",
  ]);
});
