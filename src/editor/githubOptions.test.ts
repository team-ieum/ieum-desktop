// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { api, type OptionList } from "../api";
import type { Field } from "../nodes/defs";
import { optionsState, useGithubOptions } from "./githubOptions";

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const fields: Field[] = [
  { key: "owner", title: "소유자", type: "string", optionsSource: "github.owners" },
  { key: "repo", title: "저장소", type: "string", optionsSource: "github.repos", optionsInputs: ["owner"] },
  { key: "head", title: "보낼 브랜치", type: "string", optionsSource: "github.branches", optionsInputs: ["owner", "repo"] },
];
const [owner, repo, head] = fields;
const LIST: OptionList = { items: [{ id: "a", name: "a", hint: null, updatedAt: null }], truncated: false };
const ready = (inputs: Record<string, string>) => ({ kind: "ready" as const, inputs });

test("blocked_states", () => {
  expect(optionsState(owner, {}, fields)).toEqual({ kind: "ready", inputs: {} });
  expect(optionsState(repo, { owner: "  " }, fields)).toEqual({ kind: "blocked", reason: "empty", title: "소유자" });
  expect(optionsState(repo, {}, fields)).toEqual({ kind: "blocked", reason: "empty", title: "소유자" });
  expect(optionsState(repo, { owner: "{{t.owner}}" }, fields)).toEqual({ kind: "blocked", reason: "reference", title: "소유자" });
  expect(optionsState(repo, { owner: "a b" }, fields)).toEqual({ kind: "blocked", reason: "invalid", title: "소유자" });
  expect(optionsState(repo, { owner: "x".repeat(40) }, fields)).toMatchObject({ reason: "invalid" });
  expect(optionsState(repo, { owner: " hodoon " }, fields)).toEqual({ kind: "ready", inputs: { owner: "hodoon" } });
  expect(optionsState(head, { owner: "hodoon", repo: ".." }, fields)).toEqual({ kind: "blocked", reason: "invalid", title: "저장소" });
  expect(optionsState(head, { owner: "hodoon", repo: "a/b" }, fields)).toMatchObject({ reason: "invalid", title: "저장소" });
  expect(optionsState(head, { owner: "hodoon", repo: "" }, fields)).toMatchObject({ reason: "empty", title: "저장소" });
  expect(optionsState(head, { owner: "hodoon", repo: " my.repo-1 " }, fields)).toEqual({
    kind: "ready",
    inputs: { owner: "hodoon", repo: "my.repo-1" },
  });
});

test("caches_60s", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  const call = vi.spyOn(api, "githubOptions").mockResolvedValue(LIST);
  const st = ready({ owner: "cache60" });
  const a = renderHook(() => useGithubOptions("github.repos", st, { open: false }));
  await waitFor(() => expect(a.result.current.list).toEqual(LIST));
  // 입력 순서가 달라도 같은 묶음
  renderHook(() => useGithubOptions("github.repos", { kind: "ready", inputs: { owner: "cache60" } }, { open: true }));
  expect(call).toHaveBeenCalledTimes(1);
  vi.advanceTimersByTime(60_001);
  const c = renderHook(() => useGithubOptions("github.repos", st, { open: true }));
  await waitFor(() => expect(call).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(c.result.current.loading).toBe(false));
  expect(call).toHaveBeenCalledWith("github.repos", { owner: "cache60" });
});

test("dedupes_concurrent_loads", async () => {
  let done!: (l: OptionList) => void;
  const call = vi.spyOn(api, "githubOptions").mockReturnValue(new Promise((r) => (done = r)));
  const st = ready({ owner: "dedupe", repo: "r" });
  const a = renderHook(() => useGithubOptions("github.branches", st, { open: true }));
  const b = renderHook(() => useGithubOptions("github.branches", ready({ repo: "r", owner: "dedupe" }), { open: true }));
  expect(call).toHaveBeenCalledTimes(1);
  expect(a.result.current.loading).toBe(true);
  expect(b.result.current.loading).toBe(true);
  await act(async () => done(LIST));
  expect(a.result.current.list).toEqual(LIST);
  expect(b.result.current.list).toEqual(LIST);
  expect(b.result.current.loading).toBe(false);
});

test("prefetch_error_hidden_until_open", async () => {
  vi.spyOn(api, "githubOptions").mockRejectedValue({ code: "GITHUB_NOT_CONNECTED", message: "연결 필요" });
  const st = ready({ owner: "prefetchErr" });
  const h = renderHook(({ open }) => useGithubOptions("github.repos", st, { open }), { initialProps: { open: false } });
  await waitFor(() => expect(h.result.current.loading).toBe(false));
  expect(h.result.current.error).toBeNull();
  // 열면 오류 항목을 다시 부르고, 그 결과 오류가 보인다
  h.rerender({ open: true });
  await waitFor(() => expect(h.result.current.error).toEqual({ code: "GITHUB_NOT_CONNECTED", message: "연결 필요" }));
  expect(api.githubOptions).toHaveBeenCalledTimes(2);
});

test("blocked_does_not_load", () => {
  const call = vi.spyOn(api, "githubOptions");
  const h = renderHook(() => useGithubOptions("github.repos", { kind: "blocked", reason: "empty", title: "소유자" }, { open: true }));
  expect(call).not.toHaveBeenCalled();
  expect(h.result.current).toMatchObject({ list: null, loading: false, error: null });
});

test("retry_bypasses_cache", async () => {
  const call = vi.spyOn(api, "githubOptions").mockRejectedValueOnce({ code: null, message: "잠시 뒤" }).mockResolvedValue(LIST);
  const st = ready({ owner: "retry" });
  const h = renderHook(() => useGithubOptions("github.repos", st, { open: true }));
  await waitFor(() => expect(h.result.current.error).toEqual({ code: null, message: "잠시 뒤" }));
  await act(async () => h.result.current.retry());
  await waitFor(() => expect(h.result.current.list).toEqual(LIST));
  expect(h.result.current.error).toBeNull();
  expect(call).toHaveBeenCalledTimes(2);
  // 성공해 캐시에 있어도 다시 시도는 바로 부른다
  await act(async () => h.result.current.retry());
  expect(call).toHaveBeenCalledTimes(3);
});
